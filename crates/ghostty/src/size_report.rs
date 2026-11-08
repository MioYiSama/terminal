//! In-band size reports (mode 2048) and XTWINOPS size replies.
//!
//! A terminal size callback answers size queries automatically. Use this encoder
//! when sending reports yourself, for example after a window resize.

use ghostty_sys as sys;

use crate::Result;
use crate::ffi::{c_enum, encode_vec};

c_enum! {
    /// The escape-sequence format used for a terminal size report.
    pub enum Style {
        /// In-band mode 2048: `CSI 48 ; rows ; columns ; height ; width t`.
        Mode2048 = sys::GHOSTTY_SIZE_REPORT_MODE_2048,
        /// XTWINOPS text-area size in pixels: `CSI 4 ; height ; width t`.
        Csi14T = sys::GHOSTTY_SIZE_REPORT_CSI_14_T,
        /// XTWINOPS cell size in pixels: `CSI 6 ; height ; width t`.
        Csi16T = sys::GHOSTTY_SIZE_REPORT_CSI_16_T,
        /// XTWINOPS text-area size in cells: `CSI 8 ; rows ; columns t`.
        Csi18T = sys::GHOSTTY_SIZE_REPORT_CSI_18_T,
        /// XTWINOPS screen size in cells: `CSI 9 ; rows ; columns t`.
        ///
        /// Physical screen dimensions are unknown, so this uses the text-area
        /// dimensions, as does `Csi18T`.
        Csi19T = sys::GHOSTTY_SIZE_REPORT_CSI_19_T,
    }
}

/// Terminal geometry used to encode size reports.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size {
    /// Terminal row count in cells.
    pub rows: u16,
    /// Terminal column count in cells.
    pub columns: u16,
    /// Width of one terminal cell in pixels.
    pub cell_width: u32,
    /// Height of one terminal cell in pixels.
    pub cell_height: u32,
}

impl Size {
    /// Encodes this geometry as an escape sequence in the requested format.
    ///
    /// The returned bytes are ready to write to the pty. For mode 2048,
    /// unsolicited reports should only be sent while that mode is enabled.
    pub fn encode(self, style: Style) -> Result<Vec<u8>> {
        let size = self.to_raw();
        encode_vec(|buf, len, written| unsafe {
            sys::ghostty_size_report_encode(style.to_raw(), &size, buf, len, written)
        })
    }

    pub(crate) const fn to_raw(self) -> sys::GhosttySizeReportSize {
        sys::GhosttySizeReportSize {
            rows: self.rows,
            columns: self.columns,
            cell_width: self.cell_width,
            cell_height: self.cell_height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Size, Style};

    #[test]
    fn mode_2048_matches_c_example() {
        let size = Size {
            rows: 24,
            columns: 80,
            cell_width: 9,
            cell_height: 18,
        };
        assert_eq!(
            size.encode(Style::Mode2048).unwrap(),
            b"\x1b[48;24;80;432;720t"
        );
    }

    #[test]
    fn xtwinops_reports_text_area_and_cell_dimensions() {
        let size = Size {
            rows: 24,
            columns: 80,
            cell_width: 9,
            cell_height: 18,
        };
        for (style, expected) in [
            (Style::Csi14T, b"\x1b[4;432;720t".as_slice()),
            (Style::Csi16T, b"\x1b[6;18;9t".as_slice()),
            (Style::Csi18T, b"\x1b[8;24;80t".as_slice()),
            (Style::Csi19T, b"\x1b[9;24;80t".as_slice()),
        ] {
            assert_eq!(size.encode(style).unwrap(), expected);
        }
    }
}
