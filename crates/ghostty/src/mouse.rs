//! Mouse events and encoding in X10, UTF-8, SGR, URxvt and SGR-Pixels formats.

use crate::{Error, Result, ffi, key::Mods, terminal::Terminal};
use core::ptr::{self, NonNull};
use ghostty_sys as sys;

ffi::c_enum! {
    /// Mouse event action.
    pub enum MouseAction {
        /// Press.
        Press = sys::GHOSTTY_MOUSE_ACTION_PRESS,
        /// Release.
        Release = sys::GHOSTTY_MOUSE_ACTION_RELEASE,
        /// Motion.
        Motion = sys::GHOSTTY_MOUSE_ACTION_MOTION,
    }
}

ffi::c_enum! {
    /// Mouse button identity.
    pub enum MouseButton {
        /// Unknown.
        Unknown = sys::GHOSTTY_MOUSE_BUTTON_UNKNOWN,
        /// Left.
        Left = sys::GHOSTTY_MOUSE_BUTTON_LEFT,
        /// Right.
        Right = sys::GHOSTTY_MOUSE_BUTTON_RIGHT,
        /// Middle.
        Middle = sys::GHOSTTY_MOUSE_BUTTON_MIDDLE,
        /// Four.
        Four = sys::GHOSTTY_MOUSE_BUTTON_FOUR,
        /// Five.
        Five = sys::GHOSTTY_MOUSE_BUTTON_FIVE,
        /// Six.
        Six = sys::GHOSTTY_MOUSE_BUTTON_SIX,
        /// Seven.
        Seven = sys::GHOSTTY_MOUSE_BUTTON_SEVEN,
        /// Eight.
        Eight = sys::GHOSTTY_MOUSE_BUTTON_EIGHT,
        /// Nine.
        Nine = sys::GHOSTTY_MOUSE_BUTTON_NINE,
        /// Ten.
        Ten = sys::GHOSTTY_MOUSE_BUTTON_TEN,
        /// Eleven.
        Eleven = sys::GHOSTTY_MOUSE_BUTTON_ELEVEN,
    }
}

ffi::c_enum! {
    /// Which mouse events are reported.
    pub enum MouseTrackingMode {
        /// None.
        None = sys::GHOSTTY_MOUSE_TRACKING_NONE,
        /// X10.
        X10 = sys::GHOSTTY_MOUSE_TRACKING_X10,
        /// Normal.
        Normal = sys::GHOSTTY_MOUSE_TRACKING_NORMAL,
        /// Button.
        Button = sys::GHOSTTY_MOUSE_TRACKING_BUTTON,
        /// Any.
        Any = sys::GHOSTTY_MOUSE_TRACKING_ANY,
    }
}

ffi::c_enum! {
    /// Mouse reporting wire format.
    pub enum MouseFormat {
        /// X10.
        X10 = sys::GHOSTTY_MOUSE_FORMAT_X10,
        /// Utf8.
        Utf8 = sys::GHOSTTY_MOUSE_FORMAT_UTF8,
        /// Sgr.
        Sgr = sys::GHOSTTY_MOUSE_FORMAT_SGR,
        /// Urxvt.
        Urxvt = sys::GHOSTTY_MOUSE_FORMAT_URXVT,
        /// SgrPixels.
        SgrPixels = sys::GHOSTTY_MOUSE_FORMAT_SGR_PIXELS,
    }
}

