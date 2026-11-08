//! Key events and the key encoder (legacy, xterm and Kitty keyboard protocols).

use crate::{Error, Result, ffi, terminal::Terminal};
use core::ptr::{self, NonNull};
use ghostty_sys as sys;

ffi::c_enum! {
    /// A layout-independent physical key.
    pub enum Key {
        /// Unidentified.
        Unidentified = sys::GHOSTTY_KEY_UNIDENTIFIED,
        /// Backquote.
        Backquote = sys::GHOSTTY_KEY_BACKQUOTE,
        /// Backslash.
        Backslash = sys::GHOSTTY_KEY_BACKSLASH,
        /// BracketLeft.
        BracketLeft = sys::GHOSTTY_KEY_BRACKET_LEFT,
        /// BracketRight.
        BracketRight = sys::GHOSTTY_KEY_BRACKET_RIGHT,
        /// Comma.
        Comma = sys::GHOSTTY_KEY_COMMA,
        /// Digit0.
        Digit0 = sys::GHOSTTY_KEY_DIGIT_0,
        /// Digit1.
        Digit1 = sys::GHOSTTY_KEY_DIGIT_1,
        /// Digit2.
        Digit2 = sys::GHOSTTY_KEY_DIGIT_2,
        /// Digit3.
        Digit3 = sys::GHOSTTY_KEY_DIGIT_3,
        /// Digit4.
        Digit4 = sys::GHOSTTY_KEY_DIGIT_4,
        /// Digit5.
        Digit5 = sys::GHOSTTY_KEY_DIGIT_5,
        /// Digit6.
        Digit6 = sys::GHOSTTY_KEY_DIGIT_6,
        /// Digit7.
        Digit7 = sys::GHOSTTY_KEY_DIGIT_7,
        /// Digit8.
        Digit8 = sys::GHOSTTY_KEY_DIGIT_8,
        /// Digit9.
        Digit9 = sys::GHOSTTY_KEY_DIGIT_9,
        /// Equal.
        Equal = sys::GHOSTTY_KEY_EQUAL,
        /// IntlBackslash.
        IntlBackslash = sys::GHOSTTY_KEY_INTL_BACKSLASH,
        /// IntlRo.
        IntlRo = sys::GHOSTTY_KEY_INTL_RO,
        /// IntlYen.
        IntlYen = sys::GHOSTTY_KEY_INTL_YEN,
        /// A.
        A = sys::GHOSTTY_KEY_A,
        /// B.
        B = sys::GHOSTTY_KEY_B,
        /// C.
        C = sys::GHOSTTY_KEY_C,
        /// D.
        D = sys::GHOSTTY_KEY_D,
        /// E.
        E = sys::GHOSTTY_KEY_E,
        /// F.
        F = sys::GHOSTTY_KEY_F,
        /// G.
        G = sys::GHOSTTY_KEY_G,
        /// H.
        H = sys::GHOSTTY_KEY_H,
        /// I.
        I = sys::GHOSTTY_KEY_I,
        /// J.
        J = sys::GHOSTTY_KEY_J,
        /// K.
        K = sys::GHOSTTY_KEY_K,
        /// L.
        L = sys::GHOSTTY_KEY_L,
        /// M.
        M = sys::GHOSTTY_KEY_M,
        /// N.
        N = sys::GHOSTTY_KEY_N,
        /// O.
        O = sys::GHOSTTY_KEY_O,
        /// P.
        P = sys::GHOSTTY_KEY_P,
        /// Q.
        Q = sys::GHOSTTY_KEY_Q,
        /// R.
        R = sys::GHOSTTY_KEY_R,
        /// S.
        S = sys::GHOSTTY_KEY_S,
        /// T.
        T = sys::GHOSTTY_KEY_T,
        /// U.
        U = sys::GHOSTTY_KEY_U,
        /// V.
        V = sys::GHOSTTY_KEY_V,
        /// W.
        W = sys::GHOSTTY_KEY_W,
        /// X.
        X = sys::GHOSTTY_KEY_X,
        /// Y.
        Y = sys::GHOSTTY_KEY_Y,
        /// Z.
        Z = sys::GHOSTTY_KEY_Z,
        /// Minus.
        Minus = sys::GHOSTTY_KEY_MINUS,
        /// Period.
        Period = sys::GHOSTTY_KEY_PERIOD,
        /// Quote.
        Quote = sys::GHOSTTY_KEY_QUOTE,
        /// Semicolon.
        Semicolon = sys::GHOSTTY_KEY_SEMICOLON,
        /// Slash.
        Slash = sys::GHOSTTY_KEY_SLASH,
        /// AltLeft.
        AltLeft = sys::GHOSTTY_KEY_ALT_LEFT,
        /// AltRight.
        AltRight = sys::GHOSTTY_KEY_ALT_RIGHT,
        /// Backspace.
        Backspace = sys::GHOSTTY_KEY_BACKSPACE,
        /// CapsLock.
        CapsLock = sys::GHOSTTY_KEY_CAPS_LOCK,
        /// ContextMenu.
        ContextMenu = sys::GHOSTTY_KEY_CONTEXT_MENU,
        /// ControlLeft.
        ControlLeft = sys::GHOSTTY_KEY_CONTROL_LEFT,
        /// ControlRight.
        ControlRight = sys::GHOSTTY_KEY_CONTROL_RIGHT,
        /// Enter.
        Enter = sys::GHOSTTY_KEY_ENTER,
        /// MetaLeft.
        MetaLeft = sys::GHOSTTY_KEY_META_LEFT,
        /// MetaRight.
        MetaRight = sys::GHOSTTY_KEY_META_RIGHT,
        /// ShiftLeft.
        ShiftLeft = sys::GHOSTTY_KEY_SHIFT_LEFT,
        /// ShiftRight.
        ShiftRight = sys::GHOSTTY_KEY_SHIFT_RIGHT,
        /// Space.
        Space = sys::GHOSTTY_KEY_SPACE,
        /// Tab.
        Tab = sys::GHOSTTY_KEY_TAB,
        /// Convert.
        Convert = sys::GHOSTTY_KEY_CONVERT,
        /// KanaMode.
        KanaMode = sys::GHOSTTY_KEY_KANA_MODE,
        /// NonConvert.
        NonConvert = sys::GHOSTTY_KEY_NON_CONVERT,
        /// Delete.
        Delete = sys::GHOSTTY_KEY_DELETE,
        /// End.
        End = sys::GHOSTTY_KEY_END,
        /// Help.
        Help = sys::GHOSTTY_KEY_HELP,
        /// Home.
        Home = sys::GHOSTTY_KEY_HOME,
        /// Insert.
        Insert = sys::GHOSTTY_KEY_INSERT,
        /// PageDown.
        PageDown = sys::GHOSTTY_KEY_PAGE_DOWN,
        /// PageUp.
        PageUp = sys::GHOSTTY_KEY_PAGE_UP,
        /// ArrowDown.
        ArrowDown = sys::GHOSTTY_KEY_ARROW_DOWN,
        /// ArrowLeft.
        ArrowLeft = sys::GHOSTTY_KEY_ARROW_LEFT,
        /// ArrowRight.
        ArrowRight = sys::GHOSTTY_KEY_ARROW_RIGHT,
        /// ArrowUp.
        ArrowUp = sys::GHOSTTY_KEY_ARROW_UP,
        /// NumLock.
        NumLock = sys::GHOSTTY_KEY_NUM_LOCK,
        /// Numpad0.
        Numpad0 = sys::GHOSTTY_KEY_NUMPAD_0,
        /// Numpad1.
        Numpad1 = sys::GHOSTTY_KEY_NUMPAD_1,
        /// Numpad2.
        Numpad2 = sys::GHOSTTY_KEY_NUMPAD_2,
        /// Numpad3.
        Numpad3 = sys::GHOSTTY_KEY_NUMPAD_3,
        /// Numpad4.
        Numpad4 = sys::GHOSTTY_KEY_NUMPAD_4,
        /// Numpad5.
        Numpad5 = sys::GHOSTTY_KEY_NUMPAD_5,
        /// Numpad6.
        Numpad6 = sys::GHOSTTY_KEY_NUMPAD_6,
        /// Numpad7.
        Numpad7 = sys::GHOSTTY_KEY_NUMPAD_7,
        /// Numpad8.
        Numpad8 = sys::GHOSTTY_KEY_NUMPAD_8,
        /// Numpad9.
        Numpad9 = sys::GHOSTTY_KEY_NUMPAD_9,
        /// NumpadAdd.
        NumpadAdd = sys::GHOSTTY_KEY_NUMPAD_ADD,
        /// NumpadBackspace.
        NumpadBackspace = sys::GHOSTTY_KEY_NUMPAD_BACKSPACE,
        /// NumpadClear.
        NumpadClear = sys::GHOSTTY_KEY_NUMPAD_CLEAR,
        /// NumpadClearEntry.
        NumpadClearEntry = sys::GHOSTTY_KEY_NUMPAD_CLEAR_ENTRY,
        /// NumpadComma.
        NumpadComma = sys::GHOSTTY_KEY_NUMPAD_COMMA,
        /// NumpadDecimal.
        NumpadDecimal = sys::GHOSTTY_KEY_NUMPAD_DECIMAL,
        /// NumpadDivide.
        NumpadDivide = sys::GHOSTTY_KEY_NUMPAD_DIVIDE,
        /// NumpadEnter.
        NumpadEnter = sys::GHOSTTY_KEY_NUMPAD_ENTER,
        /// NumpadEqual.
        NumpadEqual = sys::GHOSTTY_KEY_NUMPAD_EQUAL,
        /// NumpadMemoryAdd.
        NumpadMemoryAdd = sys::GHOSTTY_KEY_NUMPAD_MEMORY_ADD,
        /// NumpadMemoryClear.
        NumpadMemoryClear = sys::GHOSTTY_KEY_NUMPAD_MEMORY_CLEAR,
        /// NumpadMemoryRecall.
        NumpadMemoryRecall = sys::GHOSTTY_KEY_NUMPAD_MEMORY_RECALL,
        /// NumpadMemoryStore.
        NumpadMemoryStore = sys::GHOSTTY_KEY_NUMPAD_MEMORY_STORE,
        /// NumpadMemorySubtract.
        NumpadMemorySubtract = sys::GHOSTTY_KEY_NUMPAD_MEMORY_SUBTRACT,
        /// NumpadMultiply.
        NumpadMultiply = sys::GHOSTTY_KEY_NUMPAD_MULTIPLY,
        /// NumpadParenLeft.
        NumpadParenLeft = sys::GHOSTTY_KEY_NUMPAD_PAREN_LEFT,
        /// NumpadParenRight.
        NumpadParenRight = sys::GHOSTTY_KEY_NUMPAD_PAREN_RIGHT,
        /// NumpadSubtract.
        NumpadSubtract = sys::GHOSTTY_KEY_NUMPAD_SUBTRACT,
        /// NumpadSeparator.
        NumpadSeparator = sys::GHOSTTY_KEY_NUMPAD_SEPARATOR,
        /// NumpadUp.
        NumpadUp = sys::GHOSTTY_KEY_NUMPAD_UP,
        /// NumpadDown.
        NumpadDown = sys::GHOSTTY_KEY_NUMPAD_DOWN,
        /// NumpadRight.
        NumpadRight = sys::GHOSTTY_KEY_NUMPAD_RIGHT,
        /// NumpadLeft.
        NumpadLeft = sys::GHOSTTY_KEY_NUMPAD_LEFT,
        /// NumpadBegin.
        NumpadBegin = sys::GHOSTTY_KEY_NUMPAD_BEGIN,
        /// NumpadHome.
        NumpadHome = sys::GHOSTTY_KEY_NUMPAD_HOME,
        /// NumpadEnd.
        NumpadEnd = sys::GHOSTTY_KEY_NUMPAD_END,
        /// NumpadInsert.
        NumpadInsert = sys::GHOSTTY_KEY_NUMPAD_INSERT,
        /// NumpadDelete.
        NumpadDelete = sys::GHOSTTY_KEY_NUMPAD_DELETE,
        /// NumpadPageUp.
        NumpadPageUp = sys::GHOSTTY_KEY_NUMPAD_PAGE_UP,
        /// NumpadPageDown.
        NumpadPageDown = sys::GHOSTTY_KEY_NUMPAD_PAGE_DOWN,
        /// Escape.
        Escape = sys::GHOSTTY_KEY_ESCAPE,
        /// F1.
        F1 = sys::GHOSTTY_KEY_F1,
        /// F2.
        F2 = sys::GHOSTTY_KEY_F2,
        /// F3.
        F3 = sys::GHOSTTY_KEY_F3,
        /// F4.
        F4 = sys::GHOSTTY_KEY_F4,
        /// F5.
        F5 = sys::GHOSTTY_KEY_F5,
        /// F6.
        F6 = sys::GHOSTTY_KEY_F6,
        /// F7.
        F7 = sys::GHOSTTY_KEY_F7,
        /// F8.
        F8 = sys::GHOSTTY_KEY_F8,
        /// F9.
        F9 = sys::GHOSTTY_KEY_F9,
        /// F10.
        F10 = sys::GHOSTTY_KEY_F10,
        /// F11.
        F11 = sys::GHOSTTY_KEY_F11,
        /// F12.
        F12 = sys::GHOSTTY_KEY_F12,
        /// F13.
        F13 = sys::GHOSTTY_KEY_F13,
        /// F14.
        F14 = sys::GHOSTTY_KEY_F14,
        /// F15.
        F15 = sys::GHOSTTY_KEY_F15,
        /// F16.
        F16 = sys::GHOSTTY_KEY_F16,
        /// F17.
        F17 = sys::GHOSTTY_KEY_F17,
        /// F18.
        F18 = sys::GHOSTTY_KEY_F18,
        /// F19.
        F19 = sys::GHOSTTY_KEY_F19,
        /// F20.
        F20 = sys::GHOSTTY_KEY_F20,
        /// F21.
        F21 = sys::GHOSTTY_KEY_F21,
        /// F22.
        F22 = sys::GHOSTTY_KEY_F22,
        /// F23.
        F23 = sys::GHOSTTY_KEY_F23,
        /// F24.
        F24 = sys::GHOSTTY_KEY_F24,
        /// F25.
        F25 = sys::GHOSTTY_KEY_F25,
        /// Fn.
        Fn = sys::GHOSTTY_KEY_FN,
        /// FnLock.
        FnLock = sys::GHOSTTY_KEY_FN_LOCK,
        /// PrintScreen.
        PrintScreen = sys::GHOSTTY_KEY_PRINT_SCREEN,
        /// ScrollLock.
        ScrollLock = sys::GHOSTTY_KEY_SCROLL_LOCK,
        /// Pause.
        Pause = sys::GHOSTTY_KEY_PAUSE,
        /// BrowserBack.
        BrowserBack = sys::GHOSTTY_KEY_BROWSER_BACK,
        /// BrowserFavorites.
        BrowserFavorites = sys::GHOSTTY_KEY_BROWSER_FAVORITES,
        /// BrowserForward.
        BrowserForward = sys::GHOSTTY_KEY_BROWSER_FORWARD,
        /// BrowserHome.
        BrowserHome = sys::GHOSTTY_KEY_BROWSER_HOME,
        /// BrowserRefresh.
        BrowserRefresh = sys::GHOSTTY_KEY_BROWSER_REFRESH,
        /// BrowserSearch.
        BrowserSearch = sys::GHOSTTY_KEY_BROWSER_SEARCH,
        /// BrowserStop.
        BrowserStop = sys::GHOSTTY_KEY_BROWSER_STOP,
        /// Eject.
        Eject = sys::GHOSTTY_KEY_EJECT,
        /// LaunchApp1.
        LaunchApp1 = sys::GHOSTTY_KEY_LAUNCH_APP_1,
        /// LaunchApp2.
        LaunchApp2 = sys::GHOSTTY_KEY_LAUNCH_APP_2,
        /// LaunchMail.
        LaunchMail = sys::GHOSTTY_KEY_LAUNCH_MAIL,
        /// MediaPlayPause.
        MediaPlayPause = sys::GHOSTTY_KEY_MEDIA_PLAY_PAUSE,
        /// MediaSelect.
        MediaSelect = sys::GHOSTTY_KEY_MEDIA_SELECT,
        /// MediaStop.
        MediaStop = sys::GHOSTTY_KEY_MEDIA_STOP,
        /// MediaTrackNext.
        MediaTrackNext = sys::GHOSTTY_KEY_MEDIA_TRACK_NEXT,
        /// MediaTrackPrevious.
        MediaTrackPrevious = sys::GHOSTTY_KEY_MEDIA_TRACK_PREVIOUS,
        /// Power.
        Power = sys::GHOSTTY_KEY_POWER,
        /// Sleep.
        Sleep = sys::GHOSTTY_KEY_SLEEP,
        /// AudioVolumeDown.
        AudioVolumeDown = sys::GHOSTTY_KEY_AUDIO_VOLUME_DOWN,
        /// AudioVolumeMute.
        AudioVolumeMute = sys::GHOSTTY_KEY_AUDIO_VOLUME_MUTE,
        /// AudioVolumeUp.
        AudioVolumeUp = sys::GHOSTTY_KEY_AUDIO_VOLUME_UP,
        /// WakeUp.
        WakeUp = sys::GHOSTTY_KEY_WAKE_UP,
        /// Copy.
        Copy = sys::GHOSTTY_KEY_COPY,
        /// Cut.
        Cut = sys::GHOSTTY_KEY_CUT,
        /// Paste.
        Paste = sys::GHOSTTY_KEY_PASTE,
    }
}

