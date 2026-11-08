//! Internal helpers shared by the safe wrappers.

use core::ffi::c_void;
use std::any::Any;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use ghostty_sys as sys;

use crate::{Error, Result};

/// Declares a Rust enum mirroring a C enum from `ghostty-sys`.
///
/// Generates `to_raw` (Rust → C), `from_raw` (C → Rust, `None` for unknown
/// values) and `from_lib` (C → Rust for values returned by the library).
macro_rules! c_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident = $value:path
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name {
            $(
                $(#[$vmeta])*
                $variant,
            )*
        }

        #[allow(dead_code)]
        impl $name {
            pub(crate) const fn to_raw(self) -> ::core::ffi::c_int {
                match self {
                    $(Self::$variant => $value,)*
                }
            }

            pub(crate) const fn from_raw(raw: ::core::ffi::c_int) -> Option<Self> {
                match raw {
                    $($value => Some(Self::$variant),)*
                    _ => None,
                }
            }

            /// Converts a value produced by the library. The library and the
            /// bindings are built from the same sources, so an unknown value
            /// is a broken invariant.
            pub(crate) fn from_lib(raw: ::core::ffi::c_int) -> Self {
                Self::from_raw(raw).unwrap_or_else(|| {
                    panic!(concat!("libghostty-vt returned unknown ", stringify!($name), " value {}"), raw)
                })
            }
        }
    };
}
pub(crate) use c_enum;

/// Returns a zeroed sized struct with its `size` field set, like
/// `GHOSTTY_INIT_SIZED`.
///
/// # Safety
///
/// `T` must be a libghostty-vt struct whose first field is `size_t size`.
pub(crate) unsafe fn sized<T: Default>() -> T {
    let mut value = T::default();
    unsafe { sys::ghostty_sized_init((&raw mut value).cast(), size_of::<T>()) };
    value
}

/// Borrows `bytes` as a `GhosttyString`.
pub(crate) fn ghostty_string(bytes: &[u8]) -> sys::GhosttyString {
    sys::GhosttyString {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

/// Views a `GhosttyString` as a byte slice.
///
/// # Safety
///
/// `string` must point to `len` readable bytes that stay valid and unchanged
/// for `'a` (or be null/empty).
pub(crate) unsafe fn string_bytes<'a>(string: sys::GhosttyString) -> &'a [u8] {
    if string.ptr.is_null() || string.len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(string.ptr, string.len) }
    }
}

/// Calls a libghostty-vt function following the `(buf, buf_len,
/// out_written)` convention, growing the buffer on `GHOSTTY_OUT_OF_SPACE`.
pub(crate) fn encode_vec<T>(
    mut f: impl FnMut(*mut T, usize, &mut usize) -> sys::GhosttyResult,
) -> Result<Vec<T>> {
    let mut buf = Vec::<T>::with_capacity(64);
    loop {
        let mut written = 0;
        let code = f(buf.as_mut_ptr(), buf.capacity(), &mut written);
        if code == sys::GHOSTTY_OUT_OF_SPACE && written > buf.capacity() {
            buf.reserve_exact(written);
            continue;
        }
        Error::check(code)?;
        assert!(written <= buf.capacity());
        // SAFETY: the library initialized `written` elements.
        unsafe { buf.set_len(written) };
        return Ok(buf);
    }
}

/// Copies a buffer allocated by libghostty-vt with the default allocator
/// into a `Vec` and frees it.
///
/// # Safety
///
/// `ptr`/`len` must come from a `*_alloc` function called with a NULL
/// allocator, and must not be used afterwards.
pub(crate) unsafe fn take_alloc(ptr: *mut u8, len: usize) -> Vec<u8> {
    if ptr.is_null() {
        return Vec::new();
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) }.to_vec();
    unsafe { sys::ghostty_free(core::ptr::null(), ptr, len) };
    bytes
}

/// A panic caught in a callback, to be re-raised after the C call returns.
///
/// Callbacks invoked by libghostty-vt must not unwind into C. Wrap their
/// bodies in [`PanicSlot::catch`] and call [`PanicSlot::resume`] once the
/// C function that invoked them has returned.
#[derive(Default)]
pub(crate) struct PanicSlot(Option<Box<dyn Any + Send>>);

impl PanicSlot {
    /// Runs `f`; on panic, stores the payload (keeping the first one) and
    /// returns `None`.
    pub(crate) fn catch<R>(&mut self, f: impl FnOnce() -> R) -> Option<R> {
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(value) => Some(value),
            Err(payload) => {
                self.0.get_or_insert(payload);
                None
            }
        }
    }

    /// Re-raises a stored panic, if any.
    pub(crate) fn resume(&mut self) {
        if let Some(payload) = self.0.take() {
            resume_unwind(payload);
        }
    }
}

struct WriterState<'a, W: ?Sized> {
    writer: &'a mut W,
    error: Option<io::Error>,
    panic: PanicSlot,
}

unsafe extern "C" fn write_trampoline<W: io::Write + ?Sized>(
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) -> bool {
    let state = unsafe { &mut *userdata.cast::<WriterState<'_, W>>() };
    let bytes = if len == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(data, len) }
    };
    let WriterState {
        writer,
        error,
        panic,
    } = state;
    match panic.catch(|| writer.write_all(bytes)) {
        Some(Ok(())) => true,
        Some(Err(err)) => {
            error.get_or_insert(err);
            false
        }
        None => false,
    }
}

/// Passes `writer` to a libghostty-vt function as a `GhosttyWriter`.
///
/// I/O errors and panics from `writer` take precedence over the library's
/// result code.
pub(crate) fn with_writer<W: io::Write + ?Sized>(
    writer: &mut W,
    f: impl FnOnce(sys::GhosttyWriter) -> sys::GhosttyResult,
) -> io::Result<()> {
    let mut state = WriterState {
        writer,
        error: None,
        panic: PanicSlot::default(),
    };
    let raw = sys::GhosttyWriter {
        write: Some(write_trampoline::<W>),
        userdata: (&raw mut state).cast(),
    };
    let code = f(raw);
    state.panic.resume();
    if let Some(err) = state.error {
        return Err(err);
    }
    Error::check(code)?;
    Ok(())
}
