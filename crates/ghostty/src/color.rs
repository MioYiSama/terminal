//! RGB colors, the 256-color palette, and color parsing.

use core::ptr;

use ghostty_sys as sys;

use crate::{Error, Result};

/// An RGB color. Layout-compatible with `GhosttyColorRgb`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

const _: () = {
    assert!(size_of::<Rgb>() == size_of::<sys::GhosttyColorRgb>());
    assert!(align_of::<Rgb>() == align_of::<sys::GhosttyColorRgb>());
};

/// An index into the 256-color palette.
pub type PaletteIndex = u8;

/// A 256-color palette.
pub type Palette = [Rgb; 256];

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parses a Ghostty color value: X11 names, hex colors (`#rgb`,
    /// `#rrggbb`, 9 and 12 digit forms), and `rgb:`/`rgbi:` specifications.
    pub fn parse(value: &str) -> Result<Self> {
        let mut out = sys::GhosttyColorRgb::default();
        Error::check(unsafe {
            sys::ghostty_color_parse(value.as_ptr().cast(), value.len(), &mut out)
        })?;
        Ok(out.into())
    }

    /// Parses an X11 color name (case-insensitive, no hex values).
    pub fn parse_x11(name: &str) -> Result<Self> {
        let mut out = sys::GhosttyColorRgb::default();
        Error::check(unsafe {
            sys::ghostty_color_parse_x11(name.as_ptr().cast(), name.len(), &mut out)
        })?;
        Ok(out.into())
    }

    /// W3C relative luminance, from 0.0 (black) to 1.0 (white).
    pub fn luminance(self) -> f64 {
        unsafe { sys::ghostty_color_luminance(&self.into()) }
    }

    /// Perceived luminance, from 0.0 to 1.0.
    pub fn perceived_luminance(self) -> f64 {
        unsafe { sys::ghostty_color_perceived_luminance(&self.into()) }
    }

    /// WCAG contrast ratio between two colors, from 1.0 to 21.0.
    pub fn contrast(self, other: Rgb) -> f64 {
        unsafe { sys::ghostty_color_contrast(&self.into(), &other.into()) }
    }
}

impl From<sys::GhosttyColorRgb> for Rgb {
    fn from(c: sys::GhosttyColorRgb) -> Self {
        Self::new(c.r, c.g, c.b)
    }
}

impl From<Rgb> for sys::GhosttyColorRgb {
    fn from(c: Rgb) -> Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

/// Parses a Ghostty palette entry of the form `N=COLOR`.
pub fn parse_palette_entry(value: &str) -> Result<(PaletteIndex, Rgb)> {
    let mut index = 0;
    let mut rgb = sys::GhosttyColorRgb::default();
    Error::check(unsafe {
        sys::ghostty_color_parse_palette_entry(
            value.as_ptr().cast(),
            value.len(),
            &mut index,
            &mut rgb,
        )
    })?;
    Ok((index, rgb.into()))
}

/// Ghostty's built-in default 256-color palette.
pub fn default_palette() -> Palette {
    let mut out = [Rgb::default(); 256];
    unsafe { sys::ghostty_color_palette_default(out.as_mut_ptr().cast()) };
    out
}

/// A set of palette indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PaletteMask([u64; 4]);

impl PaletteMask {
    pub const fn new() -> Self {
        Self([0; 4])
    }

    pub fn insert(&mut self, index: PaletteIndex) {
        self.0[usize::from(index >> 6)] |= 1 << (index & 63);
    }

    pub fn remove(&mut self, index: PaletteIndex) {
        self.0[usize::from(index >> 6)] &= !(1 << (index & 63));
    }

    pub fn contains(&self, index: PaletteIndex) -> bool {
        self.0[usize::from(index >> 6)] & (1 << (index & 63)) != 0
    }

    pub(crate) fn to_raw(self) -> sys::GhosttyColorPaletteMask {
        sys::GhosttyColorPaletteMask { bits: self.0 }
    }
}

impl FromIterator<PaletteIndex> for PaletteMask {
    fn from_iter<I: IntoIterator<Item = PaletteIndex>>(iter: I) -> Self {
        let mut mask = Self::new();
        iter.into_iter().for_each(|i| mask.insert(i));
        mask
    }
}

/// Generates a 256-color palette from base colors.
///
/// Indices 0–15 always come from `base` (Ghostty's default palette when
/// `None`), as do the indices in `skip`. The color cube (16–231) and the
/// grayscale ramp (232–255) are interpolated from `background` to
/// `foreground`. For light themes, `harmonious` keeps the
/// background-to-foreground orientation instead of running dark-to-light.
pub fn generate_palette(
    base: Option<&Palette>,
    skip: &PaletteMask,
    background: Rgb,
    foreground: Rgb,
    harmonious: bool,
) -> Palette {
    let mut out = [Rgb::default(); 256];
    let skip = skip.to_raw();
    unsafe {
        sys::ghostty_color_palette_generate(
            base.map_or(ptr::null(), |p| p.as_ptr().cast()),
            &skip,
            &background.into(),
            &foreground.into(),
            harmonious,
            out.as_mut_ptr().cast(),
        )
    };
    out
}

/// All X11 color names known to Ghostty, with their colors.
pub fn x11_colors() -> impl ExactSizeIterator<Item = (&'static str, Rgb)> {
    let entries = unsafe {
        core::slice::from_raw_parts(
            sys::ghostty_color_x11_names(),
            sys::ghostty_color_x11_name_count(),
        )
    };
    entries.iter().map(|entry| {
        // SAFETY: entries are static, NUL-terminated ASCII names.
        let name = unsafe { core::ffi::CStr::from_ptr(entry.name) };
        (
            name.to_str().expect("X11 color names are ASCII"),
            entry.color.into(),
        )
    })
}
