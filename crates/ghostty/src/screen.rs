//! Packed cell and row values.

use core::ffi::c_void;

use ghostty_sys as sys;

use crate::Error;
use crate::color::{PaletteIndex, Rgb};
use crate::ffi::c_enum;
use crate::style::StyleColor;

c_enum! {
    /// What kind of content a cell holds.
    pub enum CellContentTag {
        /// A single codepoint (zero for an empty cell).
        Codepoint = sys::GHOSTTY_CELL_CONTENT_CODEPOINT,
        /// A codepoint that starts a multi-codepoint grapheme cluster.
        CodepointGrapheme = sys::GHOSTTY_CELL_CONTENT_CODEPOINT_GRAPHEME,
        /// No text, only a background color from the palette.
        BgColorPalette = sys::GHOSTTY_CELL_CONTENT_BG_COLOR_PALETTE,
        /// No text, only an RGB background color.
        BgColorRgb = sys::GHOSTTY_CELL_CONTENT_BG_COLOR_RGB,
    }
}

c_enum! {
    /// The width behavior of a cell.
    pub enum CellWide {
        /// A single-column character.
        Narrow = sys::GHOSTTY_CELL_WIDE_NARROW,
        /// The first column of a two-column character.
        Wide = sys::GHOSTTY_CELL_WIDE_WIDE,
        /// The second column of a two-column character.
        SpacerTail = sys::GHOSTTY_CELL_WIDE_SPACER_TAIL,
        /// Padding at the end of a row before a wide character wrapped.
        SpacerHead = sys::GHOSTTY_CELL_WIDE_SPACER_HEAD,
    }
}

c_enum! {
    /// Semantic content of a cell, set by OSC 133 prompt markers.
    pub enum CellSemanticContent {
        Output = sys::GHOSTTY_CELL_SEMANTIC_OUTPUT,
        Input = sys::GHOSTTY_CELL_SEMANTIC_INPUT,
        Prompt = sys::GHOSTTY_CELL_SEMANTIC_PROMPT,
    }
}

c_enum! {
    /// Semantic prompt state of a row.
    pub enum RowSemanticPrompt {
        None = sys::GHOSTTY_ROW_SEMANTIC_NONE,
        Prompt = sys::GHOSTTY_ROW_SEMANTIC_PROMPT,
        PromptContinuation = sys::GHOSTTY_ROW_SEMANTIC_PROMPT_CONTINUATION,
    }
}

/// Identifier of a style within a page.
pub type StyleId = u16;

/// A single grid cell value. This is a copy, not a reference into the
/// terminal.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell(sys::GhosttyCell);

impl Cell {
    pub fn from_raw(raw: sys::GhosttyCell) -> Self {
        Self(raw)
    }

    pub fn as_raw(self) -> sys::GhosttyCell {
        self.0
    }

    /// Reads a key that is valid for every cell.
    fn get<T: Default>(self, key: sys::GhosttyCellData) -> T {
        let mut out = T::default();
        let code = unsafe { sys::ghostty_cell_get(self.0, key, (&raw mut out).cast::<c_void>()) };
        Error::check(code).expect("ghostty_cell_get failed for a valid key");
        out
    }

    /// The primary codepoint, or 0 if the cell has no text.
    pub fn codepoint(self) -> u32 {
        self.get(sys::GHOSTTY_CELL_DATA_CODEPOINT)
    }

    pub fn content_tag(self) -> CellContentTag {
        CellContentTag::from_lib(self.get(sys::GHOSTTY_CELL_DATA_CONTENT_TAG))
    }

    pub fn wide(self) -> CellWide {
        CellWide::from_lib(self.get(sys::GHOSTTY_CELL_DATA_WIDE))
    }

    pub fn has_text(self) -> bool {
        self.get(sys::GHOSTTY_CELL_DATA_HAS_TEXT)
    }

