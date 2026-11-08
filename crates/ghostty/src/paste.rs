//! Pasting into a terminal, paste safety checks, and paste encoding.

use core::ffi::c_void;
use std::io::{self, Write};

use ghostty_sys as sys;

use crate::{
    Error, Result, ffi,
    terminal::{ClipboardLocation, Terminal},
};

ffi::c_enum! {
    /// Why text is being inserted.
    #[derive(Default)]
    pub enum PasteSource {
        /// A clipboard paste, eligible for Kitty paste events.
        #[default]
        Clipboard = sys::GHOSTTY_PASTE_SOURCE_CLIPBOARD,
        /// IME, drag-and-drop or scripted input; always inserts text.
        Text = sys::GHOSTTY_PASTE_SOURCE_TEXT,
    }
}

/// Options for a terminal-state-aware paste.
#[derive(Debug, Clone, Copy)]
pub struct PasteOptions {
    /// Clipboard location reported by Kitty paste events.
    pub location: ClipboardLocation,
    /// Whether this is a clipboard paste or direct text input.
    pub source: PasteSource,
    /// Allow command-injecting text after obtaining the user's confirmation.
    pub allow_unsafe: bool,
}

impl Default for PasteOptions {
    fn default() -> Self {
        Self {
            location: ClipboardLocation::Standard,
            source: PasteSource::Clipboard,
            allow_unsafe: false,
        }
    }
}

/// Conservatively checks for newlines, carriage returns and bracketed-paste terminators.
pub fn is_safe(data: &[u8]) -> bool {
    unsafe { sys::ghostty_paste_is_safe(data.as_ptr().cast(), data.len()) }
}

/// Sanitizes and encodes paste data without modifying the caller's bytes.
///
/// Bracketed encoding preserves newlines; plain encoding converts them to carriage returns.
pub fn encode(data: &[u8], bracketed: bool) -> Result<Vec<u8>> {
    // C sanitizes input in place, including on OUT_OF_SPACE. Retrying the scratch buffer is idempotent.
    let mut scratch = data.to_vec();
    ffi::encode_vec(|buf, cap, written| unsafe {
        sys::ghostty_paste_encode(
            scratch.as_mut_ptr().cast(),
            scratch.len(),
            bracketed,
            buf,
            cap,
            written,
        )
    })
}

struct PasteReader<F> {
    reader: F,
    panic: ffi::PanicSlot,
}

struct MimeWriter(sys::GhosttyWriter);