ffi::c_enum! {
    /// Host pointer shape using W3C cursor names.
    pub enum MouseShape {
        /// Default.
        Default = sys::GHOSTTY_MOUSE_SHAPE_DEFAULT,
        /// ContextMenu.
        ContextMenu = sys::GHOSTTY_MOUSE_SHAPE_CONTEXT_MENU,
        /// Help.
        Help = sys::GHOSTTY_MOUSE_SHAPE_HELP,
        /// Pointer.
        Pointer = sys::GHOSTTY_MOUSE_SHAPE_POINTER,
        /// Progress.
        Progress = sys::GHOSTTY_MOUSE_SHAPE_PROGRESS,
        /// Wait.
        Wait = sys::GHOSTTY_MOUSE_SHAPE_WAIT,
        /// Cell.
        Cell = sys::GHOSTTY_MOUSE_SHAPE_CELL,
        /// Crosshair.
        Crosshair = sys::GHOSTTY_MOUSE_SHAPE_CROSSHAIR,
        /// Text.
        Text = sys::GHOSTTY_MOUSE_SHAPE_TEXT,
        /// VerticalText.
        VerticalText = sys::GHOSTTY_MOUSE_SHAPE_VERTICAL_TEXT,
        /// Alias.
        Alias = sys::GHOSTTY_MOUSE_SHAPE_ALIAS,
        /// Copy.
        Copy = sys::GHOSTTY_MOUSE_SHAPE_COPY,
        /// Move.
        Move = sys::GHOSTTY_MOUSE_SHAPE_MOVE,
        /// NoDrop.
        NoDrop = sys::GHOSTTY_MOUSE_SHAPE_NO_DROP,
        /// NotAllowed.
        NotAllowed = sys::GHOSTTY_MOUSE_SHAPE_NOT_ALLOWED,
        /// Grab.
        Grab = sys::GHOSTTY_MOUSE_SHAPE_GRAB,
        /// Grabbing.
        Grabbing = sys::GHOSTTY_MOUSE_SHAPE_GRABBING,
        /// AllScroll.
        AllScroll = sys::GHOSTTY_MOUSE_SHAPE_ALL_SCROLL,
        /// ColResize.
        ColResize = sys::GHOSTTY_MOUSE_SHAPE_COL_RESIZE,
        /// RowResize.
        RowResize = sys::GHOSTTY_MOUSE_SHAPE_ROW_RESIZE,
        /// NResize.
        NResize = sys::GHOSTTY_MOUSE_SHAPE_N_RESIZE,
        /// EResize.
        EResize = sys::GHOSTTY_MOUSE_SHAPE_E_RESIZE,
        /// SResize.
        SResize = sys::GHOSTTY_MOUSE_SHAPE_S_RESIZE,
        /// WResize.
        WResize = sys::GHOSTTY_MOUSE_SHAPE_W_RESIZE,
        /// NeResize.
        NeResize = sys::GHOSTTY_MOUSE_SHAPE_NE_RESIZE,
        /// NwResize.
        NwResize = sys::GHOSTTY_MOUSE_SHAPE_NW_RESIZE,
        /// SeResize.
        SeResize = sys::GHOSTTY_MOUSE_SHAPE_SE_RESIZE,
        /// SwResize.
        SwResize = sys::GHOSTTY_MOUSE_SHAPE_SW_RESIZE,
        /// EwResize.
        EwResize = sys::GHOSTTY_MOUSE_SHAPE_EW_RESIZE,
        /// NsResize.
        NsResize = sys::GHOSTTY_MOUSE_SHAPE_NS_RESIZE,
        /// NeswResize.
        NeswResize = sys::GHOSTTY_MOUSE_SHAPE_NESW_RESIZE,
        /// NwseResize.
        NwseResize = sys::GHOSTTY_MOUSE_SHAPE_NWSE_RESIZE,
        /// ZoomIn.
        ZoomIn = sys::GHOSTTY_MOUSE_SHAPE_ZOOM_IN,
        /// ZoomOut.
        ZoomOut = sys::GHOSTTY_MOUSE_SHAPE_ZOOM_OUT,
    }
}

/// Mouse position in surface-space pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MousePosition {
    /// Horizontal pixel coordinate.
    pub x: f32,
    /// Vertical pixel coordinate.
    pub y: f32,
}

