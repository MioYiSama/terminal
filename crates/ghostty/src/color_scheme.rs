//! Light/dark color scheme reports (`CSI ? 997 ; Ps n`).
//!
//! The terminal answers `CSI ? 996 n` queries through its color scheme callback.
//! Use this encoder for unsolicited scheme changes, only while the application
//! has enabled color scheme reporting (DEC mode 2031).

use ghostty_sys as sys;

use crate::Result;
use crate::ffi::{c_enum, encode_vec};

c_enum! {
    /// The terminal's current light or dark color scheme.
    pub enum ColorScheme {
        /// A light color scheme.
        Light = sys::GHOSTTY_COLOR_SCHEME_LIGHT,
        /// A dark color scheme.
        Dark = sys::GHOSTTY_COLOR_SCHEME_DARK,
    }
}

impl ColorScheme {
    /// Encodes a color scheme report, ready to write to the pty.
    ///
    /// Dark emits `CSI ? 997 ; 1 n`; light emits `CSI ? 997 ; 2 n`.
    /// Send unsolicited reports only while DEC mode 2031 is enabled. These
    /// bytes also match the terminal's response to a `CSI ? 996 n` query.
    pub fn encode(self) -> Result<Vec<u8>> {
        encode_vec(|buf, len, written| unsafe {
            sys::ghostty_color_scheme_report_encode(self.to_raw(), buf, len, written)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ColorScheme;

    #[test]
    fn dark_report_matches_c_example() {
        assert_eq!(ColorScheme::Dark.encode().unwrap(), b"\x1b[?997;1n");
    }

    #[test]
    fn light_report_uses_light_status_parameter() {
        assert_eq!(ColorScheme::Light.encode().unwrap(), b"\x1b[?997;2n");
    }
}