impl Write for MimeWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let write = self
            .0
            .write
            .ok_or_else(|| io::Error::other("missing paste writer"))?;
        // SAFETY: the library supplies this writer for the duration of the MIME callback.
        if unsafe { write(self.0.userdata, data.as_ptr(), data.len()) } {
            Ok(data.len())
        } else {
            Err(io::Error::other("paste writer refused data"))
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

unsafe extern "C" fn read_mime<F>(
    userdata: *mut c_void,
    mime: sys::GhosttyString,
    writer: sys::GhosttyWriter,
) -> bool
where
    F: FnMut(&[u8], &mut dyn Write) -> io::Result<()>,
{
    // SAFETY: paste_with keeps this stack state alive throughout the synchronous call.
    let state = unsafe { &mut *userdata.cast::<PasteReader<F>>() };
    // SAFETY: the MIME bytes are borrowed for this callback's duration.
    let mime = unsafe { ffi::string_bytes(mime) };
    let PasteReader { reader, panic } = state;
    matches!(
        panic.catch(|| reader(mime, &mut MimeWriter(writer))),
        Some(Ok(()))
    )
}

impl Terminal {
    /// Pastes plain text through the pty callback, honoring bracketed mode and Kitty paste events.
    ///
    /// Returns whether anything was written. Unsafe unbracketed text returns
    /// [`Error::Rejected`] unless `options.allow_unsafe` is true.
    pub fn paste(&mut self, data: &[u8], options: PasteOptions) -> Result<bool> {
        let mimes = [ffi::ghostty_string(b"text/plain")];
        self.paste_mimes(&mimes, options, |_, writer| writer.write_all(data))
    }

    /// Pastes MIME-typed content, producing only the requested representation on demand.
    ///
    /// MIME types are in preference order. A text paste reads the first text MIME type
    /// at most once; a Kitty paste event lists all MIME types without calling `reader`.
    /// The reader may stream data into its writer, which must not escape the callback.
    /// Returns whether text or a paste event was written through the pty callback.
    pub fn paste_with<F>(
        &mut self,
        mimes: &[&[u8]],
        options: PasteOptions,
        reader: F,
    ) -> Result<bool>
    where
        F: FnMut(&[u8], &mut dyn Write) -> io::Result<()>,
    {
        let mimes: Vec<_> = mimes.iter().map(|mime| ffi::ghostty_string(mime)).collect();
        self.paste_mimes(&mimes, options, reader)
    }

    fn paste_mimes<F>(
        &mut self,
        mimes: &[sys::GhosttyString],
        options: PasteOptions,
        reader: F,
    ) -> Result<bool>
    where
        F: FnMut(&[u8], &mut dyn Write) -> io::Result<()>,
    {
        let mut state = PasteReader {
            reader,
            panic: ffi::PanicSlot::default(),
        };
        // SAFETY: GhosttyPaste starts with its size_t size field.
        let mut raw: sys::GhosttyPaste = unsafe { ffi::sized() };
        raw.location = options.location.to_raw();
        raw.source = options.source.to_raw();
        raw.mimes = mimes.as_ptr();
        raw.mimes_len = mimes.len();
        raw.reader = sys::GhosttyMimeReader {
            read: Some(read_mime::<F>),
            userdata: (&raw mut state).cast(),
        };
        raw.allow_unsafe = options.allow_unsafe;
        let mut pasted = false;
        // SAFETY: request, MIME strings and callback state all live through this synchronous call.
        let code = unsafe { sys::ghostty_terminal_paste(self.as_raw(), &raw, &mut pasted) };
        self.resume_callback_panic();
        state.panic.resume();
        Error::check(code)?;
        Ok(pasted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn capture(terminal: &mut Terminal) -> Arc<Mutex<Vec<u8>>> {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let output = bytes.clone();
        terminal.on_write_pty(move |data| output.lock().unwrap().extend_from_slice(data));
        bytes
    }

    #[test]
    fn paste_bracketed_and_rejected() -> Result<()> {
        let mut terminal = Terminal::new(80, 24)?;
        let output = capture(&mut terminal);
        let options = PasteOptions::default();
        assert_eq!(terminal.paste(b"echo hi\n", options), Err(Error::Rejected));
        assert!(output.lock().unwrap().is_empty());
        assert!(terminal.paste(
            b"echo hi\n",
            PasteOptions {
                allow_unsafe: true,
                ..options
            }
        )?);
        assert_eq!(&*output.lock().unwrap(), b"echo hi\r");
        output.lock().unwrap().clear();
        terminal.vt_write(b"\x1b[?2004h");
        assert!(terminal.paste(b"line one\nline two", options)?);
        assert_eq!(
            &*output.lock().unwrap(),
            b"\x1b[200~line one\nline two\x1b[201~"
        );
        assert!(!terminal.paste(b"", options)?);
        Ok(())
    }

    #[test]
    fn paste_safety_and_encoding_preserve_input() -> Result<()> {
        assert!(is_safe(b"hello world"));
        assert!(!is_safe(b"hello\nworld"));
        assert!(!is_safe(b"hello\rworld"));
        assert!(!is_safe(b"\x1b[201~"));
        let data = b"hello\x1b\0\nworld";
        assert_eq!(encode(data, true)?, b"\x1b[200~hello  \nworld\x1b[201~");
        assert_eq!(encode(data, false)?, b"hello  \rworld");
        assert_eq!(data, b"hello\x1b\0\nworld");
        let long = vec![b'x'; 200];
        assert_eq!(encode(&long, false)?, long);
        Ok(())
    }

    #[test]
    fn paste_kitty_event_and_direct_text() -> Result<()> {
        let mut terminal = Terminal::new(80, 24)?;
        let output = capture(&mut terminal);
        terminal.on_clipboard_read(|read| {
            read.reply(
                crate::terminal::ClipboardReadResult::Unsupported,
                &[],
                &[],
                false,
            );
        });
        terminal.vt_write(b"\x1b[?5522h");
        assert!(terminal.paste_with(
            &[b"text/plain", b"image/png"],
            PasteOptions::default(),
            |_, _| {
                panic!("paste events must not read representations");
            }
        )?);
        let event = output.lock().unwrap().clone();
        assert!(event.starts_with(b"\x1b]5522;type=read:status=OK:pw="));
        assert!(
            event
                .windows(b"dGV4dC9wbGFpbiBpbWFnZS9wbmcK".len())
                .any(|bytes| bytes == b"dGV4dC9wbGFpbiBpbWFnZS9wbmcK")
        );
        output.lock().unwrap().clear();
        assert!(terminal.paste(
            b"committed",
            PasteOptions {
                source: PasteSource::Text,
                ..PasteOptions::default()
            }
        )?);
        assert_eq!(&*output.lock().unwrap(), b"committed");
        Ok(())
    }

    #[test]
    fn paste_resumes_pty_callback_panic() -> Result<()> {
        let mut terminal = Terminal::new(80, 24)?;
        terminal.on_write_pty(|_| panic!("pty callback panic"));
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = terminal.paste(b"hello", PasteOptions::default());
        }));
        assert!(panic.is_err());
        Ok(())
    }

    #[test]
    fn paste_reader_errors_and_panics() -> Result<()> {
        let mut terminal = Terminal::new(80, 24)?;
        let output = capture(&mut terminal);
        assert_eq!(
            terminal.paste_with(&[b"text/plain"], PasteOptions::default(), |_, _| {
                Err(io::Error::other("clipboard unavailable"))
            }),
            Err(Error::Io)
        );
        assert!(output.lock().unwrap().is_empty());
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = terminal.paste_with(&[b"text/plain"], PasteOptions::default(), |_, _| {
                panic!("reader panic");
            });
        }));
        assert!(panic.is_err());
        assert!(output.lock().unwrap().is_empty());
        Ok(())
    }
}