impl From<MousePosition> for sys::GhosttyMousePosition {
    fn from(value: MousePosition) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

impl From<sys::GhosttyMousePosition> for MousePosition {
    fn from(value: sys::GhosttyMousePosition) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

/// Rendered terminal geometry for mapping pixel coordinates to cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEncoderSize {
    /// Full surface width in pixels.
    pub screen_width: u32,
    /// Full surface height in pixels.
    pub screen_height: u32,
    /// Cell width in pixels; must be nonzero.
    pub cell_width: u32,
    /// Cell height in pixels; must be nonzero.
    pub cell_height: u32,
    /// Top padding in pixels.
    pub padding_top: u32,
    /// Bottom padding in pixels.
    pub padding_bottom: u32,
    /// Right padding in pixels.
    pub padding_right: u32,
    /// Left padding in pixels.
    pub padding_left: u32,
}

impl Default for MouseEncoderSize {
    fn default() -> Self {
        Self {
            screen_width: 1,
            screen_height: 1,
            cell_width: 1,
            cell_height: 1,
            padding_top: 0,
            padding_bottom: 0,
            padding_right: 0,
            padding_left: 0,
        }
    }
}

impl MouseEncoderSize {
    fn to_raw(self) -> sys::GhosttyMouseEncoderSize {
        // SAFETY: this C struct starts with its size_t size field.
        let mut raw: sys::GhosttyMouseEncoderSize = unsafe { ffi::sized() };
        raw.screen_width = self.screen_width;
        raw.screen_height = self.screen_height;
        raw.cell_width = self.cell_width;
        raw.cell_height = self.cell_height;
        raw.padding_top = self.padding_top;
        raw.padding_bottom = self.padding_bottom;
        raw.padding_right = self.padding_right;
        raw.padding_left = self.padding_left;
        raw
    }
}

/// An owned normalized mouse event.
pub struct MouseEvent {
    raw: NonNull<sys::GhosttyMouseEventImpl>,
}

// SAFETY: events own independent state without thread affinity.
unsafe impl Send for MouseEvent {}

impl MouseEvent {
    /// Creates an event with no button and the library defaults.
    pub fn new() -> Result<Self> {
        let mut raw = ptr::null_mut();
        Error::check(unsafe { sys::ghostty_mouse_event_new(ptr::null(), &mut raw) })?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
        })
    }
    /// Sets the event action.
    pub fn set_action(&mut self, value: MouseAction) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_mouse_event_set_action(self.raw.as_ptr(), value.to_raw())
        })
    }

    /// Returns the event action.
    pub fn action(&self) -> MouseAction {
        let raw = unsafe { sys::ghostty_mouse_event_get_action(self.raw.as_ptr()) };
        MouseAction::from_lib(raw)
    }

    /// Sets the keyboard modifiers.
    pub fn set_mods(&mut self, value: Mods) -> Result<()> {
        Error::check(unsafe { sys::ghostty_mouse_event_set_mods(self.raw.as_ptr(), value.bits()) })
    }

    /// Returns the keyboard modifiers.
    pub fn mods(&self) -> Mods {
        let raw = unsafe { sys::ghostty_mouse_event_get_mods(self.raw.as_ptr()) };
        Mods::from_bits_retain(raw)
    }

    /// Sets the surface-space pixel position.
    pub fn set_position(&mut self, value: MousePosition) -> Result<()> {
        // C stores floats unchecked; its encoder later converts them to integers.
        if !value.x.is_finite() || !value.y.is_finite() {
            return Err(Error::InvalidValue);
        }
        Error::check(unsafe {
            sys::ghostty_mouse_event_set_position(self.raw.as_ptr(), value.into())
        })
    }

    /// Returns the surface-space pixel position.
    pub fn position(&self) -> MousePosition {
        let raw = unsafe { sys::ghostty_mouse_event_get_position(self.raw.as_ptr()) };
        raw.into()
    }

    /// Sets a concrete button, or clears it for motion without a held button.
    pub fn set_button(&mut self, button: Option<MouseButton>) -> Result<()> {
        Error::check(unsafe {
            match button {
                Some(button) => {
                    sys::ghostty_mouse_event_set_button(self.raw.as_ptr(), button.to_raw())
                }
                None => sys::ghostty_mouse_event_clear_button(self.raw.as_ptr()),
            }
        })
    }

    /// Returns the button, or None when no button is held.
    pub fn button(&self) -> Result<Option<MouseButton>> {
        let mut button = 0;
        let code = unsafe { sys::ghostty_mouse_event_get_button(self.raw.as_ptr(), &mut button) };
        if code == sys::GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        Error::check(code)?;
        Ok(Some(MouseButton::from_lib(button)))
    }
}

