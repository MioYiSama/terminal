//! The render-state snapshot: one frame of visible cells, cursor and
//! image placements.

use core::ffi::c_void;
use core::{mem, ptr};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use ghostty_sys::*;

use super::{Rgb, Terminal};

/// Character that places a Kitty graphics image with text (virtual
/// placements).
const KITTY_PLACEHOLDER: char = '\u{10EEEE}';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    HollowBlock,
    Bar,
    Underline,
}

#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub shape: CursorShape,
    /// The cursor sits on a wide character and spans two cells.
    pub wide: bool,
    pub color: Rgb,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Clone, Debug, Default)]
pub struct Cell {
    /// Byte range of this cell's grapheme in `Line::text`; empty if blank.
    pub text: Range<u32>,
    pub fg: Rgb,
    /// `None` means the default background.
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub faint: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub underline: Underline,
    pub underline_color: Option<Rgb>,
    /// Wide character occupying this cell and the next.
    pub wide: bool,
    /// Part of an OSC 8 hyperlink.
    pub hyperlink: bool,
}

#[derive(Default)]
pub struct Line {
    pub text: String,
    /// One per column; spacers after wide characters have empty text.
    pub cells: Vec<Cell>,
    /// Soft-wrapped: the text continues on the next line.
    pub wrapped: bool,
    /// Unique to this content: a new version is taken each time the line
    /// is read, which is only when it changed. Anything derived from a line
    /// stays valid while its version does.
    pub version: u64,
}

/// Source of `Line::version`, shared by all terminals so versions never
/// repeat across them.
static NEXT_LINE_VERSION: AtomicU64 = AtomicU64::new(1);

/// One frame worth of terminal state, reused across frames to avoid
/// reallocating.
#[derive(Default)]
pub struct Frame {
    pub cols: u16,
    pub rows: u16,
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor: Option<Cursor>,
    pub lines: Vec<Line>,
    /// Kitty graphics placements touching the viewport, in no particular
    /// order.
    pub images: Vec<ImagePlacement>,
}

/// Where an image is drawn relative to the cell grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImageLayer {
    BelowBackground,
    BelowText,
    AboveText,
}

/// One rectangle of one Kitty graphics image to draw. Pixel values use the
/// cell size given to [`Terminal::resize`].
#[derive(Clone, Copy, Debug)]
pub struct ImagePlacement {
    pub image_id: u32,
    /// Changes whenever the image's pixels do (new data, animation frame).
    pub generation: u64,
    pub layer: ImageLayer,
    /// Draw order within the layer, lowest first.
    pub z: i32,
    /// Top-left cell relative to the viewport; can be negative when the
    /// image starts above or left of it.
    pub col: i32,
    pub row: i32,
    /// Offset of the image within its top-left cell.
    pub offset_x: u32,
    pub offset_y: u32,
    /// Size to draw at.
    pub width: u32,
    pub height: u32,
    /// Part of the image to draw, in image pixels, scaled to `width` x
    /// `height`.
    pub source_x: u32,
    pub source_y: u32,
    pub source_width: u32,
    pub source_height: u32,
}

pub struct RenderState {
    raw: GhosttyRenderState,
    rows: GhosttyRenderStateRowIterator,
    cells: GhosttyRenderStateRowCells,
    images: GhosttyRenderStateImageIterator,
    /// Placements generation `Frame::images` was last read at.
    images_generation: Option<u64>,
    /// Scratch space for grapheme clusters longer than 64 bytes.
    grapheme_overflow: Vec<u8>,
}

