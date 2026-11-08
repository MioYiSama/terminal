//! SGR (Select Graphic Rendition) parameter parser.

use std::ptr::NonNull;

use ghostty_sys as sys;

use crate::{
    Error, Result,
    color::{PaletteIndex, Rgb},
    style::Underline,
};

/// A decoded SGR attribute. Unknown parameters borrow the parser's storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SgrAttribute<'a> {
    /// Reset all attributes.
    Unset,
    /// An unknown or invalid parameter, with the full list and unparsed suffix.
    Unknown {
        /// All parameters supplied to the parser.
        full: &'a [u16],
        /// The suffix at which parsing encountered the unknown attribute.
        partial: &'a [u16],
    },
    /// Enable bold.
    Bold,
    /// Reset bold and faint.
    ResetBold,
    /// Enable italic.
    Italic,
    /// Disable italic.
    ResetItalic,
    /// Enable faint.
    Faint,
    /// Set the underline style.
    Underline(Underline),
    /// Set the underline RGB color.
    UnderlineColor(Rgb),
    /// Set the underline palette color.
    UnderlineColor256(PaletteIndex),
    /// Reset underline color.
    ResetUnderlineColor,
    /// Enable overline.
    Overline,
    /// Disable overline.
    ResetOverline,
    /// Enable blinking.
    Blink,
    /// Disable blinking.
    ResetBlink,
    /// Enable inverse video.
    Inverse,
    /// Disable inverse video.
    ResetInverse,
    /// Enable invisible text.
    Invisible,
    /// Disable invisible text.
    ResetInvisible,
    /// Enable strikethrough.
    Strikethrough,
    /// Disable strikethrough.
    ResetStrikethrough,
    /// Set foreground RGB color.
    DirectColorFg(Rgb),
    /// Set background RGB color.
    DirectColorBg(Rgb),
    /// Set background from the basic palette.
    Bg8(PaletteIndex),
    /// Set foreground from the basic palette.
    Fg8(PaletteIndex),
    /// Reset foreground color.
    ResetFg,
    /// Reset background color.
    ResetBg,
    /// Set background from the bright palette.
    BrightBg8(PaletteIndex),
    /// Set foreground from the bright palette.
    BrightFg8(PaletteIndex),
    /// Set background from the 256-color palette.
    Bg256(PaletteIndex),
    /// Set foreground from the 256-color palette.
    Fg256(PaletteIndex),
}

/// An owning SGR parser with allocation-free, lending attribute iteration.
pub struct SgrParser {
    raw: NonNull<sys::GhosttySgrParserImpl>,
}

// SAFETY: the parser owns independent state with no thread affinity. Moving it
// transfers exclusive access to its state; no shared concurrent calls are allowed.
unsafe impl Send for SgrParser {}

impl SgrParser {
    /// Creates a parser using the library's default allocator.
    pub fn new() -> Result<Self> {
        let mut raw = std::ptr::null_mut();
        unsafe { Error::check(sys::ghostty_sgr_new(std::ptr::null(), &mut raw))? };
        Ok(Self {
            raw: NonNull::new(raw).expect("successful SGR allocation returned null"),
        })
    }

    /// Restarts iteration over the current parameters without clearing them.
    pub fn reset(&mut self) {
        unsafe { sys::ghostty_sgr_reset(self.raw.as_ptr()) };
    }

    /// Copies parameters and resets iteration.
    ///
    /// `separators[i]` separates `params[i]` from the following parameter. Use
    /// `b':'` for subparameters and `b';'` otherwise (the last byte is ignored).
    /// `None` uses semicolons throughout. When provided, the separators must
    /// have exactly the same length as `params`, or `Error::InvalidValue` is
    /// returned. Invalid separator bytes are treated as semicolons by Ghostty.
    pub fn set_params(&mut self, params: &[u16], separators: Option<&[u8]>) -> Result<()> {
        if separators.is_some_and(|separators| separators.len() != params.len()) {
            return Err(Error::InvalidValue);
        }
        unsafe {
            Error::check(sys::ghostty_sgr_set_params(
                self.raw.as_ptr(),
                params.as_ptr(),
                separators.map_or(std::ptr::null(), |separators| separators.as_ptr().cast()),
                params.len(),
            ))
        }
    }

