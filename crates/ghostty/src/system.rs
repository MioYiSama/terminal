//! Process-wide library hooks: logging, PNG decoding, and secure random.
//!
//! Installing or clearing a hook is **unsafe**: libghostty-vt changes ordinary
//! process-global function pointers, without synchronization. Do this at startup,
//! before other threads use the library. Otherwise, exclude **all** concurrent
//! library calls, including calls through `ghostty-sys` or other wrappers, for
//! the entire installation, with synchronization establishing visibility to
//! subsequent library users. Never install a hook from inside a callback.
//!
//! Callbacks may run concurrently on any thread. Global storage owns installed
//! closures until replacement or clearing; captures must not borrow local data.
//! Panics are contained: log messages are
//! discarded and PNG/random operations fail. A panic payload whose destructor
//! itself panics aborts the process rather than unwinding through C.

use core::ffi::c_void;
use core::marker::PhantomData;
use core::ptr;
use std::panic::{AssertUnwindSafe, catch_unwind};

use ghostty_sys as sys;

use crate::ffi::{self, PanicSlot};
use crate::{Error, Result};

ffi::c_enum! {
    /// Severity of an internal library log message.
    pub enum LogLevel {
        /// An operation failed.
        Error = sys::GHOSTTY_SYS_LOG_LEVEL_ERROR,
        /// A potentially problematic condition.
        Warning = sys::GHOSTTY_SYS_LOG_LEVEL_WARNING,
        /// Informational output.
        Info = sys::GHOSTTY_SYS_LOG_LEVEL_INFO,
        /// Debug output (compiled out in release builds of the library).
        Debug = sys::GHOSTTY_SYS_LOG_LEVEL_DEBUG,
    }
}

type LogHandler = dyn Fn(LogLevel, &[u8], &[u8]) + Send + Sync + 'static;
type PngDecoder = dyn Fn(&[u8], &mut PngOutput<'_>) -> Result<()> + Send + Sync + 'static;
type SecureRandom = dyn Fn(&mut [u8]) -> bool + Send + Sync + 'static;

// These are only written under the unsafe installation contract. During normal
// library use they are immutable, and the owned closures themselves are Sync.
// Installations replace the slot before dropping the old closure, so its
// destructor may safely make ordinary library calls using the new configuration.
static mut LOG_HANDLER: Option<Box<LogHandler>> = None;
static mut PNG_DECODER: Option<Box<PngDecoder>> = None;
static mut SECURE_RANDOM: Option<Box<SecureRandom>> = None;

/// Reads a hook slot from a library callback.
///
/// # Safety
///
/// No installation may run concurrently (the installation contract).
unsafe fn installed<T: ?Sized>(slot: *const Option<Box<T>>) -> Option<&'static T> {
    unsafe { (*slot).as_deref() }
}

/// Installs a log handler receiving `(level, scope, message)` as borrowed bytes.
///
/// The byte slices are valid only during the call; an empty scope is unscoped.
/// Global storage owns the closure until replacement or clearing. Emitted levels
/// depend on the library build mode, not a runtime filter.
///
/// # Safety
/// No other libghostty-vt call or hook installation may run concurrently, through
/// any API. Do not call this from a library callback. See the module documentation.
pub unsafe fn set_log_handler<F>(handler: F) -> Result<()>
where
    F: Fn(LogLevel, &[u8], &[u8]) + Send + Sync + 'static,
{
    let handler = Box::new(handler);
    // SAFETY: the caller excludes all concurrent library use; this is a function
    // pointer, not a pointer to a function-pointer variable (sys.h's convention).
    unsafe {
        set_callback(sys::GHOSTTY_SYS_OPT_LOG, log_trampoline as *const () as _)?;
        drop(ptr::replace(&raw mut LOG_HANDLER, Some(handler)));
    }
    Ok(())
}

/// Disables logging and drops any previously installed Rust closure.
///
/// # Safety
/// Exclude all other libghostty-vt calls and installations for this call, and
/// never invoke it from a library callback.
pub unsafe fn clear_log_handler() -> Result<()> {
    // SAFETY: the caller provides exclusive process-wide library access.
    unsafe {
        set_callback(sys::GHOSTTY_SYS_OPT_LOG, ptr::null())?;
        drop(ptr::replace(&raw mut LOG_HANDLER, None));
    }
    Ok(())
}