ffi::c_enum! {
    /// Keyboard event action.
    pub enum KeyAction {
        /// Key released.
        Release = sys::GHOSTTY_KEY_ACTION_RELEASE,
        /// Key pressed.
        Press = sys::GHOSTTY_KEY_ACTION_PRESS,
        /// Held key repeated.
        Repeat = sys::GHOSTTY_KEY_ACTION_REPEAT,
    }
}

ffi::c_enum! {
    /// Whether macOS Option acts as Alt.
    pub enum OptionAsAlt {
        /// Neither Option key acts as Alt.
        False = sys::GHOSTTY_OPTION_AS_ALT_FALSE,
        /// Both Option keys act as Alt.
        True = sys::GHOSTTY_OPTION_AS_ALT_TRUE,
        /// Only left Option acts as Alt.
        Left = sys::GHOSTTY_OPTION_AS_ALT_LEFT,
        /// Only right Option acts as Alt.
        Right = sys::GHOSTTY_OPTION_AS_ALT_RIGHT,
    }
}

bitflags::bitflags! {
    /// Keyboard modifiers; side bits mean right when the corresponding modifier is set.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct Mods: u16 {
        /// Shift.
        const SHIFT = sys::GHOSTTY_MODS_SHIFT as u16;
        /// Control.
        const CTRL = sys::GHOSTTY_MODS_CTRL as u16;
        /// Alt or Option.
        const ALT = sys::GHOSTTY_MODS_ALT as u16;
        /// Super, Command or Windows.
        const SUPER = sys::GHOSTTY_MODS_SUPER as u16;
        /// Caps Lock.
        const CAPS_LOCK = sys::GHOSTTY_MODS_CAPS_LOCK as u16;
        /// Num Lock.
        const NUM_LOCK = sys::GHOSTTY_MODS_NUM_LOCK as u16;
        /// Right Shift.
        const SHIFT_SIDE = sys::GHOSTTY_MODS_SHIFT_SIDE as u16;
        /// Right Control.
        const CTRL_SIDE = sys::GHOSTTY_MODS_CTRL_SIDE as u16;
        /// Right Alt.
        const ALT_SIDE = sys::GHOSTTY_MODS_ALT_SIDE as u16;
        /// Right Super.
        const SUPER_SIDE = sys::GHOSTTY_MODS_SUPER_SIDE as u16;
    }
}

