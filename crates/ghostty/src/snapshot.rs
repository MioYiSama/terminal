//! Encoding and restoring complete terminal state.

use crate::{
    Error, Result, ffi,
    terminal::{ActiveScreen, Terminal},
};
use core::{
    marker::PhantomData,
    ptr::{self, NonNull},
};
use ghostty_sys as sys;
use std::io::{self, Read, Write};

/// Streams a complete snapshot synchronously without flushing the writer.
/// The writer must not reenter APIs on the encoded terminal.
pub fn encode<W: Write + ?Sized>(terminal: &Terminal, writer: &mut W) -> io::Result<()> {
    ffi::with_writer(writer, |writer| unsafe {
        sys::ghostty_snapshot_encode(terminal.as_raw(), writer)
    })
}

/// Encodes a complete snapshot into an owned byte buffer.
pub fn encode_to_vec(terminal: &Terminal) -> Result<Vec<u8>> {
    let mut bytes = ptr::null_mut();
    let mut len = 0;
    unsafe {
        Error::check(sys::ghostty_snapshot_encode_alloc(
            terminal.as_raw(),
            ptr::null(),
            &mut bytes,
            &mut len,
        ))?;
    }
    // SAFETY: the allocation was returned by snapshot_encode_alloc with the default allocator.
    Ok(unsafe { ffi::take_alloc(bytes, len) })
}

ffi::c_enum! {
    /// Record restored by the most recent successful incremental step.
    pub enum ProgressKind {
        /// History page and its graphics placements.
        Page = sys::GHOSTTY_SNAPSHOT_DECODER_PROGRESS_KIND_PAGE,
        /// Graphics image or animation frame pixels.
        Image = sys::GHOSTTY_SNAPSHOT_DECODER_PROGRESS_KIND_IMAGE,
    }
}

struct ReaderState<'a> {
    reader: Box<dyn Read + 'a>,
    error: Option<io::Error>,
    panic: ffi::PanicSlot,
}

unsafe extern "C" fn read_trampoline(
    userdata: *mut core::ffi::c_void,
    buffer: *mut u8,
    capacity: usize,
    out_read: *mut usize,
) -> bool {
    // SAFETY: the decoder retains this stable boxed context and supplies a writable buffer.
    let state = unsafe { &mut *userdata.cast::<ReaderState<'_>>() };
    let buffer = unsafe { core::slice::from_raw_parts_mut(buffer, capacity) };
    let ReaderState {
        reader,
        error,
        panic,
    } = state;
    match panic.catch(|| {
        loop {
            match reader.read(buffer) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => break result,
            }
        }
    }) {
        Some(Ok(len)) if len <= capacity => {
            unsafe {
                *out_read = len;
            }
            true
        }
        Some(Ok(_)) => {
            error.get_or_insert_with(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "reader returned an invalid byte count",
                )
            });
            false
        }
        Some(Err(err)) => {
            error.get_or_insert(err);
            false
        }
        None => false,
    }
}

/// A decoder borrowing its source bytes or owning a synchronous reader.
///
/// After [`ready`](Self::ready), the decoder owns the restored terminal until
/// [`into_terminal`](Self::into_terminal). This ensures the terminal survives
/// every incremental step and is exclusively borrowed during each mutation.
/// Reader callbacks never unwind across C. No thread-safety traits are added.
pub struct SnapshotDecoder<'a> {
    raw: NonNull<sys::GhosttySnapshotDecoderImpl>,
    reader: Option<Box<ReaderState<'a>>>,
    terminal: Option<Terminal>,
    started: bool,
    failed: bool,
    source: PhantomData<&'a [u8]>,
}

macro_rules! decoder_option {
    ($name:ident, $ty:ty, $key:ident, $doc:literal) => {
        #[doc = $doc]
        pub fn $name(&mut self, value: Option<$ty>) -> Result<()> {
            if self.failed {
                return Err(Error::InvalidValue);
            }
            let value = value
                .as_ref()
                .map_or(ptr::null(), |value| (value as *const $ty).cast());
            unsafe {
                Error::check(sys::ghostty_snapshot_decoder_set(
                    self.raw.as_ptr(),
                    sys::$key,
                    value,
                ))
            }
        }
    };
}
macro_rules! decoder_data {
    ($name:ident, $ty:ty, $key:ident, $doc:literal) => {
        #[doc = $doc]
        pub fn $name(&self) -> Result<Option<$ty>> {
            self.get_optional(sys::$key)
        }
    };
}