/// Routes library logs to the built-in stderr logger.
///
/// Messages are formatted as `[level](scope): message\n` (without the scope
/// suffix for unscoped messages). On freestanding/Wasm targets this is a no-op.
/// Any previously installed Rust log closure is dropped.
///
/// # Safety
/// Exclude all other libghostty-vt calls and installations for this call, and
/// never invoke it from a library callback.
pub unsafe fn use_stderr_logging() -> Result<()> {
    // SAFETY: the caller provides exclusive process-wide library access.
    unsafe {
        set_callback(
            sys::GHOSTTY_SYS_OPT_LOG,
            sys::ghostty_sys_log_stderr as *const () as _,
        )?;
        drop(ptr::replace(&raw mut LOG_HANDLER, None));
    }
    Ok(())
}

/// Writes one message with the library's built-in stderr formatting.
///
/// This does not install or change a global callback.
pub fn log_stderr(level: LogLevel, scope: &[u8], message: &[u8]) {
    // SAFETY: both slices remain readable for this synchronous call. The built-in
    // logger ignores userdata and does not modify global callback configuration.
    unsafe {
        sys::ghostty_sys_log_stderr(
            ptr::null_mut(),
            level.to_raw(),
            scope.as_ptr(),
            scope.len(),
            message.as_ptr(),
            message.len(),
        );
    }
}

/// Installs a PNG decoder, enabling PNG Kitty graphics support.
///
/// Decode the input bytes into [`PngOutput::allocate`]'s RGBA buffer and return
/// `Ok(())`. Returning an error, panicking, or not allocating an image rejects
/// the image. Allocation is limited by the allocator supplied by the library.
/// Global storage owns the closure until replacement or clearing.
///
/// # Safety
/// Exclude all other libghostty-vt calls and installations for this call, and
/// never invoke it from a library callback.
pub unsafe fn set_png_decoder<F>(decoder: F) -> Result<()>
where
    F: Fn(&[u8], &mut PngOutput<'_>) -> Result<()> + Send + Sync + 'static,
{
    let decoder = Box::new(decoder);
    // SAFETY: the caller provides exclusive process-wide library access.
    unsafe {
        set_callback(
            sys::GHOSTTY_SYS_OPT_DECODE_PNG,
            png_trampoline as *const () as _,
        )?;
        drop(ptr::replace(&raw mut PNG_DECODER, Some(decoder)));
    }
    Ok(())
}

/// Disables PNG decoding and drops any installed Rust decoder.
///
/// # Safety
/// Exclude all other libghostty-vt calls and installations for this call, and
/// never invoke it from a library callback.
pub unsafe fn clear_png_decoder() -> Result<()> {
    // SAFETY: the caller provides exclusive process-wide library access.
    unsafe {
        set_callback(sys::GHOSTTY_SYS_OPT_DECODE_PNG, ptr::null())?;
        drop(ptr::replace(&raw mut PNG_DECODER, None));
    }
    Ok(())
}

/// Installs a cryptographically secure random source.
///
/// Fill every byte and return `true`; return `false` when entropy is unavailable.
/// This overrides the platform source. Global storage owns the closure until
/// replacement or clearing. A predictable source compromises library secrets;
/// use a real CSPRNG.
///
/// # Safety
/// Exclude all other libghostty-vt calls and installations for this call, and
/// never invoke it from a library callback.
pub unsafe fn set_secure_random<F>(random: F) -> Result<()>
where
    F: Fn(&mut [u8]) -> bool + Send + Sync + 'static,
{
    let random = Box::new(random);
    // SAFETY: the caller provides exclusive process-wide library access.
    unsafe {
        set_callback(
            sys::GHOSTTY_SYS_OPT_RANDOM_SECURE,
            random_trampoline as *const () as _,
        )?;
        drop(ptr::replace(&raw mut SECURE_RANDOM, Some(random)));
    }
    Ok(())
}

/// Restores the platform random source, if available, and drops the old closure.
///
/// # Safety
/// Exclude all other libghostty-vt calls and installations for this call, and
/// never invoke it from a library callback.
pub unsafe fn clear_secure_random() -> Result<()> {
    // SAFETY: the caller provides exclusive process-wide library access.
    unsafe {
        set_callback(sys::GHOSTTY_SYS_OPT_RANDOM_SECURE, ptr::null())?;
        drop(ptr::replace(&raw mut SECURE_RANDOM, None));
    }
    Ok(())
}

unsafe fn set_callback(option: sys::GhosttySysOption, callback: *const c_void) -> Result<()> {
    // SAFETY: callers supply the function-pointer type specified by the option
    // and uphold the process-global installation contract.
    Error::check(unsafe { sys::ghostty_sys_set(option, callback) })
}

/// Callback-scoped decoded PNG output, allocated by the library's allocator.
///
/// The decoder borrows this value only for its invocation. Successful decoding
/// transfers its pixel buffer to the library; failure or panic frees the buffer
/// with the same allocator and exact allocation length. It cannot be constructed
/// outside a decoder callback or retained after the callback returns.
pub struct PngOutput<'a> {
    allocator: *const sys::GhosttyAllocator,
    image: sys::GhosttySysImage,
    _callback: PhantomData<&'a sys::GhosttyAllocator>,
}

