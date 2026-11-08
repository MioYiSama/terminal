//! Terminal modes (ANSI and DEC private) and DECRPM mode reports.

use ghostty_sys as sys;

use crate::Result;
use crate::ffi::{c_enum, encode_vec};

/// A terminal mode: a mode number plus whether it is an ANSI mode or a DEC
/// private (`?`-prefixed) mode. Layout-compatible with `GhosttyMode`.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mode(sys::GhosttyMode);

impl Mode {
    /// An ANSI mode (`CSI Ps h`). `value` is truncated to 15 bits.
    pub const fn ansi(value: u16) -> Self {
        Self((value & 0x7FFF) | 0x8000)
    }

    /// A DEC private mode (`CSI ? Ps h`). `value` is truncated to 15 bits.
    pub const fn dec(value: u16) -> Self {
        Self(value & 0x7FFF)
    }

    /// The mode number.
    pub const fn value(self) -> u16 {
        self.0 & 0x7FFF
    }

    /// Whether this is an ANSI mode rather than a DEC private mode.
    pub const fn is_ansi(self) -> bool {
        self.0 >> 15 != 0
    }

    pub(crate) const fn to_raw(self) -> sys::GhosttyMode {
        self.0
    }

    /// Encodes a DECRPM report (`CSI ? Ps1 ; Ps2 $ y`) for this mode.
    pub fn report(self, state: ModeReportState) -> Result<Vec<u8>> {
        encode_vec(|buf, len, written| unsafe {
            sys::ghostty_mode_report_encode(self.0, state.to_raw(), buf, len, written)
        })
    }
}

/// Well-known modes.
impl Mode {
    /// Keyboard action (disable keyboard)
    pub const DISABLE_KEYBOARD: Self = Self::ansi(2);
    /// Insert mode
    pub const INSERT: Self = Self::ansi(4);
    /// Send/receive mode
    pub const SEND_RECEIVE_MODE: Self = Self::ansi(12);
    /// Linefeed/new line mode
    pub const LINEFEED: Self = Self::ansi(20);
    /// Cursor keys
    pub const CURSOR_KEYS: Self = Self::dec(1);
    /// 132/80 column mode
    pub const COLUMN_132: Self = Self::dec(3);
    /// Slow scroll
    pub const SLOW_SCROLL: Self = Self::dec(4);
    /// Reverse video
    pub const REVERSE_COLORS: Self = Self::dec(5);
    /// Origin mode
    pub const ORIGIN: Self = Self::dec(6);
    /// Auto-wrap mode
    pub const WRAPAROUND: Self = Self::dec(7);
    /// Auto-repeat keys
    pub const AUTOREPEAT: Self = Self::dec(8);
    /// X10 mouse reporting
    pub const MOUSE_EVENT_X10: Self = Self::dec(9);
    /// Cursor blink
    pub const CURSOR_BLINKING: Self = Self::dec(12);
    /// Cursor visible (DECTCEM)
    pub const CURSOR_VISIBLE: Self = Self::dec(25);
    /// Allow 132 column mode
    pub const ENABLE_MODE_3: Self = Self::dec(40);
    /// Reverse wrap
    pub const REVERSE_WRAP: Self = Self::dec(45);
    /// Alternate screen (legacy)
    pub const ALT_SCREEN_LEGACY: Self = Self::dec(47);
    /// Application keypad
    pub const KEYPAD_KEYS: Self = Self::dec(66);
    /// Backarrow key mode (DECBKM)
    pub const BACKARROW_KEY_MODE: Self = Self::dec(67);
    /// Left/right margin mode
    pub const ENABLE_LEFT_AND_RIGHT_MARGIN: Self = Self::dec(69);
    /// Normal mouse tracking
    pub const MOUSE_EVENT_NORMAL: Self = Self::dec(1000);
    /// Button-event mouse tracking
    pub const MOUSE_EVENT_BUTTON: Self = Self::dec(1002);
    /// Any-event mouse tracking
    pub const MOUSE_EVENT_ANY: Self = Self::dec(1003);
    /// Focus in/out events
    pub const FOCUS_EVENT: Self = Self::dec(1004);
    /// UTF-8 mouse format
    pub const MOUSE_FORMAT_UTF8: Self = Self::dec(1005);
    /// SGR mouse format
    pub const MOUSE_FORMAT_SGR: Self = Self::dec(1006);
    /// Alternate scroll mode
    pub const MOUSE_ALTERNATE_SCROLL: Self = Self::dec(1007);
    /// URxvt mouse format
    pub const MOUSE_FORMAT_URXVT: Self = Self::dec(1015);
    /// SGR-Pixels mouse format
    pub const MOUSE_FORMAT_SGR_PIXELS: Self = Self::dec(1016);
    /// Ignore keypad with NumLock
    pub const IGNORE_KEYPAD_WITH_NUMLOCK: Self = Self::dec(1035);
    /// Alt key sends ESC prefix
    pub const ALT_ESC_PREFIX: Self = Self::dec(1036);
    /// Alt sends escape
    pub const ALT_SENDS_ESCAPE: Self = Self::dec(1039);
    /// Extended reverse wrap
    pub const REVERSE_WRAP_EXTENDED: Self = Self::dec(1045);
    /// Alternate screen
    pub const ALT_SCREEN: Self = Self::dec(1047);
    /// Save cursor (DECSC)
    pub const SAVE_CURSOR: Self = Self::dec(1048);
    /// Alt screen + save cursor + clear
    pub const ALT_SCREEN_SAVE_CURSOR_CLEAR_ENTER: Self = Self::dec(1049);
    /// Bracketed paste mode
    pub const BRACKETED_PASTE: Self = Self::dec(2004);
    /// Synchronized output
    pub const SYNCHRONIZED_OUTPUT: Self = Self::dec(2026);
    /// Grapheme cluster mode
    pub const GRAPHEME_CLUSTER: Self = Self::dec(2027);
    /// Report color scheme
    pub const REPORT_COLOR_SCHEME: Self = Self::dec(2031);
    /// Report terminal visibility
    pub const REPORT_VISIBILITY: Self = Self::dec(2033);
    /// In-band size reports
    pub const IN_BAND_SIZE_REPORTS: Self = Self::dec(2048);
    /// Kitty clipboard protocol paste events
    pub const KITTY_PASTE_EVENTS: Self = Self::dec(5522);
}

c_enum! {
    /// The state reported in a DECRPM report.
    pub enum ModeReportState {
        NotRecognized = sys::GHOSTTY_MODE_REPORT_NOT_RECOGNIZED,
        Set = sys::GHOSTTY_MODE_REPORT_SET,
        Reset = sys::GHOSTTY_MODE_REPORT_RESET,
        PermanentlySet = sys::GHOSTTY_MODE_REPORT_PERMANENTLY_SET,
        PermanentlyReset = sys::GHOSTTY_MODE_REPORT_PERMANENTLY_RESET,
    }
}