bitflags::bitflags! {
    /// Kitty keyboard protocol features; empty flags disable the protocol.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct KittyKeyFlags: u8 {
        /// Disambiguate escape codes.
        const DISAMBIGUATE = sys::GHOSTTY_KITTY_KEY_DISAMBIGUATE as u8;
        /// Report press and release events.
        const REPORT_EVENTS = sys::GHOSTTY_KITTY_KEY_REPORT_EVENTS as u8;
        /// Report alternate key codes.
        const REPORT_ALTERNATES = sys::GHOSTTY_KITTY_KEY_REPORT_ALTERNATES as u8;
        /// Report all key events.
        const REPORT_ALL = sys::GHOSTTY_KITTY_KEY_REPORT_ALL as u8;
        /// Report associated text.
        const REPORT_ASSOCIATED = sys::GHOSTTY_KITTY_KEY_REPORT_ASSOCIATED as u8;
    }
}

/// An owned key event, including the text borrowed by the C event.
pub struct KeyEvent {
    raw: NonNull<sys::GhosttyKeyEventImpl>,
    utf8: String,
}

// SAFETY: the event owns independent state and its text allocation; no thread affinity.
unsafe impl Send for KeyEvent {}

impl KeyEvent {
    /// Creates an event with library defaults.
    pub fn new() -> Result<Self> {
        let mut raw = ptr::null_mut();
        Error::check(unsafe { sys::ghostty_key_event_new(ptr::null(), &mut raw) })?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
            utf8: String::new(),
        })
    }
    /// Sets the event action.
    pub fn set_action(&mut self, value: KeyAction) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_key_event_set_action(self.raw.as_ptr(), value.to_raw())
        })
    }

    /// Returns the event action.
    pub fn action(&self) -> KeyAction {
        let raw = unsafe { sys::ghostty_key_event_get_action(self.raw.as_ptr()) };
        KeyAction::from_lib(raw)
    }

    /// Sets the physical key.
    pub fn set_key(&mut self, value: Key) -> Result<()> {
        Error::check(unsafe { sys::ghostty_key_event_set_key(self.raw.as_ptr(), value.to_raw()) })
    }

    /// Returns the physical key.
    pub fn key(&self) -> Key {
        let raw = unsafe { sys::ghostty_key_event_get_key(self.raw.as_ptr()) };
        Key::from_lib(raw)
    }

    /// Sets the keyboard modifiers.
    pub fn set_mods(&mut self, value: Mods) -> Result<()> {
        Error::check(unsafe { sys::ghostty_key_event_set_mods(self.raw.as_ptr(), value.bits()) })
    }

    /// Returns the keyboard modifiers.
    pub fn mods(&self) -> Mods {
        let raw = unsafe { sys::ghostty_key_event_get_mods(self.raw.as_ptr()) };
        Mods::from_bits_retain(raw)
    }

    /// Sets the modifiers consumed by text generation.
    pub fn set_consumed_mods(&mut self, value: Mods) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_key_event_set_consumed_mods(self.raw.as_ptr(), value.bits())
        })
    }

    /// Returns the modifiers consumed by text generation.
    pub fn consumed_mods(&self) -> Mods {
        let raw = unsafe { sys::ghostty_key_event_get_consumed_mods(self.raw.as_ptr()) };
        Mods::from_bits_retain(raw)
    }

    /// Sets the composition status.
    pub fn set_composing(&mut self, value: bool) -> Result<()> {
        Error::check(unsafe { sys::ghostty_key_event_set_composing(self.raw.as_ptr(), value) })
    }

    /// Returns the composition status.
    pub fn composing(&self) -> bool {
        unsafe { sys::ghostty_key_event_get_composing(self.raw.as_ptr()) }
    }

    /// Sets the unshifted Unicode codepoint.
    pub fn set_unshifted_codepoint(&mut self, value: u32) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_key_event_set_unshifted_codepoint(self.raw.as_ptr(), value)
        })
    }

    /// Returns the unshifted Unicode codepoint.
    pub fn unshifted_codepoint(&self) -> u32 {
        unsafe { sys::ghostty_key_event_get_unshifted_codepoint(self.raw.as_ptr()) }
    }

    /// Sets layout text before Ctrl/Meta transformations; use empty text for control keys.
    ///
    /// The event owns a copy so the C library's borrowed pointer remains valid.
    pub fn set_utf8(&mut self, text: &str) -> Result<()> {
        let text = text.to_owned();
        Error::check(unsafe {
            sys::ghostty_key_event_set_utf8(self.raw.as_ptr(), ffi::ghostty_string(text.as_bytes()))
        })?;
        self.utf8 = text;
        Ok(())
    }

    /// Returns the layout text.
    pub fn utf8(&self) -> &str {
        let raw = unsafe { sys::ghostty_key_event_get_utf8(self.raw.as_ptr()) };
        // SAFETY: only set_utf8 changes this pointer, and its owned String lives in self.
        let bytes = unsafe { ffi::string_bytes(raw) };
        // SAFETY: set_utf8 only accepts valid UTF-8, and C does not change the bytes.
        unsafe { core::str::from_utf8_unchecked(bytes) }
    }
}

