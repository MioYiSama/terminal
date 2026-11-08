//! Key, mouse and focus input encoders.

use core::ffi::c_void;
use core::{mem, ptr};

use ghostty_sys::*;

use super::Terminal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Press,
    Release,
    Repeat,
}

impl KeyAction {
    fn raw(self) -> GhosttyKeyAction {
        match self {
            Self::Press => GHOSTTY_KEY_ACTION_PRESS,
            Self::Release => GHOSTTY_KEY_ACTION_RELEASE,
            Self::Repeat => GHOSTTY_KEY_ACTION_REPEAT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

impl MouseAction {
    fn raw(self) -> GhosttyMouseAction {
        match self {
            Self::Press => GHOSTTY_MOUSE_ACTION_PRESS,
            Self::Release => GHOSTTY_MOUSE_ACTION_RELEASE,
            Self::Motion => GHOSTTY_MOUSE_ACTION_MOTION,
        }
    }
}

/// Mouse buttons as the terminal numbers them. `Four` and `Five` are the
/// wheel; `Eight` and `Nine` are the back and forward side buttons, as X11
/// and xterm number them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Four,
    Five,
    Eight,
    Nine,
}

impl MouseButton {
    fn raw(self) -> GhosttyMouseButton {
        match self {
            Self::Left => GHOSTTY_MOUSE_BUTTON_LEFT,
            Self::Right => GHOSTTY_MOUSE_BUTTON_RIGHT,
            Self::Middle => GHOSTTY_MOUSE_BUTTON_MIDDLE,
            Self::Four => GHOSTTY_MOUSE_BUTTON_FOUR,
            Self::Five => GHOSTTY_MOUSE_BUTTON_FIVE,
            Self::Eight => GHOSTTY_MOUSE_BUTTON_EIGHT,
            Self::Nine => GHOSTTY_MOUSE_BUTTON_NINE,
        }
    }
}

pub struct KeyEncoder {
    encoder: GhosttyKeyEncoder,
    event: GhosttyKeyEvent,
}

pub struct KeyInput<'a> {
    pub action: KeyAction,
    /// Physical key (US layout), as translated by `keys`.
    pub key: GhosttyKey,
    pub mods: GhosttyMods,
    /// Text the key produces without Ctrl/Meta applied; empty for keys that
    /// do not produce text.
    pub text: &'a str,
    pub unshifted: Option<char>,
}

impl KeyEncoder {
    pub fn new() -> Self {
        let mut encoder = ptr::null_mut();
        let mut event = ptr::null_mut();
        unsafe {
            assert_eq!(
                ghostty_key_encoder_new(ptr::null(), &mut encoder),
                GHOSTTY_SUCCESS
            );
            assert_eq!(
                ghostty_key_event_new(ptr::null(), &mut event),
                GHOSTTY_SUCCESS
            );
        }
        Self { encoder, event }
    }

    /// Encode a key using the terminal's current keyboard modes.
    pub fn encode(&mut self, terminal: &Terminal, input: &KeyInput, out: &mut Vec<u8>) {
        unsafe {
            ghostty_key_encoder_set_from_terminal(self.encoder, terminal.raw);
            // Keys reach the encoder only when Alt should act as Meta (Option
            // keys that compose characters go through the text input path),
            // so Alt always means Alt here. `set_from_terminal` resets this.
            let option_as_alt: GhosttyOptionAsAlt = GHOSTTY_OPTION_AS_ALT_TRUE;
            ghostty_key_encoder_set(
                self.encoder,
                GHOSTTY_KEY_ENCODER_OPT_MACOS_OPTION_AS_ALT,
                &option_as_alt as *const _ as *const c_void,
            );
            ghostty_key_event_set_action(self.event, input.action.raw());
            ghostty_key_event_set_key(self.event, input.key);
            ghostty_key_event_set_mods(self.event, input.mods);
            ghostty_key_event_set_consumed_mods(self.event, 0);
            ghostty_key_event_set_composing(self.event, false);
            ghostty_key_event_set_utf8(
                self.event,
                GhosttyString {
                    ptr: input.text.as_ptr(),
                    len: input.text.len(),
                },
            );
            ghostty_key_event_set_unshifted_codepoint(
                self.event,
                input.unshifted.map_or(0, |c| c as u32),
            );
        }
        encode_into(out, |buf, len, written| unsafe {
            ghostty_key_encoder_encode(self.encoder, self.event, buf, len, written)
        });
        // The event borrows `text`; clear it so no dangling pointer remains.
        unsafe {
            ghostty_key_event_set_utf8(self.event, GhosttyString::default());
        }
    }
}

impl Drop for KeyEncoder {
    fn drop(&mut self) {
        unsafe {
            ghostty_key_event_free(self.event);
            ghostty_key_encoder_free(self.encoder);
        }
    }
}

/// One mouse event to report to the program.
pub struct MouseInput {
    pub action: MouseAction,
    /// `None` for motion without a button.
    pub button: Option<MouseButton>,
    pub any_button_pressed: bool,
    pub mods: GhosttyMods,
    /// Surface position in device pixels.
    pub x: f32,
    pub y: f32,
}

pub struct MouseEncoder {
    encoder: GhosttyMouseEncoder,
    event: GhosttyMouseEvent,
}

/// Geometry for mapping surface pixels to cells, in device pixels.
#[derive(Clone, Copy, Debug)]
pub struct SurfaceSize {
    pub width: u32,
    pub height: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub padding: u32,
}

impl MouseEncoder {
    pub fn new() -> Self {
        let mut encoder = ptr::null_mut();
        let mut event = ptr::null_mut();
        unsafe {
            assert_eq!(
                ghostty_mouse_encoder_new(ptr::null(), &mut encoder),
                GHOSTTY_SUCCESS
            );
            assert_eq!(
                ghostty_mouse_event_new(ptr::null(), &mut event),
                GHOSTTY_SUCCESS
            );
            // Only report motion when the pointer enters a new cell.
            let track_last_cell = true;
            ghostty_mouse_encoder_set(
                encoder,
                GHOSTTY_MOUSE_ENCODER_OPT_TRACK_LAST_CELL,
                &track_last_cell as *const bool as *const c_void,
            );
        }
        Self { encoder, event }
    }