impl<'a> SnapshotDecoder<'a> {
    /// Creates a decoder borrowing immutable snapshot bytes, including trailing data.
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
        let mut raw = ptr::null_mut();
        unsafe {
            Error::check(sys::ghostty_snapshot_decoder_new_buf(
                ptr::null(),
                &mut raw,
                bytes.as_ptr(),
                bytes.len(),
            ))?;
        }
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
            reader: None,
            terminal: None,
            started: false,
            failed: false,
            source: PhantomData,
        })
    }
    /// Creates a decoder owning a reader; pass `&mut reader` to borrow it instead.
    /// A successful zero-byte read means permanent EOF. Reads occur only during decoding.
    pub fn from_reader<R: Read + 'a>(reader: R) -> Result<Self> {
        let mut reader = Box::new(ReaderState {
            reader: Box::new(reader),
            error: None,
            panic: ffi::PanicSlot::default(),
        });
        let input = sys::GhosttyReader {
            read: Some(read_trampoline),
            userdata: (&raw mut *reader).cast(),
        };
        let mut raw = ptr::null_mut();
        unsafe {
            Error::check(sys::ghostty_snapshot_decoder_new(
                ptr::null(),
                &mut raw,
                input,
            ))?;
        }
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
            reader: Some(reader),
            terminal: None,
            started: false,
            failed: false,
            source: PhantomData,
        })
    }
    decoder_option!(
        set_max_continuation_bytes,
        usize,
        GHOSTTY_SNAPSHOT_DECODER_OPT_MAX_CONTINUATION_BYTES,
        "Sets the accepted continuation limit, or restores the default with None. Only before decoding."
    );
    decoder_option!(
        set_retain_continuation,
        bool,
        GHOSTTY_SNAPSHOT_DECODER_OPT_RETAIN_CONTINUATION,
        "Sets continuation retention, or restores false with None. Only before decoding."
    );
    decoder_option!(
        set_compress_history,
        bool,
        GHOSTTY_SNAPSHOT_DECODER_OPT_COMPRESS_HISTORY,
        "Sets history compression, or restores false with None. Only before decoding."
    );
    decoder_option!(
        set_kitty_image_storage_limit,
        u64,
        GHOSTTY_SNAPSHOT_DECODER_OPT_KITTY_IMAGE_STORAGE_LIMIT,
        "Sets per-screen graphics storage limit, or restores its default with None. Only before decoding."
    );

    fn finish_call(&mut self, code: sys::GhosttyResult) -> io::Result<()> {
        if code != sys::GHOSTTY_SUCCESS && code != sys::GHOSTTY_NO_VALUE {
            self.failed = true;
        }
        if let Some(reader) = &mut self.reader {
            reader.panic.resume();
            if let Some(error) = reader.error.take() {
                return Err(error);
            }
        }
        Error::check(code).map_err(Into::into)
    }
    fn start(&mut self) -> io::Result<()> {
        if self.started || self.failed {
            return Err(Error::InvalidValue.into());
        }
        self.started = true;
        Ok(())
    }
    /// Restores the renderable prefix and retains ownership of its terminal.
    /// The returned mutable borrow must end before calling next.
    pub fn ready(&mut self) -> io::Result<&mut Terminal> {
        self.start()?;
        let mut raw = ptr::null_mut();
        let code = unsafe { sys::ghostty_snapshot_decoder_ready(self.raw.as_ptr(), &mut raw) };
        self.finish_call(code)?;
        let raw = NonNull::new(raw).ok_or(Error::InvalidValue)?;
        // SAFETY: READY transfers ownership, retained until after this decoder is freed.
        self.terminal = Some(unsafe { Terminal::from_raw_owned(raw) });
        Ok(self.terminal.as_mut().unwrap())
    }
    /// Restores one history/image record. False means FINISH has validated.
    /// On failure the partial terminal remains usable, but the decoder cannot decode further.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<bool> {
        if self.failed || self.terminal.is_none() {
            return Err(Error::InvalidValue.into());
        }
        let code = unsafe { sys::ghostty_snapshot_decoder_next(self.raw.as_ptr()) };
        if code == sys::GHOSTTY_NO_VALUE {
            return Ok(false);
        }
        self.finish_call(code)?;
        Ok(true)
    }
    /// Decodes the complete snapshot in one call and transfers the terminal to the caller.
    pub fn decode(&mut self) -> io::Result<Terminal> {
        self.start()?;
        let mut raw = ptr::null_mut();
        let code = unsafe { sys::ghostty_snapshot_decoder_decode(self.raw.as_ptr(), &mut raw) };
        self.finish_call(code)?;
        let raw = NonNull::new(raw).ok_or(Error::InvalidValue)?;
        // SAFETY: complete decoding transfers ownership with no outstanding decoder borrow.
        Ok(unsafe { Terminal::from_raw_owned(raw) })
    }
    /// Returns the incrementally restored terminal after ready, including after a later failure.
    pub fn terminal(&self) -> Option<&Terminal> {
        self.terminal.as_ref()
    }
    /// Borrows the incrementally restored terminal for live input or resizing between steps.
    pub fn terminal_mut(&mut self) -> Option<&mut Terminal> {
        self.terminal.as_mut()
    }
    /// Frees the decoder first, then transfers its partial or completed terminal, if ready was called.
    pub fn into_terminal(mut self) -> Option<Terminal> {
        let terminal = self.terminal.take();
        drop(self);
        terminal
    }
    fn get_optional<T: Default>(&self, data: sys::GhosttySnapshotDecoderData) -> Result<Option<T>> {
        if self.failed {
            return Err(Error::InvalidValue);
        }
        let mut value = T::default();
        let code = unsafe {
            sys::ghostty_snapshot_decoder_get(self.raw.as_ptr(), data, (&raw mut value).cast())
        };
        if code == sys::GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        Error::check(code)?;
        Ok(Some(value))
    }
    decoder_data!(
        max_continuation_bytes,
        usize,
        GHOSTTY_SNAPSHOT_DECODER_DATA_MAX_CONTINUATION_BYTES,
        "Returns the accepted continuation limit."
    );
    decoder_data!(
        source_offset,
        usize,
        GHOSTTY_SNAPSHOT_DECODER_DATA_SOURCE_OFFSET,
        "Returns bytes consumed; trailing bytes after FINISH remain unread."
    );
    decoder_data!(
        history_rows_primary,
        u64,
        GHOSTTY_SNAPSHOT_DECODER_DATA_HISTORY_ROWS_PRIMARY,
        "Returns the advisory primary history extent after READY."
    );
    decoder_data!(
        history_rows_alternate,
        u64,
        GHOSTTY_SNAPSHOT_DECODER_DATA_HISTORY_ROWS_ALTERNATE,
        "Returns the advisory alternate history extent, when present."
    );
    decoder_data!(
        progress_rows,
        usize,
        GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_ROWS,
        "Returns rows prepended by the latest history-page step."
    );
    decoder_data!(
        progress_remaining,
        u32,
        GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_REMAINING,
        "Returns remaining pages in the latest step's history sequence."
    );
    decoder_data!(
        retain_continuation,
        bool,
        GHOSTTY_SNAPSHOT_DECODER_DATA_RETAIN_CONTINUATION,
        "Returns whether continuation tracking is retained."
    );
    decoder_data!(
        compress_history,
        bool,
        GHOSTTY_SNAPSHOT_DECODER_DATA_COMPRESS_HISTORY,
        "Returns whether history is restored compressed."
    );
    decoder_data!(
        kitty_image_storage_limit,
        u64,
        GHOSTTY_SNAPSHOT_DECODER_DATA_KITTY_IMAGE_STORAGE_LIMIT,
        "Returns the per-screen graphics storage limit."
    );
    decoder_data!(
        progress_image_id,
        u32,
        GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_IMAGE_ID,
        "Returns the latest restored graphics image ID."
    );
    decoder_data!(
        progress_image_frame,
        u32,
        GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_IMAGE_FRAME,
        "Returns the latest restored graphics frame number."
    );
    decoder_data!(
        progress_image_bytes,
        usize,
        GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_IMAGE_BYTES,
        "Returns pixel bytes attached by the latest graphics step."
    );
    decoder_data!(
        kitty_images_primary,
        u32,
        GHOSTTY_SNAPSHOT_DECODER_DATA_KITTY_IMAGES_PRIMARY,
        "Returns the primary screen's graphics image count at READY."
    );
    decoder_data!(
        kitty_images_alternate,
        u32,
        GHOSTTY_SNAPSHOT_DECODER_DATA_KITTY_IMAGES_ALTERNATE,
        "Returns the alternate screen's graphics image count at READY, when present."
    );
    decoder_data!(
        kitty_pending_bytes_primary,
        u64,
        GHOSTTY_SNAPSHOT_DECODER_DATA_KITTY_PENDING_BYTES_PRIMARY,
        "Returns primary graphics pixel bytes pending at READY."
    );
    decoder_data!(
        kitty_pending_bytes_alternate,
        u64,
        GHOSTTY_SNAPSHOT_DECODER_DATA_KITTY_PENDING_BYTES_ALTERNATE,
        "Returns alternate graphics pixel bytes pending at READY, when present."
    );
    /// Returns which screen the latest incremental step restored.
    pub fn progress_screen(&self) -> Result<Option<ActiveScreen>> {
        Ok(self
            .get_optional(sys::GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_SCREEN)?
            .map(ActiveScreen::from_lib))
    }
    /// Returns which kind of record the latest incremental step restored.
    pub fn progress_kind(&self) -> Result<Option<ProgressKind>> {
        Ok(self
            .get_optional(sys::GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_KIND)?
            .map(ProgressKind::from_lib))
    }
}
impl Drop for SnapshotDecoder<'_> {
    fn drop(&mut self) {
        // SAFETY: the decoder is freed before its reader context and retained terminal fields.
        unsafe { sys::ghostty_snapshot_decoder_free(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::Formatter;
    #[test]
    fn snapshot_round_trip_and_incremental() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let mut original = Terminal::new(215, 2)?;
        original.set_scrollback_max_bytes(Some(usize::MAX))?;
        original.set_continuation_max_bytes(Some(1024))?;
        for index in 0..1000 {
            original.vt_write(format!("row {index}\r\n").as_bytes());
        }
        original.vt_write(b"Hello, \x1b[1;32mworld\x1b[0m!\x1b[31");
        let bytes = encode_to_vec(&original)?;
        let expected = Formatter::new(&original)?.format_to_vec()?;
        let mut decoder = SnapshotDecoder::from_bytes(&bytes)?;
        let restored = decoder.decode()?;
        assert_eq!(Formatter::new(&restored)?.format_to_vec()?, expected);
        assert_eq!(decoder.source_offset()?, Some(bytes.len()));
        struct ShortReader<R>(R);
        impl<R: Read> Read for ShortReader<R> {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let len = buffer.len().min(64);
                self.0.read(&mut buffer[..len])
            }
        }
        let mut reader_decoder =
            SnapshotDecoder::from_reader(ShortReader(io::Cursor::new(&bytes)))?;
        reader_decoder.set_compress_history(Some(true))?;
        reader_decoder.ready()?;
        {
            // READY restores the active area, but older history arrives only through next.
            let terminal = reader_decoder.terminal().unwrap();
            let selection = crate::selection::Selection::new(
                terminal.grid_ref(crate::point::Point::active(0, 0))?,
                terminal.grid_ref(crate::point::Point::active(214, 1))?,
                false,
            );
            let mut formatter = Formatter::new(terminal)?;
            formatter.set_selection(Some(&selection))?;
            assert_eq!(formatter.format_to_vec()?, b"row 999\nHello, world!");
        }
        let mut pages = 0;
        while reader_decoder.next()? {
            assert_eq!(reader_decoder.progress_kind()?, Some(ProgressKind::Page));
            assert_eq!(
                reader_decoder.progress_screen()?,
                Some(ActiveScreen::Primary)
            );
            pages += 1;
        }
        assert!(pages > 0);
        assert!(!reader_decoder.next()?);
        let mut incremental = reader_decoder.into_terminal().unwrap();
        assert_eq!(Formatter::new(&incremental)?.format_to_vec()?, expected);
        original.vt_write(b"m red");
        incremental.vt_write(b"m red");
        assert_eq!(
            Formatter::new(&incremental)?.format_to_vec()?,
            Formatter::new(&original)?.format_to_vec()?
        );
        Ok(())
    }
    #[test]
    fn writer_reader_errors_panics_and_trailing_bytes()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let mut terminal = Terminal::new(10, 2)?;
        terminal.vt_write(b"hello");
        let mut bytes = Vec::new();
        encode(&terminal, &mut bytes)?;
        let snapshot_len = bytes.len();
        bytes.extend_from_slice(b"trailing");
        let mut decoder = SnapshotDecoder::from_bytes(&bytes)?;
        decoder.decode()?;
        assert_eq!(decoder.source_offset()?, Some(snapshot_len));
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "reader failure",
                ))
            }
        }
        let mut decoder = SnapshotDecoder::from_reader(Failing)?;
        assert_eq!(
            decoder.decode().err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(decoder.source_offset().is_err());
        struct Panicking;
        impl Read for Panicking {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("reader panic")
            }
        }
        let mut decoder = SnapshotDecoder::from_reader(Panicking)?;
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decoder.decode())).is_err()
        );
        Ok(())
    }
}