    /// Returns the next attribute, borrowing unknown parameters until released.
    ///
    /// This is a lending iterator rather than `Iterator`: borrowed unknown
    /// parameters must not survive replacing or freeing the parser's storage.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<SgrAttribute<'_>> {
        let mut raw = sys::GhosttySgrAttribute::default();
        if !unsafe { sys::ghostty_sgr_next(self.raw.as_ptr(), &mut raw) } {
            return None;
        }
        // SAFETY: the returned tag identifies the initialized union field. Any
        // unknown slices point into parser-owned storage and the returned
        // lifetime prevents further parser calls until those slices are released.
        Some(unsafe {
            match raw.tag {
                sys::GHOSTTY_SGR_ATTR_UNSET => SgrAttribute::Unset,
                sys::GHOSTTY_SGR_ATTR_UNKNOWN => {
                    let unknown = raw.value.unknown;
                    let mut full = std::ptr::null();
                    let mut partial = std::ptr::null();
                    let full_len = sys::ghostty_sgr_unknown_full(unknown, &mut full);
                    let partial_len = sys::ghostty_sgr_unknown_partial(unknown, &mut partial);
                    SgrAttribute::Unknown {
                        full: if full_len == 0 {
                            &[]
                        } else {
                            std::slice::from_raw_parts(full, full_len)
                        },
                        partial: if partial_len == 0 {
                            &[]
                        } else {
                            std::slice::from_raw_parts(partial, partial_len)
                        },
                    }
                }
                sys::GHOSTTY_SGR_ATTR_BOLD => SgrAttribute::Bold,
                sys::GHOSTTY_SGR_ATTR_RESET_BOLD => SgrAttribute::ResetBold,
                sys::GHOSTTY_SGR_ATTR_ITALIC => SgrAttribute::Italic,
                sys::GHOSTTY_SGR_ATTR_RESET_ITALIC => SgrAttribute::ResetItalic,
                sys::GHOSTTY_SGR_ATTR_FAINT => SgrAttribute::Faint,
                sys::GHOSTTY_SGR_ATTR_UNDERLINE => {
                    SgrAttribute::Underline(Underline::from_lib(raw.value.underline))
                }
                sys::GHOSTTY_SGR_ATTR_UNDERLINE_COLOR => {
                    SgrAttribute::UnderlineColor(raw.value.underline_color.into())
                }
                sys::GHOSTTY_SGR_ATTR_UNDERLINE_COLOR_256 => {
                    SgrAttribute::UnderlineColor256(raw.value.underline_color_256)
                }
                sys::GHOSTTY_SGR_ATTR_RESET_UNDERLINE_COLOR => SgrAttribute::ResetUnderlineColor,
                sys::GHOSTTY_SGR_ATTR_OVERLINE => SgrAttribute::Overline,
                sys::GHOSTTY_SGR_ATTR_RESET_OVERLINE => SgrAttribute::ResetOverline,
                sys::GHOSTTY_SGR_ATTR_BLINK => SgrAttribute::Blink,
                sys::GHOSTTY_SGR_ATTR_RESET_BLINK => SgrAttribute::ResetBlink,
                sys::GHOSTTY_SGR_ATTR_INVERSE => SgrAttribute::Inverse,
                sys::GHOSTTY_SGR_ATTR_RESET_INVERSE => SgrAttribute::ResetInverse,
                sys::GHOSTTY_SGR_ATTR_INVISIBLE => SgrAttribute::Invisible,
                sys::GHOSTTY_SGR_ATTR_RESET_INVISIBLE => SgrAttribute::ResetInvisible,
                sys::GHOSTTY_SGR_ATTR_STRIKETHROUGH => SgrAttribute::Strikethrough,
                sys::GHOSTTY_SGR_ATTR_RESET_STRIKETHROUGH => SgrAttribute::ResetStrikethrough,
                sys::GHOSTTY_SGR_ATTR_DIRECT_COLOR_FG => {
                    SgrAttribute::DirectColorFg(raw.value.direct_color_fg.into())
                }
                sys::GHOSTTY_SGR_ATTR_DIRECT_COLOR_BG => {
                    SgrAttribute::DirectColorBg(raw.value.direct_color_bg.into())
                }
                sys::GHOSTTY_SGR_ATTR_BG_8 => SgrAttribute::Bg8(raw.value.bg_8),
                sys::GHOSTTY_SGR_ATTR_FG_8 => SgrAttribute::Fg8(raw.value.fg_8),
                sys::GHOSTTY_SGR_ATTR_RESET_FG => SgrAttribute::ResetFg,
                sys::GHOSTTY_SGR_ATTR_RESET_BG => SgrAttribute::ResetBg,
                sys::GHOSTTY_SGR_ATTR_BRIGHT_BG_8 => SgrAttribute::BrightBg8(raw.value.bright_bg_8),
                sys::GHOSTTY_SGR_ATTR_BRIGHT_FG_8 => SgrAttribute::BrightFg8(raw.value.bright_fg_8),
                sys::GHOSTTY_SGR_ATTR_BG_256 => SgrAttribute::Bg256(raw.value.bg_256),
                sys::GHOSTTY_SGR_ATTR_FG_256 => SgrAttribute::Fg256(raw.value.fg_256),
                tag => panic!("libghostty-vt returned unknown SGR tag {tag}"),
            }
        })
    }
}