    pub fn encode(
        &mut self,
        terminal: &Terminal,
        size: SurfaceSize,
        input: &MouseInput,
        out: &mut Vec<u8>,
    ) {
        let size = GhosttyMouseEncoderSize {
            size: mem::size_of::<GhosttyMouseEncoderSize>(),
            screen_width: size.width,
            screen_height: size.height,
            cell_width: size.cell_width.max(1),
            cell_height: size.cell_height.max(1),
            padding_top: size.padding,
            padding_bottom: size.padding,
            padding_right: size.padding,
            padding_left: size.padding,
        };
        unsafe {
            ghostty_mouse_encoder_set_from_terminal(self.encoder, terminal.raw);
            ghostty_mouse_encoder_set(
                self.encoder,
                GHOSTTY_MOUSE_ENCODER_OPT_SIZE,
                &size as *const _ as *const c_void,
            );
            ghostty_mouse_encoder_set(
                self.encoder,
                GHOSTTY_MOUSE_ENCODER_OPT_ANY_BUTTON_PRESSED,
                &input.any_button_pressed as *const bool as *const c_void,
            );
            ghostty_mouse_event_set_action(self.event, input.action.raw());
            match input.button {
                Some(button) => ghostty_mouse_event_set_button(self.event, button.raw()),
                None => ghostty_mouse_event_clear_button(self.event),
            };
            ghostty_mouse_event_set_mods(self.event, input.mods);
            ghostty_mouse_event_set_position(
                self.event,
                GhosttyMousePosition {
                    x: input.x,
                    y: input.y,
                },
            );
        }
        encode_into(out, |buf, len, written| unsafe {
            ghostty_mouse_encoder_encode(self.encoder, self.event, buf, len, written)
        });
    }
}

impl Drop for MouseEncoder {
    fn drop(&mut self) {
        unsafe {
            ghostty_mouse_event_free(self.event);
            ghostty_mouse_encoder_free(self.encoder);
        }
    }
}

/// Encode a focus gained/lost report.
pub fn encode_focus(focused: bool, out: &mut Vec<u8>) {
    let event = if focused {
        GHOSTTY_FOCUS_GAINED
    } else {
        GHOSTTY_FOCUS_LOST
    };
    encode_into(out, |buf, len, written| unsafe {
        ghostty_focus_encode(event, buf, len, written)
    });
}

/// Run an encoder that follows the "buf, len, out_written" convention,
/// growing the buffer if it reports `GHOSTTY_OUT_OF_SPACE`.
fn encode_into(
    out: &mut Vec<u8>,
    mut encode: impl FnMut(*mut u8, usize, *mut usize) -> GhosttyResult,
) {
    let mut buf = [0u8; 128];
    let mut written = 0usize;
    match encode(buf.as_mut_ptr(), buf.len(), &mut written) {
        GHOSTTY_SUCCESS => out.extend_from_slice(&buf[..written]),
        GHOSTTY_OUT_OF_SPACE => {
            let start = out.len();
            out.resize(start + written, 0);
            let mut written2 = 0usize;
            if encode(out[start..].as_mut_ptr(), written, &mut written2) == GHOSTTY_SUCCESS {
                out.truncate(start + written2);
            } else {
                out.truncate(start);
            }
        }
        _ => {}
    }
}