impl PngOutput<'_> {
    /// Allocates tightly packed, top-to-bottom 8-bit RGBA pixels, initially zero.
    ///
    /// Returns `InvalidValue` for zero dimensions or a byte length exceeding
    /// `isize::MAX`, and `OutOfMemory` when the supplied allocator refuses the
    /// image. On success this replaces and frees any earlier output buffer.
    /// Fill the returned slice directly to avoid copying decoded pixels.
    pub fn allocate(&mut self, width: u32, height: u32) -> Result<&mut [u8]> {
        let len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .filter(|&len| width != 0 && height != 0 && len <= isize::MAX as usize)
            .ok_or(Error::InvalidValue)?;
        // SAFETY: the allocator is supplied by C and remains valid throughout
        // this callback. Only ghostty_alloc/free access its vtable.
        let data = unsafe { sys::ghostty_alloc(self.allocator, len) };
        if data.is_null() {
            return Err(Error::OutOfMemory);
        }
        // SAFETY: this is a fresh, uniquely owned allocation of exactly len
        // bytes. Initialize it before exposing a safe slice of u8.
        unsafe {
            ptr::write_bytes(data, 0, len);
            sys::ghostty_free(self.allocator, self.image.data, self.image.data_len);
        }
        self.image = sys::GhosttySysImage {
            width,
            height,
            data,
            data_len: len,
        };
        // SAFETY: initialization, uniqueness, and the slice size bound were
        // established above. The slice cannot outlive this mutable borrow.
        Ok(unsafe { core::slice::from_raw_parts_mut(data, len) })
    }
}

impl Drop for PngOutput<'_> {
    fn drop(&mut self) {
        // SAFETY: an untransferred image is still uniquely owned, and the
        // callback allocator is live. A null pointer is explicitly accepted.
        unsafe { sys::ghostty_free(self.allocator, self.image.data, self.image.data_len) };
    }
}

// Unlike per-operation callbacks, process-wide hooks have no outer Rust caller
// at which to resume a panic. Drop its payload inside a second unwind boundary:
// payload destructors are arbitrary user code too.
fn catch_callback<T>(f: impl FnOnce() -> T) -> Option<T> {
    let mut panic = PanicSlot::default();
    let result = panic.catch(f);
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(panic))) {
        core::mem::forget(payload);
        std::process::abort();
    }
    result
}