impl RenderState {
    pub fn new() -> Self {
        let mut raw = ptr::null_mut();
        let mut rows = ptr::null_mut();
        let mut cells = ptr::null_mut();
        let mut images = ptr::null_mut();
        unsafe {
            assert_eq!(
                ghostty_render_state_new(ptr::null(), &mut raw),
                GHOSTTY_SUCCESS
            );
            assert_eq!(
                ghostty_render_state_image_iterator_new(ptr::null(), &mut images),
                GHOSTTY_SUCCESS
            );
            assert_eq!(
                ghostty_render_state_row_iterator_new(ptr::null(), &mut rows),
                GHOSTTY_SUCCESS
            );
            assert_eq!(
                ghostty_render_state_row_cells_new(ptr::null(), &mut cells),
                GHOSTTY_SUCCESS
            );
        }
        Self {
            raw,
            rows,
            cells,
            images,
            images_generation: None,
            grapheme_overflow: Vec::new(),
        }
    }

    /// Re-read the image placements if the last update changed them.
    fn read_images(&mut self, out: &mut Vec<ImagePlacement>) {
        let mut generation = 0u64;
        unsafe {
            ghostty_render_state_get(
                self.raw,
                GHOSTTY_RENDER_STATE_DATA_IMAGE_PLACEMENTS_GENERATION,
                &mut generation as *mut u64 as *mut c_void,
            );
        }
        if self.images_generation == Some(generation) {
            return;
        }
        self.images_generation = Some(generation);
        out.clear();
        let result = unsafe {
            ghostty_render_state_get(
                self.raw,
                GHOSTTY_RENDER_STATE_DATA_IMAGE_ITERATOR,
                &mut self.images as *mut _ as *mut c_void,
            )
        };
        if result != GHOSTTY_SUCCESS {
            return;
        }
        while unsafe { ghostty_render_state_image_iterator_next(self.images) } {
            let mut protocol: GhosttyImageProtocol = GHOSTTY_IMAGE_PROTOCOL_KITTY;
            unsafe {
                ghostty_render_state_image_get(
                    self.images,
                    GHOSTTY_RENDER_STATE_IMAGE_DATA_PROTOCOL,
                    &mut protocol as *mut _ as *mut c_void,
                );
            }
            if protocol != GHOSTTY_IMAGE_PROTOCOL_KITTY {
                continue;
            }
            let mut layer: GhosttyImageLayer = GHOSTTY_IMAGE_LAYER_ABOVE_TEXT;
            let mut p = ImagePlacement {
                image_id: 0,
                generation: 0,
                layer: ImageLayer::AboveText,
                z: 0,
                col: 0,
                row: 0,
                offset_x: 0,
                offset_y: 0,
                width: 0,
                height: 0,
                source_x: 0,
                source_y: 0,
                source_width: 0,
                source_height: 0,
            };
            let keys = [
                GHOSTTY_RENDER_STATE_IMAGE_DATA_IMAGE_ID,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_IMAGE_GENERATION,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_LAYER,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_Z,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_VIEWPORT_COL,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_VIEWPORT_ROW,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_CELL_OFFSET_X,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_CELL_OFFSET_Y,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_PIXEL_WIDTH,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_PIXEL_HEIGHT,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_X,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_Y,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_WIDTH,
                GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_HEIGHT,
            ];
            let mut values: [*mut c_void; 14] = [
                &mut p.image_id as *mut u32 as *mut c_void,
                &mut p.generation as *mut u64 as *mut c_void,
                &mut layer as *mut GhosttyImageLayer as *mut c_void,
                &mut p.z as *mut i32 as *mut c_void,
                &mut p.col as *mut i32 as *mut c_void,
                &mut p.row as *mut i32 as *mut c_void,
                &mut p.offset_x as *mut u32 as *mut c_void,
                &mut p.offset_y as *mut u32 as *mut c_void,
                &mut p.width as *mut u32 as *mut c_void,
                &mut p.height as *mut u32 as *mut c_void,
                &mut p.source_x as *mut u32 as *mut c_void,
                &mut p.source_y as *mut u32 as *mut c_void,
                &mut p.source_width as *mut u32 as *mut c_void,
                &mut p.source_height as *mut u32 as *mut c_void,
            ];
            let result = unsafe {
                ghostty_render_state_image_get_multi(
                    self.images,
                    keys.len(),
                    keys.as_ptr(),
                    values.as_mut_ptr(),
                    ptr::null_mut(),
                )
            };
            if result != GHOSTTY_SUCCESS {
                continue;
            }
            p.layer = match layer {
                GHOSTTY_IMAGE_LAYER_BELOW_BACKGROUND => ImageLayer::BelowBackground,
                GHOSTTY_IMAGE_LAYER_BELOW_TEXT => ImageLayer::BelowText,
                _ => ImageLayer::AboveText,
            };
            out.push(p);
        }
    }