impl Drop for KeyEvent {
    fn drop(&mut self) {
        // The String remains alive while the C event is freed.
        unsafe { sys::ghostty_key_event_free(self.raw.as_ptr()) };
    }
}

/// An owned keyboard encoder with terminal modes and protocol options.
pub struct KeyEncoder {
    raw: NonNull<sys::GhosttyKeyEncoderImpl>,
}

// SAFETY: the encoder owns independent state without thread affinity or terminal borrows.
unsafe impl Send for KeyEncoder {}

impl KeyEncoder {
    /// Creates an encoder with library defaults.
    pub fn new() -> Result<Self> {
        let mut raw = ptr::null_mut();
        Error::check(unsafe { sys::ghostty_key_encoder_new(ptr::null(), &mut raw) })?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
        })
    }
    /// Sets cursor-key application mode.
    pub fn set_cursor_key_application(&mut self, value: bool) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_key_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_KEY_ENCODER_OPT_CURSOR_KEY_APPLICATION,
                (&raw const value).cast(),
            )
        })
    }

    /// Sets keypad application mode.
    pub fn set_keypad_key_application(&mut self, value: bool) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_key_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_KEY_ENCODER_OPT_KEYPAD_KEY_APPLICATION,
                (&raw const value).cast(),
            )
        })
    }

    /// Sets Num Lock keypad handling.
    pub fn set_ignore_keypad_with_numlock(&mut self, value: bool) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_key_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_KEY_ENCODER_OPT_IGNORE_KEYPAD_WITH_NUMLOCK,
                (&raw const value).cast(),
            )
        })
    }

    /// Sets Alt escape prefix mode.
    pub fn set_alt_esc_prefix(&mut self, value: bool) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_key_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_KEY_ENCODER_OPT_ALT_ESC_PREFIX,
                (&raw const value).cast(),
            )
        })
    }

    /// Sets modifyOtherKeys mode 2.
    pub fn set_modify_other_keys_state_2(&mut self, value: bool) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_key_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_KEY_ENCODER_OPT_MODIFY_OTHER_KEYS_STATE_2,
                (&raw const value).cast(),
            )
        })
    }

    /// Sets Kitty keyboard protocol flags.
    pub fn set_kitty_flags(&mut self, value: KittyKeyFlags) -> Result<()> {
        let value = value.bits();
        Error::check(unsafe {
            sys::ghostty_key_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_KEY_ENCODER_OPT_KITTY_FLAGS,
                (&raw const value).cast(),
            )
        })
    }

    /// Sets macOS Option-as-Alt behavior.
    pub fn set_macos_option_as_alt(&mut self, value: OptionAsAlt) -> Result<()> {
        let value = value.to_raw();
        Error::check(unsafe {
            sys::ghostty_key_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_KEY_ENCODER_OPT_MACOS_OPTION_AS_ALT,
                (&raw const value).cast(),
            )
        })
    }

    /// Sets backarrow mode (true sends BS instead of DEL).
    pub fn set_backarrow_key_mode(&mut self, value: bool) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_key_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_KEY_ENCODER_OPT_BACKARROW_KEY_MODE,
                (&raw const value).cast(),
            )
        })
    }

    /// Copies current terminal modes; resets macOS Option-as-Alt to false.
    pub fn set_from_terminal(&mut self, terminal: &Terminal) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_key_encoder_set_from_terminal(self.raw.as_ptr(), terminal.as_raw())
        })
    }

    /// Encodes an event; events not reported produce an empty vector.
    pub fn encode(&self, event: &KeyEvent) -> Result<Vec<u8>> {
        ffi::encode_vec(|buf, cap, written| unsafe {
            sys::ghostty_key_encoder_encode(
                self.raw.as_ptr(),
                event.raw.as_ptr(),
                buf,
                cap,
                written,
            )
        })
    }
}