unsafe fn dispatch_log(
    handler: &LogHandler,
    level: sys::GhosttySysLogLevel,
    scope: *const u8,
    scope_len: usize,
    message: *const u8,
    message_len: usize,
) {
    let _ = catch_callback(|| {
        // SAFETY: C provides readable callback-scoped slices, including null
        // pointers for empty strings. No reference escapes the closure call.
        let scope = unsafe {
            ffi::string_bytes(sys::GhosttyString {
                ptr: scope,
                len: scope_len,
            })
        };
        let message = unsafe {
            ffi::string_bytes(sys::GhosttyString {
                ptr: message,
                len: message_len,
            })
        };
        handler(LogLevel::from_lib(level), scope, message);
    });
}

unsafe extern "C" fn log_trampoline(
    _userdata: *mut c_void,
    level: sys::GhosttySysLogLevel,
    scope: *const u8,
    scope_len: usize,
    message: *const u8,
    message_len: usize,
) {
    // SAFETY: installation excludes every library call, so the slot and owned
    // Sync closure are only borrowed immutably until this callback returns.
    if let Some(handler) = unsafe { installed(&raw const LOG_HANDLER) } {
        // SAFETY: all pointers and lengths come directly from C's callback.
        unsafe { dispatch_log(handler, level, scope, scope_len, message, message_len) };
    }
}

unsafe fn dispatch_png(
    decoder: &PngDecoder,
    allocator: *const sys::GhosttyAllocator,
    data: *const u8,
    data_len: usize,
    out: *mut sys::GhosttySysImage,
) -> bool {
    catch_callback(|| {
        let mut output = PngOutput {
            allocator,
            image: sys::GhosttySysImage::default(),
            _callback: PhantomData,
        };
        // SAFETY: PNG input is readable for this callback only.
        let data = unsafe {
            ffi::string_bytes(sys::GhosttyString {
                ptr: data,
                len: data_len,
            })
        };
        if decoder(data, &mut output).is_err() || output.image.data.is_null() {
            return false;
        }
        // No fallible or user operation occurs after transferring ownership.
        let image = core::mem::take(&mut output.image);
        // SAFETY: C supplies an exclusive, initialized output struct. On true,
        // it takes ownership and frees using this same allocator and data_len.
        unsafe { out.write(image) };
        true
    })
    .unwrap_or(false)
}

unsafe extern "C" fn png_trampoline(
    _userdata: *mut c_void,
    allocator: *const sys::GhosttyAllocator,
    data: *const u8,
    data_len: usize,
    out: *mut sys::GhosttySysImage,
) -> bool {
    // SAFETY: installation excludes every library call, so the slot and owned
    // Sync closure are only borrowed immutably until this callback returns.
    match unsafe { installed(&raw const PNG_DECODER) } {
        // SAFETY: arguments are forwarded unchanged from the C callback.
        Some(decoder) => unsafe { dispatch_png(decoder, allocator, data, data_len, out) },
        None => false,
    }
}

unsafe fn dispatch_random(random: &SecureRandom, buf: *mut u8, len: usize) -> bool {
    catch_callback(|| {
        let bytes = if len == 0 {
            &mut []
        } else {
            // SAFETY: C grants exclusive access to len writable bytes, which
            // may initially be uninitialized. Initialize before exposing a safe
            // u8 slice; no reference can escape the closure.
            unsafe {
                ptr::write_bytes(buf, 0, len);
                core::slice::from_raw_parts_mut(buf, len)
            }
        };
        random(bytes)
    })
    .unwrap_or(false)
}

