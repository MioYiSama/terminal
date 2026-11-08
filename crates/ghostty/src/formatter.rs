//! Formatting terminal content as plain text, VT sequences, or HTML.

use crate::{Error, Result, ffi, selection::Selection, terminal::Terminal};
use core::{
    ffi::c_void,
    ptr::{self, NonNull},
};
use ghostty_sys as sys;
use std::io;

ffi::c_enum! {
    /// Output representation of terminal content.
    pub enum FormatterFormat {
        /// Text without escape sequences.
        Plain = sys::GHOSTTY_FORMATTER_FORMAT_PLAIN,
        /// VT sequences preserving styling.
        Vt = sys::GHOSTTY_FORMATTER_FORMAT_VT,
        /// HTML with inline styles.
        Html = sys::GHOSTTY_FORMATTER_FORMAT_HTML,
    }
}

bitflags::bitflags! {
    /// Additional terminal state restored by VT output.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Extras: u32 {
        /// Color palette.
        const PALETTE = sys::GHOSTTY_FORMATTER_EXTRA_PALETTE;
        /// Non-default terminal modes.
        const MODES = sys::GHOSTTY_FORMATTER_EXTRA_MODES;
        /// Scrolling region.
        const SCROLLING_REGION = sys::GHOSTTY_FORMATTER_EXTRA_SCROLLING_REGION;
        /// Tab stops.
        const TABSTOPS = sys::GHOSTTY_FORMATTER_EXTRA_TABSTOPS;
        /// Working directory.
        const PWD = sys::GHOSTTY_FORMATTER_EXTRA_PWD;
        /// Keyboard modes.
        const KEYBOARD = sys::GHOSTTY_FORMATTER_EXTRA_KEYBOARD;
        /// Cursor position.
        const CURSOR = sys::GHOSTTY_FORMATTER_EXTRA_CURSOR;
        /// Cursor writing style.
        const STYLE = sys::GHOSTTY_FORMATTER_EXTRA_STYLE;
        /// Cursor hyperlink.
        const HYPERLINK = sys::GHOSTTY_FORMATTER_EXTRA_HYPERLINK;
        /// Character protection.
        const PROTECTION = sys::GHOSTTY_FORMATTER_EXTRA_PROTECTION;
        /// Kitty keyboard protocol flags.
        const KITTY_KEYBOARD = sys::GHOSTTY_FORMATTER_EXTRA_KITTY_KEYBOARD;
        /// Character sets.
        const CHARSETS = sys::GHOSTTY_FORMATTER_EXTRA_CHARSETS;
    }
}

impl Default for Extras {
    fn default() -> Self {
        Self::from_bits_retain(sys::GHOSTTY_FORMATTER_EXTRAS_DEFAULT)
    }
}

/// A read-only view of a terminal, preventing mutation while formatting.
pub struct Formatter<'t> {
    raw: NonNull<sys::GhosttyFormatterImpl>,
    terminal: &'t Terminal,
}