    fn cell_get<T>(&self, data: GhosttyRenderStateRowCellsData, out: &mut T) -> GhosttyResult {
        unsafe {
            ghostty_render_state_row_cells_get(self.cells, data, out as *mut T as *mut c_void)
        }
    }

    /// Capture the terminal's visible state into `frame`, re-reading only
    /// the rows that changed since the last snapshot into it. If libghostty
    /// cannot update its render state (out of memory), `frame` keeps the
    /// last complete snapshot.
    ///
    /// `frame` must be the one this render state last filled: rows reported
    /// clean are left as they are.
    pub fn snapshot(&mut self, terminal: &Terminal, frame: &mut Frame) {
        if unsafe { ghostty_render_state_update(self.raw, terminal.raw) } != GHOSTTY_SUCCESS {
            return;
        }
        let mut colors = sized!(GhosttyRenderStateColors);
        let mut cursor = sized!(GhosttyRenderStateCursor);
        let mut cols: u16 = 0;
        let mut rows: u16 = 0;
        render_state_get(self.raw, GHOSTTY_RENDER_STATE_DATA_COLORS, &mut colors);
        render_state_get(self.raw, GHOSTTY_RENDER_STATE_DATA_CURSOR, &mut cursor);
        render_state_get(self.raw, GHOSTTY_RENDER_STATE_DATA_COLS, &mut cols);
        render_state_get(self.raw, GHOSTTY_RENDER_STATE_DATA_ROWS, &mut rows);
        render_state_get(
            self.raw,
            GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
            &mut self.rows,
        );

        // libghostty redraws everything after a resize, but the frame may
        // also be new.
        let reread_all = frame.cols != cols || frame.lines.len() != rows as usize;
        frame.cols = cols;
        frame.background = colors.background.into();
        frame.foreground = colors.foreground.into();
        frame.lines.resize_with(rows as usize, Line::default);
        if reread_all {
            let mut count = 0usize;
            while count < rows as usize
                && unsafe { ghostty_render_state_row_iterator_next(self.rows) }
            {
                self.read_row(&colors, &mut frame.lines[count]);
                count += 1;
            }
            frame.lines.truncate(count);
        } else {
            let mut y: u16 = 0;
            while unsafe { ghostty_render_state_row_iterator_next_dirty(self.rows, &mut y) } {
                if let Some(line) = frame.lines.get_mut(y as usize) {
                    self.read_row(&colors, line);
                }
            }
        }
        frame.rows = frame.lines.len() as u16;
        frame.cursor = read_cursor(&cursor, &colors, &frame.lines);
        self.read_images(&mut frame.images);

        unsafe { ghostty_render_state_clean(self.raw) };
    }