impl Drop for MouseEvent {
    fn drop(&mut self) {
        unsafe { sys::ghostty_mouse_event_free(self.raw.as_ptr()) };
    }
}

/// An owned mouse encoder, including motion deduplication state.
pub struct MouseEncoder {
    raw: NonNull<sys::GhosttyMouseEncoderImpl>,
    size: MouseEncoderSize,
}

// SAFETY: encoders own independent state with no thread affinity or terminal borrow.
unsafe impl Send for MouseEncoder {}

impl MouseEncoder {
    /// Creates an encoder with mouse reporting disabled.
    pub fn new() -> Result<Self> {
        let mut raw = ptr::null_mut();
        Error::check(unsafe { sys::ghostty_mouse_encoder_new(ptr::null(), &mut raw) })?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
            size: MouseEncoderSize::default(),
        })
    }
    /// Sets mouse tracking mode.
    pub fn set_tracking_mode(&mut self, value: MouseTrackingMode) -> Result<()> {
        let value = value.to_raw();
        Error::check(unsafe {
            sys::ghostty_mouse_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_MOUSE_ENCODER_OPT_EVENT,
                (&raw const value).cast(),
            )
        })
    }

    /// Sets wire format.
    pub fn set_format(&mut self, value: MouseFormat) -> Result<()> {
        let value = value.to_raw();
        Error::check(unsafe {
            sys::ghostty_mouse_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_MOUSE_ENCODER_OPT_FORMAT,
                (&raw const value).cast(),
            )
        })
    }

    /// Sets geometry; cell dimensions must be nonzero and the grid must fit 16-bit coordinates.
    pub fn set_size(&mut self, value: MouseEncoderSize) -> Result<()> {
        // C adds padding without overflow checks and converts grid dimensions to u16.
        let horizontal = value
            .padding_left
            .checked_add(value.padding_right)
            .ok_or(Error::InvalidValue)?;
        let vertical = value
            .padding_top
            .checked_add(value.padding_bottom)
            .ok_or(Error::InvalidValue)?;
        if value.cell_width == 0
            || value.cell_height == 0
            || value.screen_width.saturating_sub(horizontal) as f32 / value.cell_width as f32
                >= 65536.0
            || value.screen_height.saturating_sub(vertical) as f32 / value.cell_height as f32
                >= 65536.0
        {
            return Err(Error::InvalidValue);
        }
        let raw = value.to_raw();
        Error::check(unsafe {
            sys::ghostty_mouse_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_MOUSE_ENCODER_OPT_SIZE,
                (&raw const raw).cast(),
            )
        })?;
        self.size = value;
        Ok(())
    }

    /// Sets whether any mouse button is currently pressed.
    pub fn set_any_button_pressed(&mut self, value: bool) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_mouse_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_MOUSE_ENCODER_OPT_ANY_BUTTON_PRESSED,
                (&raw const value).cast(),
            )
        })
    }

    /// Sets motion deduplication by cell.
    pub fn set_track_last_cell(&mut self, value: bool) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_mouse_encoder_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_MOUSE_ENCODER_OPT_TRACK_LAST_CELL,
                (&raw const value).cast(),
            )
        })
    }

    /// Copies tracking mode and format, clearing deduplication but preserving geometry and button state.
    pub fn set_from_terminal(&mut self, terminal: &Terminal) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_mouse_encoder_set_from_terminal(self.raw.as_ptr(), terminal.as_raw())
        })
    }

    /// Clears the last-cell motion deduplication state.
    pub fn reset(&mut self) -> Result<()> {
        Error::check(unsafe { sys::ghostty_mouse_encoder_reset(self.raw.as_ptr()) })
    }

    /// Encodes an event; unreported or duplicate events produce an empty vector.
    ///
    /// Returns InvalidValue for coordinates outside the C encoder's integer range.
    pub fn encode(&mut self, event: &MouseEvent) -> Result<Vec<u8>> {
        let position = event.position();
        let x = f64::from(position.x) - f64::from(self.size.padding_left);
        let y = f64::from(position.y) - f64::from(self.size.padding_top);
        // C casts cells to u16 before clamping, and SGR pixels to i32 after rounding.
        // Validate both possible formats because set_from_terminal selects format internally.
        for (pixel, cell_size) in [(x, self.size.cell_width), (y, self.size.cell_height)] {
            let rounded = pixel.round();
            if !pixel.is_finite()
                || pixel.max(0.0) / f64::from(cell_size) >= 65536.0
                || rounded < f64::from(i32::MIN)
                || rounded > f64::from(i32::MAX)
            {
                return Err(Error::InvalidValue);
            }
        }
        ffi::encode_vec(|buf, cap, written| unsafe {
            sys::ghostty_mouse_encoder_encode(
                self.raw.as_ptr(),
                event.raw.as_ptr(),
                buf,
                cap,
                written,
            )
        })
    }
}

