//! OSC (Operating System Command) sequence parser.

use std::{marker::PhantomData, ptr::NonNull};

use ghostty_sys as sys;

use crate::ffi::c_enum;
use crate::{Error, Result, ffi};

c_enum! {
    /// The kind of command decoded from an OSC sequence.
    pub enum OscCommandType {
        /// Invalid, malformed, or cancelled sequence.
        Invalid = sys::GHOSTTY_OSC_COMMAND_INVALID,
        /// Change the window title.
        ChangeWindowTitle = sys::GHOSTTY_OSC_COMMAND_CHANGE_WINDOW_TITLE,
        /// Change the window icon.
        ChangeWindowIcon = sys::GHOSTTY_OSC_COMMAND_CHANGE_WINDOW_ICON,
        /// Semantic prompt marker.
        SemanticPrompt = sys::GHOSTTY_OSC_COMMAND_SEMANTIC_PROMPT,
        /// Clipboard contents.
        ClipboardContents = sys::GHOSTTY_OSC_COMMAND_CLIPBOARD_CONTENTS,
        /// Report the working directory.
        ReportPwd = sys::GHOSTTY_OSC_COMMAND_REPORT_PWD,
        /// Mouse cursor shape.
        MouseShape = sys::GHOSTTY_OSC_COMMAND_MOUSE_SHAPE,
        /// Color operation.
        ColorOperation = sys::GHOSTTY_OSC_COMMAND_COLOR_OPERATION,
        /// Kitty color protocol.
        KittyColorProtocol = sys::GHOSTTY_OSC_COMMAND_KITTY_COLOR_PROTOCOL,
        /// Desktop notification.
        ShowDesktopNotification = sys::GHOSTTY_OSC_COMMAND_SHOW_DESKTOP_NOTIFICATION,
        /// Start a hyperlink.
        HyperlinkStart = sys::GHOSTTY_OSC_COMMAND_HYPERLINK_START,
        /// End a hyperlink.
        HyperlinkEnd = sys::GHOSTTY_OSC_COMMAND_HYPERLINK_END,
        /// ConEmu sleep command.
        ConEmuSleep = sys::GHOSTTY_OSC_COMMAND_CONEMU_SLEEP,
        /// ConEmu message box.
        ConEmuShowMessageBox = sys::GHOSTTY_OSC_COMMAND_CONEMU_SHOW_MESSAGE_BOX,
        /// ConEmu tab title.
        ConEmuChangeTabTitle = sys::GHOSTTY_OSC_COMMAND_CONEMU_CHANGE_TAB_TITLE,
        /// ConEmu progress report.
        ConEmuProgressReport = sys::GHOSTTY_OSC_COMMAND_CONEMU_PROGRESS_REPORT,
        /// ConEmu wait for input.
        ConEmuWaitInput = sys::GHOSTTY_OSC_COMMAND_CONEMU_WAIT_INPUT,
        /// ConEmu GUI macro.
        ConEmuGuiMacro = sys::GHOSTTY_OSC_COMMAND_CONEMU_GUIMACRO,
        /// ConEmu process launch.
        ConEmuRunProcess = sys::GHOSTTY_OSC_COMMAND_CONEMU_RUN_PROCESS,
        /// ConEmu environment variable output.
        ConEmuOutputEnvironmentVariable = sys::GHOSTTY_OSC_COMMAND_CONEMU_OUTPUT_ENVIRONMENT_VARIABLE,
        /// ConEmu xterm emulation.
        ConEmuXtermEmulation = sys::GHOSTTY_OSC_COMMAND_CONEMU_XTERM_EMULATION,
        /// ConEmu comment.
        ConEmuComment = sys::GHOSTTY_OSC_COMMAND_CONEMU_COMMENT,
        /// Kitty text sizing.
        KittyTextSizing = sys::GHOSTTY_OSC_COMMAND_KITTY_TEXT_SIZING,
        /// Kitty clipboard protocol.
        KittyClipboardProtocol = sys::GHOSTTY_OSC_COMMAND_KITTY_CLIPBOARD_PROTOCOL,
        /// Kitty drag-and-drop protocol.
        KittyDndProtocol = sys::GHOSTTY_OSC_COMMAND_KITTY_DND_PROTOCOL,
        /// Hierarchical context.
        HierarchicalContext = sys::GHOSTTY_OSC_COMMAND_HIERARCHICAL_CONTEXT,
        /// Kitty desktop notification.
        KittyDesktopNotification = sys::GHOSTTY_OSC_COMMAND_KITTY_DESKTOP_NOTIFICATION,
        /// An unimplemented OSC number retained by the parser.
        Unknown = sys::GHOSTTY_OSC_COMMAND_UNKNOWN,
        /// Program status report or support query (OSC 7501).
        ProgramStatus = sys::GHOSTTY_OSC_COMMAND_PROGRAM_STATUS,
    }
}