    /// Read the row the row iterator is on.
    fn read_row(&mut self, colors: &GhosttyRenderStateColors, line: &mut Line) {
        line.version = NEXT_LINE_VERSION.fetch_add(1, Ordering::Relaxed);
        line.text.clear();
        line.cells.clear();
        let mut raw_row: GhosttyRow = Default::default();
        line.wrapped = unsafe {
            ghostty_render_state_row_get(
                self.rows,
                GHOSTTY_RENDER_STATE_ROW_DATA_RAW,
                &mut raw_row as *mut _ as *mut c_void,
            ) == GHOSTTY_SUCCESS
                && {
                    let mut wrapped = false;
                    ghostty_row_get(
                        raw_row,
                        GHOSTTY_ROW_DATA_WRAP,
                        &mut wrapped as *mut bool as *mut c_void,
                    ) == GHOSTTY_SUCCESS
                        && wrapped
                }
        };

        let mut selection = sized!(GhosttyRenderStateRowSelection);
        let selected = unsafe {
            ghostty_render_state_row_get(
                self.rows,
                GHOSTTY_RENDER_STATE_ROW_DATA_SELECTION,
                &mut selection as *mut _ as *mut c_void,
            )
        } == GHOSTTY_SUCCESS;
        unsafe {
            ghostty_render_state_row_get(
                self.rows,
                GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,
                &mut self.cells as *mut _ as *mut c_void,
            );
        }

        let mut col: u16 = 0;
        while unsafe { ghostty_render_state_row_cells_next(self.cells) } {
            let in_selection = selected && col >= selection.start_x && col <= selection.end_x;
            let cell = self.read_cell(colors, in_selection, &mut line.text);
            line.cells.push(cell);
            col += 1;
        }
    }

    /// Read the cell the cell iterator is on, appending its text to `text`.
    fn read_cell(
        &mut self,
        colors: &GhosttyRenderStateColors,
        in_selection: bool,
        text: &mut String,
    ) -> Cell {
        let mut cell = Cell::default();

        let mut raw: GhosttyCell = 0;
        self.cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW, &mut raw);
        let mut wide: GhosttyCellWide = GHOSTTY_CELL_WIDE_NARROW;
        unsafe {
            ghostty_cell_get(
                raw,
                GHOSTTY_CELL_DATA_WIDE,
                &mut wide as *mut _ as *mut c_void,
            );
            ghostty_cell_get(
                raw,
                GHOSTTY_CELL_DATA_HAS_HYPERLINK,
                &mut cell.hyperlink as *mut bool as *mut c_void,
            );
        }
        cell.wide = wide == GHOSTTY_CELL_WIDE_WIDE;
        let spacer = wide == GHOSTTY_CELL_WIDE_SPACER_TAIL || wide == GHOSTTY_CELL_WIDE_SPACER_HEAD;
        if !spacer {
            let start = text.len();
            self.read_grapheme(text);
            // Kitty graphics placeholders stand for the image the render
            // state reports; they have no glyph of their own.
            if text[start..].starts_with(KITTY_PLACEHOLDER) {
                text.truncate(start);
            }
            cell.text = start as u32..text.len() as u32;
        }