impl<'t> Formatter<'t> {
    /// Creates a plain-text formatter with trailing whitespace trimmed.
    pub fn new(terminal: &'t Terminal) -> Result<Self> {
        let mut raw = ptr::null_mut();
        Error::check(unsafe {
            sys::ghostty_formatter_terminal_new(ptr::null(), &mut raw, terminal.as_raw())
        })?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
            terminal,
        })
    }

    fn set<T>(&mut self, option: sys::GhosttyFormatterOption, value: &T) -> Result<()> {
        unsafe {
            Error::check(sys::ghostty_formatter_set(
                self.raw.as_ptr(),
                option,
                (value as *const T).cast(),
            ))
        }
    }
    fn get<T: Default>(&self, option: sys::GhosttyFormatterOption) -> Result<T> {
        let mut value = T::default();
        unsafe {
            Error::check(sys::ghostty_formatter_get(
                self.raw.as_ptr(),
                option,
                (&raw mut value).cast(),
            ))?;
        }
        Ok(value)
    }
    /// Sets the output representation.
    pub fn set_format(&mut self, format: FormatterFormat) -> Result<()> {
        self.set(sys::GHOSTTY_FORMATTER_OPT_EMIT, &format.to_raw())
    }
    /// Returns the output representation.
    pub fn format_kind(&self) -> Result<FormatterFormat> {
        self.get(sys::GHOSTTY_FORMATTER_OPT_EMIT)
            .map(FormatterFormat::from_lib)
    }
    /// Joins soft-wrapped lines when enabled.
    pub fn set_unwrap(&mut self, unwrap: bool) -> Result<()> {
        self.set(sys::GHOSTTY_FORMATTER_OPT_UNWRAP, &unwrap)
    }
    /// Returns whether soft-wrapped lines are joined.
    pub fn unwrap(&self) -> Result<bool> {
        self.get(sys::GHOSTTY_FORMATTER_OPT_UNWRAP)
    }
    /// Removes trailing whitespace when enabled.
    pub fn set_trim(&mut self, trim: bool) -> Result<()> {
        self.set(sys::GHOSTTY_FORMATTER_OPT_TRIM, &trim)
    }
    /// Returns whether trailing whitespace is removed.
    pub fn trim(&self) -> Result<bool> {
        self.get(sys::GHOSTTY_FORMATTER_OPT_TRIM)
    }
    /// Sets the additional state included in VT output.
    pub fn set_extras(&mut self, extras: Extras) -> Result<()> {
        self.set(sys::GHOSTTY_FORMATTER_OPT_EXTRAS, &extras.bits())
    }
    /// Returns the additional state included in VT output.
    pub fn extras(&self) -> Result<Extras> {
        self.get(sys::GHOSTTY_FORMATTER_OPT_EXTRAS)
            .map(Extras::from_bits_retain)
    }
    /// Restricts output to a selection from this terminal, or clears the restriction.
    pub fn set_selection(&mut self, selection: Option<&Selection<'_>>) -> Result<()> {
        if let Some(selection) = selection {
            if !ptr::eq(self.terminal, selection.terminal()) {
                return Err(Error::InvalidValue);
            }
            self.set(sys::GHOSTTY_FORMATTER_OPT_SELECTION, &selection.to_raw())
        } else {
            unsafe {
                Error::check(sys::ghostty_formatter_set(
                    self.raw.as_ptr(),
                    sys::GHOSTTY_FORMATTER_OPT_SELECTION,
                    ptr::null::<c_void>(),
                ))
            }
        }
    }
    /// Returns the current selection restriction.
    pub fn selection(&self) -> Result<Option<Selection<'t>>> {
        match self.get::<sys::GhosttySelection>(sys::GHOSTTY_FORMATTER_OPT_SELECTION) {
            // SAFETY: the formatter borrows the same terminal, preventing stale references.
            Ok(raw) => Ok(Some(unsafe { Selection::from_raw(self.terminal, &raw) })),
            Err(Error::NoValue) => Ok(None),
            Err(error) => Err(error),
        }
    }
    /// Streams output synchronously. The writer must not reenter this formatter or terminal.
    pub fn format<W: io::Write + ?Sized>(&self, writer: &mut W) -> io::Result<()> {
        ffi::with_writer(writer, |writer| unsafe {
            sys::ghostty_formatter_format(self.raw.as_ptr(), writer)
        })
    }
    /// Formats content into an owned byte buffer.
    pub fn format_to_vec(&self) -> Result<Vec<u8>> {
        ffi::encode_vec(|buffer, capacity, written| unsafe {
            sys::ghostty_formatter_format_buf(self.raw.as_ptr(), buffer, capacity, written)
        })
    }
}

impl Drop for Formatter<'_> {
    fn drop(&mut self) {
        unsafe { sys::ghostty_formatter_free(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plain_vt_html_and_writer() -> Result<()> {
        let mut terminal = Terminal::new(40, 4)?;
        terminal.vt_write(b"Hello, \x1b[1;32mworld\x1b[0m!\r\nsecond line   ");
        let mut formatter = Formatter::new(&terminal)?;
        assert_eq!(formatter.format_to_vec()?, b"Hello, world!\nsecond line");
        let mut streamed = Vec::new();
        formatter.format(&mut streamed).unwrap();
        assert_eq!(streamed, formatter.format_to_vec()?);
        formatter.set_format(FormatterFormat::Vt)?;
        formatter.set_extras(Extras::empty())?;
        let vt = formatter.format_to_vec()?;
        assert!(vt.contains(&0x1b));
        let mut restored = Terminal::new(40, 4)?;
        restored.vt_write(&vt);
        assert_eq!(
            Formatter::new(&restored)?.format_to_vec()?,
            b"Hello, world!\nsecond line"
        );
        formatter.set_format(FormatterFormat::Html)?;
        let html = String::from_utf8(formatter.format_to_vec()?).unwrap();
        assert!(html.contains("world"));
        assert!(html.starts_with("<div style=\"font-family: monospace; white-space: pre;"));
        assert!(html.contains("<div style=\"display: inline;"));
        assert!(html.contains("font-weight: bold;"));
        assert!(html.contains("world</div>"));
        Ok(())
    }
    #[test]
    fn writer_error_and_panic() -> Result<()> {
        let mut terminal = Terminal::new(10, 2)?;
        terminal.vt_write(b"hello");
        let formatter = Formatter::new(&terminal)?;
        struct Failing;
        impl io::Write for Failing {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "writer failure"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        assert_eq!(
            formatter.format(&mut Failing).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        struct Panicking;
        impl io::Write for Panicking {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                panic!("writer panic")
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || formatter.format(&mut Panicking)
            ))
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn unwrap_and_trim() -> Result<()> {
        let mut terminal = Terminal::new(5, 3)?;
        terminal.vt_write(b"abcdefgh  ");
        let mut formatter = Formatter::new(&terminal)?;
        assert_eq!(formatter.format_to_vec()?, b"abcde\nfgh");
        formatter.set_unwrap(true)?;
        assert_eq!(formatter.format_to_vec()?, b"abcdefgh");
        formatter.set_trim(false)?;
        assert_eq!(formatter.format_to_vec()?, b"abcdefgh  ");
        Ok(())
    }
}
