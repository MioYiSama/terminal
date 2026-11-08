//! Translate GPUI keyboard and mouse input into libghostty's vocabulary.

use ghostty_sys::*;
use gpui::{Keystroke, Modifiers, NavigationDirection};

use crate::vt::{KeyAction, KeyInput, MouseButton};

/// Map GPUI's logical key name to the physical key libghostty expects. GPUI
/// reports the US-layout character for letter/digit/punctuation keys.
fn ghostty_key(key: &str) -> GhosttyKey {
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.clone().next()) {
        return match c {
            'a'..='z' => GHOSTTY_KEY_A + (c as GhosttyKey - 'a' as GhosttyKey),
            '0'..='9' => GHOSTTY_KEY_DIGIT_0 + (c as GhosttyKey - '0' as GhosttyKey),
            '`' => GHOSTTY_KEY_BACKQUOTE,
            '\\' => GHOSTTY_KEY_BACKSLASH,
            '[' => GHOSTTY_KEY_BRACKET_LEFT,
            ']' => GHOSTTY_KEY_BRACKET_RIGHT,
            ',' => GHOSTTY_KEY_COMMA,
            '=' => GHOSTTY_KEY_EQUAL,
            '-' => GHOSTTY_KEY_MINUS,
            '.' => GHOSTTY_KEY_PERIOD,
            '\'' => GHOSTTY_KEY_QUOTE,
            ';' => GHOSTTY_KEY_SEMICOLON,
            '/' => GHOSTTY_KEY_SLASH,
            ' ' => GHOSTTY_KEY_SPACE,
            _ => GHOSTTY_KEY_UNIDENTIFIED,
        };
    }
    match key {
        "space" => GHOSTTY_KEY_SPACE,
        "enter" => GHOSTTY_KEY_ENTER,
        "tab" => GHOSTTY_KEY_TAB,
        "backspace" => GHOSTTY_KEY_BACKSPACE,
        "escape" => GHOSTTY_KEY_ESCAPE,
        "delete" => GHOSTTY_KEY_DELETE,
        "insert" => GHOSTTY_KEY_INSERT,
        "home" => GHOSTTY_KEY_HOME,
        "end" => GHOSTTY_KEY_END,
        "pageup" => GHOSTTY_KEY_PAGE_UP,
        "pagedown" => GHOSTTY_KEY_PAGE_DOWN,
        "up" => GHOSTTY_KEY_ARROW_UP,
        "down" => GHOSTTY_KEY_ARROW_DOWN,
        "left" => GHOSTTY_KEY_ARROW_LEFT,
        "right" => GHOSTTY_KEY_ARROW_RIGHT,
        "f1" => GHOSTTY_KEY_F1,
        "f2" => GHOSTTY_KEY_F2,
        "f3" => GHOSTTY_KEY_F3,
        "f4" => GHOSTTY_KEY_F4,
        "f5" => GHOSTTY_KEY_F5,
        "f6" => GHOSTTY_KEY_F6,
        "f7" => GHOSTTY_KEY_F7,
        "f8" => GHOSTTY_KEY_F8,
        "f9" => GHOSTTY_KEY_F9,
        "f10" => GHOSTTY_KEY_F10,
        "f11" => GHOSTTY_KEY_F11,
        "f12" => GHOSTTY_KEY_F12,
        "f13" => GHOSTTY_KEY_F13,
        "f14" => GHOSTTY_KEY_F14,
        "f15" => GHOSTTY_KEY_F15,
        "f16" => GHOSTTY_KEY_F16,
        "f17" => GHOSTTY_KEY_F17,
        "f18" => GHOSTTY_KEY_F18,
        "f19" => GHOSTTY_KEY_F19,
        "f20" => GHOSTTY_KEY_F20,
        "f21" => GHOSTTY_KEY_F21,
        "f22" => GHOSTTY_KEY_F22,
        "f23" => GHOSTTY_KEY_F23,
        "f24" => GHOSTTY_KEY_F24,
        "f25" => GHOSTTY_KEY_F25,
        _ => GHOSTTY_KEY_UNIDENTIFIED,
    }
}

pub fn ghostty_mods(modifiers: &Modifiers) -> GhosttyMods {
    let mut mods = 0;
    if modifiers.shift {
        mods |= GHOSTTY_MODS_SHIFT;
    }
    if modifiers.control {
        mods |= GHOSTTY_MODS_CTRL;
    }
    if modifiers.alt {
        mods |= GHOSTTY_MODS_ALT;
    }
    if modifiers.platform {
        mods |= GHOSTTY_MODS_SUPER;
    }
    mods as GhosttyMods
}