impl Drop for MouseEncoder {
    fn drop(&mut self) {
        unsafe { sys::ghostty_mouse_encoder_free(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_left_press_sgr() -> Result<()> {
        let mut encoder = MouseEncoder::new()?;
        encoder.set_tracking_mode(MouseTrackingMode::Normal)?;
        encoder.set_format(MouseFormat::Sgr)?;
        encoder.set_size(MouseEncoderSize {
            screen_width: 800,
            screen_height: 600,
            cell_width: 10,
            cell_height: 20,
            ..MouseEncoderSize::default()
        })?;
        let mut event = MouseEvent::new()?;
        event.set_action(MouseAction::Press)?;
        event.set_button(Some(MouseButton::Left))?;
        event.set_position(MousePosition { x: 50.0, y: 40.0 })?;
        assert_eq!(encoder.encode(&event)?, b"\x1b[<0;6;3M");
        Ok(())
    }

    #[test]
    fn mouse_rejects_unrepresentable_coordinates() -> Result<()> {
        let mut encoder = MouseEncoder::new()?;
        encoder.set_tracking_mode(MouseTrackingMode::Normal)?;
        encoder.set_format(MouseFormat::SgrPixels)?;
        assert_eq!(
            encoder.set_size(MouseEncoderSize {
                screen_width: u32::MAX,
                ..MouseEncoderSize::default()
            }),
            Err(Error::InvalidValue)
        );
        let mut event = MouseEvent::new()?;
        event.set_action(MouseAction::Release)?;
        event.set_button(Some(MouseButton::Left))?;
        assert_eq!(
            event.set_position(MousePosition {
                x: f32::NAN,
                y: 0.0
            }),
            Err(Error::InvalidValue)
        );
        event.set_position(MousePosition {
            x: f32::MAX,
            y: 0.0,
        })?;
        assert_eq!(encoder.encode(&event), Err(Error::InvalidValue));
        event.set_position(MousePosition { x: -2.0, y: -3.0 })?;
        assert_eq!(encoder.encode(&event)?, b"\x1b[<0;-2;-3m");
        Ok(())
    }

    #[test]
    fn mouse_terminal_modes_deduplicate_and_reset() -> Result<()> {
        let mut encoder = MouseEncoder::new()?;
        let mut event = MouseEvent::new()?;
        event.set_action(MouseAction::Motion)?;
        assert!(encoder.encode(&event)?.is_empty());
        let mut terminal = Terminal::new(80, 24)?;
        terminal.vt_write(b"\x1b[?1003h\x1b[?1006h");
        encoder.set_from_terminal(&terminal)?;
        encoder.set_track_last_cell(true)?;
        assert_eq!(encoder.encode(&event)?, b"\x1b[<35;1;1M");
        assert!(encoder.encode(&event)?.is_empty());
        encoder.reset()?;
        assert_eq!(encoder.encode(&event)?, b"\x1b[<35;1;1M");
        Ok(())
    }
}