impl Drop for SgrParser {
    fn drop(&mut self) {
        unsafe { sys::ghostty_sgr_free(self.raw.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kakoune_underline_and_rgb_colors() {
        let mut parser = SgrParser::new().unwrap();
        let params = [
            4, 3, 38, 2, 51, 51, 51, 48, 2, 170, 170, 170, 58, 2, 255, 97, 136,
        ];
        let mut separators = [b';'; 17];
        separators[0] = b':';
        parser.set_params(&params, Some(&separators)).unwrap();
        assert_eq!(
            parser.next(),
            Some(SgrAttribute::Underline(Underline::Curly))
        );
        assert_eq!(
            parser.next(),
            Some(SgrAttribute::DirectColorFg(Rgb {
                r: 51,
                g: 51,
                b: 51
            }))
        );
        assert_eq!(
            parser.next(),
            Some(SgrAttribute::DirectColorBg(Rgb {
                r: 170,
                g: 170,
                b: 170
            }))
        );
        assert_eq!(
            parser.next(),
            Some(SgrAttribute::UnderlineColor(Rgb {
                r: 255,
                g: 97,
                b: 136
            }))
        );
        assert_eq!(parser.next(), None);
        parser.reset();
        assert_eq!(
            parser.next(),
            Some(SgrAttribute::Underline(Underline::Curly))
        );
    }

    #[test]
    fn basic_and_unknown_parameters() {
        let mut parser = SgrParser::new().unwrap();
        parser.set_params(&[1, 31, 999], None).unwrap();
        assert_eq!(parser.next(), Some(SgrAttribute::Bold));
        assert_eq!(parser.next(), Some(SgrAttribute::Fg8(1)));
        assert_eq!(
            parser.next(),
            Some(SgrAttribute::Unknown {
                full: &[1, 31, 999],
                partial: &[999]
            })
        );
        assert_eq!(parser.next(), None);
    }

    #[test]
    fn rejects_mismatched_separator_lengths() {
        let mut parser = SgrParser::new().unwrap();
        assert_eq!(
            parser.set_params(&[4, 3], Some(b":")),
            Err(Error::InvalidValue)
        );
    }
}