    /// Whether the cell has a non-default style.
    pub fn has_styling(self) -> bool {
        self.get(sys::GHOSTTY_CELL_DATA_HAS_STYLING)
    }

    pub fn style_id(self) -> StyleId {
        self.get(sys::GHOSTTY_CELL_DATA_STYLE_ID)
    }

    pub fn has_hyperlink(self) -> bool {
        self.get(sys::GHOSTTY_CELL_DATA_HAS_HYPERLINK)
    }

    /// Whether the cell is protected from erasure (DECSCA / SPA).
    pub fn is_protected(self) -> bool {
        self.get(sys::GHOSTTY_CELL_DATA_PROTECTED)
    }

    pub fn semantic_content(self) -> CellSemanticContent {
        CellSemanticContent::from_lib(self.get(sys::GHOSTTY_CELL_DATA_SEMANTIC_CONTENT))
    }

    /// The background color of a cell that holds no text, only a color
    /// (content tag [`BgColorPalette`](CellContentTag::BgColorPalette) or
    /// [`BgColorRgb`](CellContentTag::BgColorRgb)). `None` for other cells.
    pub fn background_only_color(self) -> Option<StyleColor> {
        match self.content_tag() {
            CellContentTag::BgColorPalette => Some(StyleColor::Palette(
                self.get::<PaletteIndex>(sys::GHOSTTY_CELL_DATA_COLOR_PALETTE),
            )),
            CellContentTag::BgColorRgb => Some(StyleColor::Rgb(Rgb::from(
                self.get::<sys::GhosttyColorRgb>(sys::GHOSTTY_CELL_DATA_COLOR_RGB),
            ))),
            _ => None,
        }
    }
}

/// A single grid row value. This is a copy, not a reference into the
/// terminal.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Row(sys::GhosttyRow);

impl Row {
    pub fn from_raw(raw: sys::GhosttyRow) -> Self {
        Self(raw)
    }

    pub fn as_raw(self) -> sys::GhosttyRow {
        self.0
    }

    fn get<T: Default>(self, key: sys::GhosttyRowData) -> T {
        let mut out = T::default();
        let code = unsafe { sys::ghostty_row_get(self.0, key, (&raw mut out).cast::<c_void>()) };
        Error::check(code).expect("ghostty_row_get failed for a valid key");
        out
    }

    /// Whether the row soft-wraps into the next row.
    pub fn is_wrapped(self) -> bool {
        self.get(sys::GHOSTTY_ROW_DATA_WRAP)
    }

    /// Whether the row continues a soft-wrapped previous row.
    pub fn is_wrap_continuation(self) -> bool {
        self.get(sys::GHOSTTY_ROW_DATA_WRAP_CONTINUATION)
    }

    /// Whether any cell in the row has a multi-codepoint grapheme.
    pub fn has_grapheme(self) -> bool {
        self.get(sys::GHOSTTY_ROW_DATA_GRAPHEME)
    }

    /// Whether any cell in the row has a non-default style.
    pub fn is_styled(self) -> bool {
        self.get(sys::GHOSTTY_ROW_DATA_STYLED)
    }

    /// Whether any cell in the row has a hyperlink.
    pub fn has_hyperlink(self) -> bool {
        self.get(sys::GHOSTTY_ROW_DATA_HYPERLINK)
    }

    pub fn semantic_prompt(self) -> RowSemanticPrompt {
        RowSemanticPrompt::from_lib(self.get(sys::GHOSTTY_ROW_DATA_SEMANTIC_PROMPT))
    }

    /// Whether the row contains a Kitty graphics virtual placeholder.
    pub fn has_kitty_virtual_placeholder(self) -> bool {
        self.get(sys::GHOSTTY_ROW_DATA_KITTY_VIRTUAL_PLACEHOLDER)
    }

    /// Whether the row changed since it was last marked clean.
    pub fn is_dirty(self) -> bool {
        self.get(sys::GHOSTTY_ROW_DATA_DIRTY)
    }
}