impl Drop for KeyEncoder {
    fn drop(&mut self) {
        unsafe { sys::ghostty_key_encoder_free(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_ctrl_c_kitty() -> Result<()> {
        let mut encoder = KeyEncoder::new()?;
        encoder.set_kitty_flags(KittyKeyFlags::all())?;
        let mut event = KeyEvent::new()?;
        event.set_action(KeyAction::Press)?;
        event.set_key(Key::C)?;
        event.set_mods(Mods::CTRL)?;
        event.set_unshifted_codepoint('c' as u32)?;
        assert_eq!(encoder.encode(&event)?, b"\x1b[99;5u");
        Ok(())
    }

    #[test]
    fn key_owned_text_and_terminal_modes() -> Result<()> {
        let mut encoder = KeyEncoder::new()?;
        let mut terminal = Terminal::new(80, 24)?;
        terminal.vt_write(b"\x1b[?1h");
        encoder.set_from_terminal(&terminal)?;
        let mut event = KeyEvent::new()?;
        event.set_action(KeyAction::Press)?;
        event.set_key(Key::ArrowUp)?;
        assert_eq!(encoder.encode(&event)?, b"\x1bOA");
        event.set_key(Key::A)?;
        event.set_utf8(&String::from("a"))?;
        assert_eq!(event.utf8(), "a");
        assert_eq!(encoder.encode(&event)?, b"a");
        event.set_utf8("")?;
        event.set_key(Key::ShiftLeft)?;
        assert!(encoder.encode(&event)?.is_empty());
        Ok(())
    }
}