/// Keys that never produce text and always go through the key encoder.
fn is_function_key(key: &str) -> bool {
    matches!(
        key,
        "enter"
            | "tab"
            | "backspace"
            | "escape"
            | "delete"
            | "insert"
            | "home"
            | "end"
            | "pageup"
            | "pagedown"
            | "up"
            | "down"
            | "left"
            | "right"
    ) || (key.len() >= 2
        && key.starts_with('f')
        && key[1..].bytes().all(|b| b.is_ascii_digit()))
}

/// The text a keystroke would type, excluding control characters.
fn printable(text: &str) -> Option<&str> {
    (!text.is_empty() && !text.chars().any(char::is_control)).then_some(text)
}

/// Decide whether a key press is encoded by libghostty (`Some`) or left to
/// the platform text input system (`None`), which delivers typed text and
/// IME commits through the input handler.
pub fn encodable<'a>(keystroke: &'a Keystroke, owned_text: &'a mut String) -> Option<KeyInput<'a>> {
    let m = &keystroke.modifiers;
    // Cmd / Super / Win shortcuts belong to the app and the OS.
    if m.platform {
        return None;
    }

    let key = keystroke.key.as_str();
    let produced = keystroke.key_char.as_deref().and_then(printable);
    let function = is_function_key(key);

    let encode = if function {
        true
    } else if m.control && m.alt && produced.is_some_and(|text| text != key) {
        // AltGr on Windows/Linux is reported as Ctrl+Alt; the character it
        // produces is text, not a control sequence.
        false
    } else if m.control {
        true
    } else if m.alt {
        // macOS Option composes characters (é, ß, …) through the input
        // system. Elsewhere, and for Option keys that produce nothing, Alt
        // acts as Meta.
        !cfg!(target_os = "macos") || produced.is_none()
    } else {
        // Plain and shifted keys type text via the input handler.
        false
    };
    if !encode {
        return None;
    }

    let unshifted = unshifted(key);

    // The encoder wants the text before Ctrl/Alt transformations: the
    // shifted character for Shift, the plain key otherwise.
    let text: &str = if function {
        ""
    } else if m.alt || m.control {
        owned_text.clear();
        match unshifted {
            Some(c) if m.shift => match shifted_symbol(c) {
                Some(symbol) => owned_text.push(symbol),
                None => owned_text.extend(c.to_uppercase()),
            },
            Some(c) => owned_text.push(c),
            None => {}
        }
        let owned_text: &'a String = owned_text;
        owned_text.as_str()
    } else {
        produced.unwrap_or("")
    };

    Some(KeyInput {
        action: KeyAction::Press,
        key: ghostty_key(key),
        mods: ghostty_mods(m),
        text,
        unshifted,
    })
}

/// A key that typed `text` through the platform text input system, for
/// programs that want text keys as escape codes too.
pub fn text_key<'a>(keystroke: &Keystroke, text: &'a str) -> KeyInput<'a> {
    let key = keystroke.key.as_str();
    KeyInput {
        action: KeyAction::Press,
        key: ghostty_key(key),
        mods: ghostty_mods(&keystroke.modifiers),
        text,
        unshifted: unshifted(key),
    }
}

/// The character GPUI's key name stands for, if it is one.
fn unshifted(key: &str) -> Option<char> {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ if key == "space" => Some(' '),
        _ => None,
    }
}

/// The symbol Shift turns US-layout digit or punctuation key `c` into, as
/// GPUI names those keys by their unshifted character. Letters are
/// uppercased instead.
fn shifted_symbol(c: char) -> Option<char> {
    Some(match c {
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '`' => '~',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        _ => return None,
    })
}

/// The arrow key `up` or down, unmodified, as alternate scroll sends it.
pub fn arrow(up: bool) -> KeyInput<'static> {
    KeyInput {
        action: KeyAction::Press,
        key: if up {
            GHOSTTY_KEY_ARROW_UP
        } else {
            GHOSTTY_KEY_ARROW_DOWN
        },
        mods: 0,
        text: "",
        unshifted: None,
    }
}

pub fn mouse_button(button: gpui::MouseButton) -> MouseButton {
    match button {
        gpui::MouseButton::Left => MouseButton::Left,
        gpui::MouseButton::Right => MouseButton::Right,
        gpui::MouseButton::Middle => MouseButton::Middle,
        gpui::MouseButton::Navigate(NavigationDirection::Back) => MouseButton::Eight,
        gpui::MouseButton::Navigate(NavigationDirection::Forward) => MouseButton::Nine,
    }
}