c_enum! {
    /// How an OSC request ended; replies should use the same terminator.
    pub enum OscTerminator {
        /// String terminator, ESC followed by a backslash.
        St = sys::GHOSTTY_OSC_TERMINATOR_ST,
        /// Bell (0x07).
        Bel = sys::GHOSTTY_OSC_TERMINATOR_BEL,
    }
}

impl From<OscTerminator> for u8 {
    fn from(value: OscTerminator) -> Self {
        match value {
            OscTerminator::St => b'\\',
            OscTerminator::Bel => 0x07,
        }
    }
}

/// An owning parser for the bytes between `ESC ]` and an OSC terminator.
pub struct OscParser {
    raw: NonNull<sys::GhosttyOscParserImpl>,
}

// SAFETY: the parser owns independent state, has no thread affinity, and all
// parser operations require exclusive access. It is deliberately not Sync.
unsafe impl Send for OscParser {}

impl OscParser {
    /// Creates a parser using the library's default allocator.
    pub fn new() -> Result<Self> {
        let mut raw = std::ptr::null_mut();
        unsafe { Error::check(sys::ghostty_osc_new(std::ptr::null(), &mut raw))? };
        Ok(Self {
            raw: NonNull::new(raw).expect("successful OSC allocation returned null"),
        })
    }

    /// Clears the sequence being parsed, preserving options.
    pub fn reset(&mut self) {
        unsafe { sys::ghostty_osc_reset(self.raw.as_ptr()) };
    }

    /// Retains up to `max_bytes` from unknown sequences; zero disables retention.
    pub fn set_unknown_max_bytes(&mut self, max_bytes: usize) -> Result<()> {
        unsafe {
            Error::check(sys::ghostty_osc_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_OSC_OPT_UNKNOWN_MAX_BYTES,
                (&raw const max_bytes).cast(),
            ))
        }
    }

    /// Feeds one payload byte, excluding the introducer and terminator.
    pub fn next(&mut self, byte: u8) {
        unsafe { sys::ghostty_osc_next(self.raw.as_ptr(), byte) };
    }

    /// Feeds payload bytes, excluding the introducer and terminator.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.next(byte);
        }
    }

    /// Finalizes a sequence and borrows its result until the command is dropped.
    ///
    /// Accepts an [`OscTerminator`] or a raw terminating byte, including CAN
    /// (0x18) and SUB (0x1a), which discard the command. Invalid commands have
    /// type [`OscCommandType::Invalid`]. Reset before parsing another sequence.
    pub fn end(&mut self, terminator: impl Into<u8>) -> OscCommand<'_> {
        let raw = unsafe { sys::ghostty_osc_end(self.raw.as_ptr(), terminator.into()) };
        OscCommand {
            raw,
            _parser: PhantomData,
        }
    }
}

impl Drop for OscParser {
    fn drop(&mut self) {
        unsafe { sys::ghostty_osc_free(self.raw.as_ptr()) };
    }
}

/// A decoded OSC command borrowing its parser exclusively.
///
/// Introspection does not invalidate the command. The exclusive parser borrow
/// prevents reset, feeding, option changes, or freeing while its data is used.
pub struct OscCommand<'parser> {
    raw: sys::GhosttyOscCommand,
    _parser: PhantomData<&'parser mut OscParser>,
}