unsafe extern "C" fn random_trampoline(_userdata: *mut c_void, buf: *mut u8, len: usize) -> bool {
    // SAFETY: installation excludes every library call, so the slot and owned
    // Sync closure are only borrowed immutably until this callback returns.
    match unsafe { installed(&raw const SECURE_RANDOM) } {
        // SAFETY: arguments are forwarded unchanged from the C callback.
        Some(random) => unsafe { dispatch_random(random, buf, len) },
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::{Layout, alloc, dealloc};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct AllocationCounts {
        allocations: AtomicUsize,
        frees: AtomicUsize,
        reject: bool,
    }

    unsafe extern "C" fn test_alloc(
        ctx: *mut c_void,
        len: usize,
        alignment: u8,
        _ret_addr: usize,
    ) -> *mut c_void {
        // SAFETY: each test keeps this allocator context alive for every call.
        let counts = unsafe { &*ctx.cast::<AllocationCounts>() };
        if counts.reject {
            return ptr::null_mut();
        }
        let Ok(layout) = Layout::from_size_align(len, 1usize << alignment) else {
            return ptr::null_mut();
        };
        // SAFETY: the checked layout has nonzero size from PngOutput::allocate.
        let data = unsafe { alloc(layout) };
        if !data.is_null() {
            counts.allocations.fetch_add(1, Ordering::Relaxed);
        }
        data.cast()
    }

    unsafe extern "C" fn test_free(
        ctx: *mut c_void,
        data: *mut c_void,
        len: usize,
        alignment: u8,
        _ret_addr: usize,
    ) {
        // SAFETY: the context is live, and C passes the original allocation's
        // pointer, size and alignment back to this matching free function.
        unsafe {
            let counts = &*ctx.cast::<AllocationCounts>();
            counts.frees.fetch_add(1, Ordering::Relaxed);
            dealloc(
                data.cast(),
                Layout::from_size_align_unchecked(len, 1usize << alignment),
            );
        }
    }

    fn allocator(
        counts: &AllocationCounts,
        vtable: &sys::GhosttyAllocatorVtable,
    ) -> sys::GhosttyAllocator {
        sys::GhosttyAllocator {
            ctx: ptr::from_ref(counts).cast_mut().cast(),
            vtable,
        }
    }

    fn vtable() -> sys::GhosttyAllocatorVtable {
        sys::GhosttyAllocatorVtable {
            alloc: Some(test_alloc),
            free: Some(test_free),
            ..Default::default()
        }
    }

    #[test]
    fn png_transfers_callback_allocated_rgba_without_copying() {
        let counts = AllocationCounts::default();
        let vtable = vtable();
        let allocator = allocator(&counts, &vtable);
        let pointer = Arc::new(AtomicUsize::new(0));
        let recorded = pointer.clone();
        let decoder = move |input: &[u8], output: &mut PngOutput<'_>| {
            assert_eq!(input, b"encoded PNG");
            let rgba = output.allocate(2, 1)?;
            assert_eq!(rgba, &[0; 8]);
            rgba.copy_from_slice(&[255, 0, 0, 255, 0, 255, 0, 128]);
            recorded.store(rgba.as_ptr() as usize, Ordering::Relaxed);
            Ok(())
        };
        let mut out = sys::GhosttySysImage::default();
        // SAFETY: local input, allocator, and output remain live for dispatch;
        // no process-global callback is installed by any of these tests.
        unsafe {
            assert!(dispatch_png(
                &decoder,
                &allocator,
                b"encoded PNG".as_ptr(),
                11,
                &mut out
            ));
            assert_eq!((out.width, out.height, out.data_len), (2, 1, 8));
            assert_eq!(out.data as usize, pointer.load(Ordering::Relaxed));
            assert_eq!(
                core::slice::from_raw_parts(out.data, out.data_len),
                &[255, 0, 0, 255, 0, 255, 0, 128]
            );
            assert_eq!(counts.frees.load(Ordering::Relaxed), 0);
            sys::ghostty_free(&allocator, out.data, out.data_len);
        }
        assert_eq!(counts.allocations.load(Ordering::Relaxed), 1);
        assert_eq!(counts.frees.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn png_failure_and_panic_free_every_allocation() {
        for panics in [false, true] {
            let counts = AllocationCounts::default();
            let vtable = vtable();
            let allocator = allocator(&counts, &vtable);
            let decoder = move |_: &[u8], output: &mut PngOutput<'_>| {
                output.allocate(1, 1)?;
                output.allocate(2, 2)?;
                if panics {
                    panic!("decoder panic after allocation");
                }
                Err(Error::InvalidValue)
            };
            let mut out = sys::GhosttySysImage::default();
            // SAFETY: all arguments remain live for synchronous dispatch.
            assert!(!unsafe { dispatch_png(&decoder, &allocator, ptr::null(), 0, &mut out) });
            assert!(out.data.is_null());
            assert_eq!(counts.allocations.load(Ordering::Relaxed), 2);
            assert_eq!(counts.frees.load(Ordering::Relaxed), 2);
        }
    }

    #[test]
    fn png_rejects_allocator_failure_bad_dimensions_and_missing_output() {
        let counts = AllocationCounts {
            reject: true,
            ..Default::default()
        };
        let vtable = vtable();
        let allocator = allocator(&counts, &vtable);
        let decoder = |_: &[u8], output: &mut PngOutput<'_>| {
            assert_eq!(output.allocate(0, 1).unwrap_err(), Error::InvalidValue);
            assert_eq!(
                output.allocate(u32::MAX, u32::MAX).unwrap_err(),
                Error::InvalidValue
            );
            assert_eq!(output.allocate(1, 1).unwrap_err(), Error::OutOfMemory);
            Ok(())
        };
        let mut out = sys::GhosttySysImage::default();
        // SAFETY: all arguments remain live for synchronous dispatch.
        assert!(!unsafe { dispatch_png(&decoder, &allocator, ptr::null(), 0, &mut out) });
        assert!(out.data.is_null());
        assert_eq!(counts.allocations.load(Ordering::Relaxed), 0);
        assert_eq!(counts.frees.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn log_delivers_borrowed_bytes_and_contains_panics() {
        let messages = Arc::new(Mutex::new(Vec::new()));
        let recorded = messages.clone();
        let handler = move |level, scope: &[u8], message: &[u8]| {
            recorded
                .lock()
                .unwrap()
                .push((level, scope.to_vec(), message.to_vec()));
        };
        // SAFETY: byte slices are valid for these synchronous invocations.
        unsafe {
            dispatch_log(
                &handler,
                sys::GHOSTTY_SYS_LOG_LEVEL_WARNING,
                b"kitty".as_ptr(),
                5,
                b"bad image".as_ptr(),
                9,
            );
            dispatch_log(
                &handler,
                sys::GHOSTTY_SYS_LOG_LEVEL_INFO,
                ptr::null(),
                0,
                ptr::null(),
                0,
            );
            dispatch_log(
                &|_, _, _| panic!("log panic"),
                sys::GHOSTTY_SYS_LOG_LEVEL_ERROR,
                ptr::null(),
                0,
                ptr::null(),
                0,
            );
        }
        assert_eq!(
            *messages.lock().unwrap(),
            vec![
                (LogLevel::Warning, b"kitty".to_vec(), b"bad image".to_vec()),
                (LogLevel::Info, Vec::new(), Vec::new()),
            ]
        );
    }

    #[test]
    fn random_fills_exclusive_buffer_and_reports_failure_or_panic() {
        let mut uninitialized = core::mem::MaybeUninit::<[u8; 16]>::uninit();
        // SAFETY: the callback is granted exclusive writable memory, matching
        // C's potentially uninitialized random-buffer contract.
        unsafe {
            assert!(dispatch_random(
                &|bytes| {
                    assert_eq!(bytes, &[0; 16]);
                    bytes.fill(0xa5);
                    true
                },
                uninitialized.as_mut_ptr().cast(),
                16
            ));
            assert_eq!(uninitialized.assume_init(), [0xa5; 16]);
        }
        let mut bytes = [0; 16];
        // Deliberately deterministic test-only source, not an installed CSPRNG.
        let random = |bytes: &mut [u8]| {
            bytes.fill(0x5a);
            true
        };
        // SAFETY: each dispatch borrows an exclusive live output buffer; the
        // empty invocation requires no pointer. No callback globals change.
        unsafe {
            assert!(dispatch_random(&random, bytes.as_mut_ptr(), bytes.len()));
            assert_eq!(bytes, [0x5a; 16]);
            assert!(dispatch_random(&random, ptr::null_mut(), 0));
            assert!(!dispatch_random(
                &|_| false,
                bytes.as_mut_ptr(),
                bytes.len()
            ));
            assert!(!dispatch_random(
                &|_| panic!("random panic"),
                bytes.as_mut_ptr(),
                bytes.len()
            ));
        }
    }
}
