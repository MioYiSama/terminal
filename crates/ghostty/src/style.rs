//! Cell styles: colors, text attributes, and underline styles.

use ghostty_sys as sys;

use crate::color::{PaletteIndex, Rgb};
use crate::ffi::c_enum;

/// A color chosen by a style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StyleColor {
    /// The style doesn't choose a color; use the terminal default.
    #[default]
    None,
    /// An index into the terminal palette.
    Palette(PaletteIndex),
    /// A direct RGB color.
    Rgb(Rgb),
}

impl StyleColor {
    pub(crate) fn from_raw(raw: &sys::GhosttyStyleColor) -> Self {
        // SAFETY: the tag says which union field is initialized; all bytes
        // of the 3-byte value are always initialized.
        match i32::from(raw.tag) {
            sys::GHOSTTY_STYLE_COLOR_PALETTE => Self::Palette(unsafe { raw.value.palette }),
            sys::GHOSTTY_STYLE_COLOR_RGB => Self::Rgb(unsafe { raw.value.rgb }.into()),
            _ => Self::None,
        }
    }

    pub(crate) fn to_raw(self) -> sys::GhosttyStyleColor {
        let mut raw = sys::GhosttyStyleColor::default();
        match self {
            Self::None => raw.tag = sys::GHOSTTY_STYLE_COLOR_NONE as u8,
            Self::Palette(index) => {
                raw.tag = sys::GHOSTTY_STYLE_COLOR_PALETTE as u8;
                raw.value.palette = index;
            }
            Self::Rgb(rgb) => {
                raw.tag = sys::GHOSTTY_STYLE_COLOR_RGB as u8;
                raw.value.rgb = rgb.into();
            }
        }
        raw
    }
}

c_enum! {
    /// Underline style (`GhosttySgrUnderline`).
    #[derive(Default)]
    pub enum Underline {
        #[default]
        None = sys::GHOSTTY_SGR_UNDERLINE_NONE,
        Single = sys::GHOSTTY_SGR_UNDERLINE_SINGLE,
        Double = sys::GHOSTTY_SGR_UNDERLINE_DOUBLE,
        Curly = sys::GHOSTTY_SGR_UNDERLINE_CURLY,
        Dotted = sys::GHOSTTY_SGR_UNDERLINE_DOTTED,
        Dashed = sys::GHOSTTY_SGR_UNDERLINE_DASHED,
    }
}

c_enum! {
    /// Cursor shape (`GhosttyCursorStyle`, DECSCUSR).
    pub enum CursorStyle {
        /// Bar cursor (DECSCUSR 5, 6).
        Bar = sys::GHOSTTY_CURSOR_STYLE_BAR,
        /// Block cursor (DECSCUSR 1, 2).
        Block = sys::GHOSTTY_CURSOR_STYLE_BLOCK,
        /// Underline cursor (DECSCUSR 3, 4).
        Underline = sys::GHOSTTY_CURSOR_STYLE_UNDERLINE,
        /// Hollow block cursor.
        BlockHollow = sys::GHOSTTY_CURSOR_STYLE_BLOCK_HOLLOW,
    }
}

bitflags::bitflags! {
    /// On/off text attributes of a style.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct StyleFlags: u16 {
        const BOLD = sys::GHOSTTY_STYLE_FLAG_BOLD as u16;
        const ITALIC = sys::GHOSTTY_STYLE_FLAG_ITALIC as u16;
        const FAINT = sys::GHOSTTY_STYLE_FLAG_FAINT as u16;
        const BLINK = sys::GHOSTTY_STYLE_FLAG_BLINK as u16;
        const INVERSE = sys::GHOSTTY_STYLE_FLAG_INVERSE as u16;
        const INVISIBLE = sys::GHOSTTY_STYLE_FLAG_INVISIBLE as u16;
        const STRIKETHROUGH = sys::GHOSTTY_STYLE_FLAG_STRIKETHROUGH as u16;
        const OVERLINE = sys::GHOSTTY_STYLE_FLAG_OVERLINE as u16;
    }
}

/// The complete visual style of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Style {
    pub foreground: StyleColor,
    pub background: StyleColor,
    pub underline_color: StyleColor,
    pub flags: StyleFlags,
    pub underline: Underline,
}

impl Default for Style {
    fn default() -> Self {
        let mut raw = sys::GhosttyStyle::default();
        unsafe { sys::ghostty_style_default(&mut raw) };
        Self::from_raw(&raw)
    }
}

impl Style {
    /// Whether this is the default style.
    pub fn is_default(&self) -> bool {
        unsafe { sys::ghostty_style_is_default(&self.to_raw()) }
    }

    pub(crate) fn from_raw(raw: &sys::GhosttyStyle) -> Self {
        let underline = (u32::from(raw.flags) & sys::GHOSTTY_STYLE_UNDERLINE_MASK)
            >> sys::GHOSTTY_STYLE_UNDERLINE_SHIFT;
        Self {
            foreground: StyleColor::from_raw(&raw.fg_color),
            background: StyleColor::from_raw(&raw.bg_color),
            underline_color: StyleColor::from_raw(&raw.underline_color),
            flags: StyleFlags::from_bits_truncate(raw.flags),
            underline: Underline::from_lib(underline as i32),
        }
    }

    pub(crate) fn to_raw(self) -> sys::GhosttyStyle {
        let underline = (self.underline.to_raw() as u16) << sys::GHOSTTY_STYLE_UNDERLINE_SHIFT;
        sys::GhosttyStyle {
            fg_color: self.foreground.to_raw(),
            bg_color: self.background.to_raw(),
            underline_color: self.underline_color.to_raw(),
            flags: self.flags.bits() | underline,
            reserved: 0,
        }
    }
}