impl OscCommand<'_> {
    /// Returns the command kind, including `Invalid` for a rejected sequence.
    pub fn command_type(&self) -> OscCommandType {
        OscCommandType::from_lib(unsafe { sys::ghostty_osc_command_type(self.raw) })
    }

    /// Borrows the window title bytes, or returns `Error::NoValue` for other commands.
    pub fn window_title(&self) -> Result<&[u8]> {
        self.bytes(sys::GHOSTTY_OSC_DATA_CHANGE_WINDOW_TITLE_STR)
    }

    /// Borrows all retained bytes of an unknown sequence, including its number.
    pub fn unknown_content(&self) -> Result<&[u8]> {
        self.bytes(sys::GHOSTTY_OSC_DATA_UNKNOWN_CONTENT)
    }

    /// Reports whether an unknown sequence exceeded the retention limit.
    pub fn unknown_truncated(&self) -> Result<bool> {
        let mut value = false;
        unsafe {
            Error::check(sys::ghostty_osc_command_data(
                self.raw,
                sys::GHOSTTY_OSC_DATA_UNKNOWN_TRUNCATED,
                (&raw mut value).cast(),
            ))?;
        }
        Ok(value)
    }

    /// Returns the terminator used by an unknown sequence.
    pub fn unknown_terminator(&self) -> Result<OscTerminator> {
        let mut value = sys::GHOSTTY_OSC_TERMINATOR_ST;
        unsafe {
            Error::check(sys::ghostty_osc_command_data(
                self.raw,
                sys::GHOSTTY_OSC_DATA_UNKNOWN_TERMINATOR,
                (&raw mut value).cast(),
            ))?;
        }
        Ok(OscTerminator::from_lib(value))
    }

    fn bytes(&self, data: sys::GhosttyOscCommandData) -> Result<&[u8]> {
        let mut value = sys::GhosttyString::default();
        // SAFETY: callers supply only GhosttyString data keys. The parser cannot
        // mutate while this command is borrowed; introspection is nonmutating.
        unsafe {
            Error::check(sys::ghostty_osc_command_data(
                self.raw,
                data,
                (&raw mut value).cast(),
            ))?;
            Ok(ffi::string_bytes(value))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_window_title() {
        let mut parser = OscParser::new().unwrap();
        parser.feed(b"2;hello");
        let command = parser.end(OscTerminator::Bel);
        assert_eq!(command.command_type(), OscCommandType::ChangeWindowTitle);
        assert_eq!(command.window_title().unwrap(), b"hello");
        assert_eq!(command.unknown_content(), Err(Error::NoValue));
    }

    #[test]
    fn retain_unknown_and_reset() {
        let mut parser = OscParser::new().unwrap();
        parser.set_unknown_max_bytes(8).unwrap();
        parser.feed(b"7400;status=busy");
        let command = parser.end(OscTerminator::St);
        assert_eq!(command.command_type(), OscCommandType::Unknown);
        assert_eq!(command.unknown_content().unwrap(), b"7400;sta");
        assert!(command.unknown_truncated().unwrap());
        assert_eq!(command.unknown_terminator().unwrap(), OscTerminator::St);
        parser.reset();
        parser.feed(b"7400;ok");
        let command = parser.end(OscTerminator::Bel);
        assert_eq!(command.unknown_content().unwrap(), b"7400;ok");
        assert!(!command.unknown_truncated().unwrap());
        assert_eq!(command.unknown_terminator().unwrap(), OscTerminator::Bel);
    }

    #[test]
    fn cancelled_title_is_invalid() {
        let mut parser = OscParser::new().unwrap();
        for terminator in [0x18u8, 0x1a] {
            parser.reset();
            parser.feed(b"2;cancelled");
            let command = parser.end(terminator);
            assert_eq!(command.command_type(), OscCommandType::Invalid);
        }
    }
}