        let mut style = GhosttyStyle::default();
        self.cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE, &mut style);
        let flags = style.flags as u32;
        cell.bold = flags & GHOSTTY_STYLE_FLAG_BOLD != 0;
        cell.italic = flags & GHOSTTY_STYLE_FLAG_ITALIC != 0;
        cell.faint = flags & GHOSTTY_STYLE_FLAG_FAINT != 0;
        cell.invisible = flags & GHOSTTY_STYLE_FLAG_INVISIBLE != 0;
        cell.strikethrough = flags & GHOSTTY_STYLE_FLAG_STRIKETHROUGH != 0;
        cell.overline = flags & GHOSTTY_STYLE_FLAG_OVERLINE != 0;
        cell.underline =
            match (flags & GHOSTTY_STYLE_UNDERLINE_MASK) >> GHOSTTY_STYLE_UNDERLINE_SHIFT {
                0 => Underline::None,
                1 => Underline::Single,
                2 => Underline::Double,
                3 => Underline::Curly,
                4 => Underline::Dotted,
                _ => Underline::Dashed,
            };
        cell.underline_color = resolve_color(colors, &style.underline_color);

        let mut rgb = GhosttyColorRgb::default();
        let fg = (self.cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR, &mut rgb)
            == GHOSTTY_SUCCESS)
            .then(|| Rgb::from(rgb));
        let bg = (self.cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR, &mut rgb)
            == GHOSTTY_SUCCESS)
            .then(|| Rgb::from(rgb));
        let (mut fg, mut bg) = (fg.unwrap_or(colors.foreground.into()), bg);
        // Inverse video and selection both swap foreground and background.
        let inverse = flags & GHOSTTY_STYLE_FLAG_INVERSE != 0;
        if inverse != in_selection {
            let new_fg = bg.unwrap_or(colors.background.into());
            bg = Some(fg);
            fg = new_fg;
        }
        cell.fg = fg;
        cell.bg = bg;
        cell
    }

    /// Append the current cell's grapheme cluster to `text`. Clusters too
    /// long for the stack buffer go through a reused heap buffer.
    fn read_grapheme(&mut self, text: &mut String) {
        let mut stack = [0u8; 64];
        let mut buf = GhosttyBuffer {
            ptr: stack.as_mut_ptr(),
            cap: stack.len(),
            len: 0,
        };
        match self.cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &mut buf) {
            GHOSTTY_SUCCESS => push_utf8(text, &stack[..buf.len]),
            GHOSTTY_OUT_OF_SPACE => {
                let mut overflow = mem::take(&mut self.grapheme_overflow);
                overflow.resize(buf.len, 0);
                buf = GhosttyBuffer {
                    ptr: overflow.as_mut_ptr(),
                    cap: overflow.len(),
                    len: 0,
                };
                if self.cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &mut buf)
                    == GHOSTTY_SUCCESS
                {
                    push_utf8(text, &overflow[..buf.len]);
                }
                self.grapheme_overflow = overflow;
            }
            _ => {}
        }
    }
}

fn render_state_get<T>(raw: GhosttyRenderState, data: GhosttyRenderStateData, out: &mut T) {
    unsafe { ghostty_render_state_get(raw, data, out as *mut T as *mut c_void) };
}

fn resolve_color(colors: &GhosttyRenderStateColors, color: &GhosttyStyleColor) -> Option<Rgb> {
    match color.tag as GhosttyStyleColorTag {
        GHOSTTY_STYLE_COLOR_RGB => Some(unsafe { color.value.rgb }.into()),
        GHOSTTY_STYLE_COLOR_PALETTE => {
            Some(colors.palette[unsafe { color.value.palette } as usize].into())
        }
        _ => None,
    }
}

fn read_cursor(
    cursor: &GhosttyRenderStateCursor,
    colors: &GhosttyRenderStateColors,
    lines: &[Line],
) -> Option<Cursor> {
    if !(cursor.visible && cursor.viewport_has_value) {
        return None;
    }
    let mut x = cursor.viewport_x;
    if cursor.wide_tail && x > 0 {
        x -= 1;
    }
    let y = cursor.viewport_y;
    let wide = lines
        .get(y as usize)
        .and_then(|line| line.cells.get(x as usize))
        .is_some_and(|cell| cell.wide);
    Some(Cursor {
        x,
        y,
        wide,
        shape: match cursor.visual_style {
            GHOSTTY_CURSOR_STYLE_BAR => CursorShape::Bar,
            GHOSTTY_CURSOR_STYLE_UNDERLINE => CursorShape::Underline,
            GHOSTTY_CURSOR_STYLE_BLOCK_HOLLOW => CursorShape::HollowBlock,
            _ => CursorShape::Block,
        },
        color: if colors.cursor_has_value {
            colors.cursor.into()
        } else {
            colors.foreground.into()
        },
    })
}

fn push_utf8(out: &mut String, bytes: &[u8]) {
    match core::str::from_utf8(bytes) {
        Ok(s) => out.push_str(s),
        Err(_) => out.push(char::REPLACEMENT_CHARACTER),
    }
}

impl Drop for RenderState {
    fn drop(&mut self) {
        unsafe {
            ghostty_render_state_image_iterator_free(self.images);
            ghostty_render_state_row_cells_free(self.cells);
            ghostty_render_state_row_iterator_free(self.rows);
            ghostty_render_state_free(self.raw);
        }
    }
}
