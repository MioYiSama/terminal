//! Independent render snapshots and reusable, lending row/cell/image iterators.
use crate::{
    Error, Result, Terminal,
    color::{Palette, Rgb},
    ffi,
    kitty_graphics::ImageLayer,
    screen::{Cell, Row},
    style::{CursorStyle, Style},
};
use ghostty_sys as sys;
use std::{marker::PhantomData, ptr::NonNull};

ffi::c_enum! {
    /// Amount of the frame needing redraw.
    pub enum Dirty {
        /// No redraw needed.
        Clean = sys::GHOSTTY_RENDER_STATE_DIRTY_FALSE,
        /// Some rows changed.
        Partial = sys::GHOSTTY_RENDER_STATE_DIRTY_PARTIAL,
        /// Redraw everything.
        Full = sys::GHOSTTY_RENDER_STATE_DIRTY_FULL,
    }
}
ffi::c_enum! {
    /// Image protocol used by a render placement.
    pub enum ImageProtocol {
        /// Kitty graphics.
        Kitty = sys::GHOSTTY_IMAGE_PROTOCOL_KITTY,
    }
}
ffi::c_enum! {
    /// Kind of a Kitty placement in the captured frame.
    pub enum KittyPlacementKind {
        /// Run of Unicode placeholder cells.
        Virtual = sys::GHOSTTY_RENDER_STATE_KITTY_PLACEMENT_KIND_VIRTUAL,
        /// Relative to another placement.
        Relative = sys::GHOSTTY_RENDER_STATE_KITTY_PLACEMENT_KIND_RELATIVE,
        /// Positioned at a screen cell.
        Fixed = sys::GHOSTTY_RENDER_STATE_KITTY_PLACEMENT_KIND_FIXED,
    }
}
/// Requested or captured extra rows outside the viewport.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Overscan {
    /// Rows above the viewport.
    pub above: u16,
    /// Rows below the viewport.
    pub below: u16,
}
/// Opaque row identity, comparable across updates of a render state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowId([u64; 2]);
/// Cursor state of a captured frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Viewport position and whether the cursor is on a wide-character tail.
    pub viewport: Option<CursorViewport>,
    /// Visibility based on terminal modes.
    pub visible: bool,
    /// Whether the cursor should blink.
    pub blinking: bool,
    /// Whether this is a password input field.
    pub password_input: bool,
    /// Cursor shape.
    pub visual_style: CursorStyle,
}
/// Cursor position within the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorViewport {
    /// Column.
    pub x: u16,
    /// Row.
    pub y: u16,
    /// Whether the cursor is on a wide-character tail.
    pub wide_tail: bool,
}
/// Captured default colors and active palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Colors {
    /// Default background.
    pub background: Rgb,
    /// Default foreground.
    pub foreground: Rgb,
    /// Explicit cursor color, if set.
    pub cursor: Option<Rgb>,
    /// Active 256-color palette.
    pub palette: Palette,
}
/// Inclusive selected columns on a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowSelection {
    /// First selected column.
    pub start_x: u16,
    /// Last selected column.
    pub end_x: u16,
}

macro_rules! getters {
    ($(#[doc = $doc:literal] $name:ident: $ty:ty = $key:ident;)*) => {$(
        #[doc = $doc]
        pub fn $name(&self) -> Result<$ty> {
            // SAFETY: this key's output type is exactly the header's documented type.
            unsafe { self.get(sys::$key) }
        }
    )*};
}
/// A frame snapshot, independent of the terminal after its update completes.
pub struct RenderState {
    raw: NonNull<sys::GhosttyRenderStateImpl>,
    ready: bool,
}
// SAFETY: the state owns its data and allocator, has no thread affinity, and is never shared concurrently.
unsafe impl Send for RenderState {}
impl RenderState {
    /// Allocate a render state using the default allocator.
    pub fn new() -> Result<Self> {
        let mut raw = std::ptr::null_mut();
        Error::check(unsafe { sys::ghostty_render_state_new(std::ptr::null(), &mut raw) })?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
            ready: true,
        })
    }
    fn require_ready(&self) -> Result<()> {
        if self.ready {
            Ok(())
        } else {
            Err(Error::InvalidValue)
        }
    }
    /// Capture a terminal frame, consuming its dirty bookkeeping.
    /// This does not invalidate terminal-owned cell or image data.
    pub fn update(&mut self, terminal: &Terminal) -> Result<()> {
        self.begin_update(terminal)?;
        self.end_update()
    }
    /// Capture terminal-owned data, deferring independent work until `end_update`.
    /// Reading this state returns `InvalidValue` until the end phase completes.
    pub fn begin_update(&mut self, terminal: &Terminal) -> Result<()> {
        if !self.ready {
            self.end_update()?;
        }
        // Mark incomplete even if allocation fails partway through the begin phase.
        self.ready = false;
        Error::check(unsafe {
            sys::ghostty_render_state_begin_update(self.raw.as_ptr(), terminal.as_raw())
        })
    }
    /// Complete deferred work; no terminal borrow or lock is required.
    pub fn end_update(&mut self) -> Result<()> {
        Error::check(unsafe { sys::ghostty_render_state_end_update(self.raw.as_ptr()) })?;
        self.ready = true;
        Ok(())
    }
    /// Clear all global and per-row dirty flags after rendering a complete frame.
    pub fn clean(&mut self) -> Result<()> {
        self.require_ready()?;
        Error::check(unsafe { sys::ghostty_render_state_clean(self.raw.as_ptr()) })
    }
    // SAFETY: callers pair each key with the header's exact output type.
    unsafe fn get<T: Default>(&self, key: sys::GhosttyRenderStateData) -> Result<T> {
        self.require_ready()?;
        let mut out = T::default();
        Error::check(unsafe {
            sys::ghostty_render_state_get(self.raw.as_ptr(), key, (&mut out as *mut T).cast())
        })?;
        Ok(out)
    }
    getters! {
        /// Viewport columns.
        cols: u16 = GHOSTTY_RENDER_STATE_DATA_COLS;
        /// Viewport rows, excluding overscan.
        rows: u16 = GHOSTTY_RENDER_STATE_DATA_ROWS;
        /// Whether a cursor color is explicitly set.
        cursor_color_has_value: bool = GHOSTTY_RENDER_STATE_DATA_COLOR_CURSOR_HAS_VALUE;
        /// Whether the cursor is visible based on modes.
        cursor_visible: bool = GHOSTTY_RENDER_STATE_DATA_CURSOR_VISIBLE;
        /// Whether the cursor should blink.
        cursor_blinking: bool = GHOSTTY_RENDER_STATE_DATA_CURSOR_BLINKING;
        /// Whether this is a password field.
        cursor_password_input: bool = GHOSTTY_RENDER_STATE_DATA_CURSOR_PASSWORD_INPUT;
        /// Whether the cursor has a viewport position.
        cursor_viewport_has_value: bool = GHOSTTY_RENDER_STATE_DATA_CURSOR_VIEWPORT_HAS_VALUE;
        /// Generation of the complete image placement list.
        image_placements_generation: u64 = GHOSTTY_RENDER_STATE_DATA_IMAGE_PLACEMENTS_GENERATION;
    }
    /// Global redraw requirement.
    pub fn dirty(&self) -> Result<Dirty> {
        Ok(Dirty::from_lib(unsafe {
            self.get(sys::GHOSTTY_RENDER_STATE_DATA_DIRTY)?
        }))
    }
    /// Default/current background color.
    pub fn background(&self) -> Result<Rgb> {
        Ok(unsafe {
            self.get::<sys::GhosttyColorRgb>(sys::GHOSTTY_RENDER_STATE_DATA_COLOR_BACKGROUND)?
        }
        .into())
    }
    /// Default/current foreground color.
    pub fn foreground(&self) -> Result<Rgb> {
        Ok(unsafe {
            self.get::<sys::GhosttyColorRgb>(sys::GHOSTTY_RENDER_STATE_DATA_COLOR_FOREGROUND)?
        }
        .into())
    }
    /// Explicit cursor color.
    pub fn cursor_color(&self) -> Result<Option<Rgb>> {
        optional(unsafe {
            self.get::<sys::GhosttyColorRgb>(sys::GHOSTTY_RENDER_STATE_DATA_COLOR_CURSOR)
        })
        .map(|v| v.map(Into::into))
    }
    /// Active palette.
    pub fn palette(&self) -> Result<Palette> {
        self.require_ready()?;
        let mut out = [sys::GhosttyColorRgb::default(); 256];
        Error::check(unsafe {
            sys::ghostty_render_state_get(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_DATA_COLOR_PALETTE,
                out.as_mut_ptr().cast(),
            )
        })?;
        Ok(out.map(Into::into))
    }
    /// Cursor shape.
    pub fn cursor_visual_style(&self) -> Result<CursorStyle> {
        Ok(CursorStyle::from_lib(unsafe {
            self.get(sys::GHOSTTY_RENDER_STATE_DATA_CURSOR_VISUAL_STYLE)?
        }))
    }
    /// Cursor viewport column, if visible in the viewport.
    pub fn cursor_viewport_x(&self) -> Result<Option<u16>> {
        optional(unsafe { self.get(sys::GHOSTTY_RENDER_STATE_DATA_CURSOR_VIEWPORT_X) })
    }
    /// Cursor viewport row, if visible in the viewport.
    pub fn cursor_viewport_y(&self) -> Result<Option<u16>> {
        optional(unsafe { self.get(sys::GHOSTTY_RENDER_STATE_DATA_CURSOR_VIEWPORT_Y) })
    }
    /// Whether the cursor is on the tail of a wide character, if in the viewport.
    pub fn cursor_viewport_wide_tail(&self) -> Result<Option<bool>> {
        optional(unsafe { self.get(sys::GHOSTTY_RENDER_STATE_DATA_CURSOR_VIEWPORT_WIDE_TAIL) })
    }
    /// Read all cursor state together.
    pub fn cursor(&self) -> Result<Cursor> {
        self.require_ready()?;
        let mut out = unsafe { ffi::sized::<sys::GhosttyRenderStateCursor>() };
        Error::check(unsafe {
            sys::ghostty_render_state_get(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_DATA_CURSOR,
                (&mut out as *mut sys::GhosttyRenderStateCursor).cast(),
            )
        })?;
        Ok(Cursor {
            viewport: if out.viewport_has_value {
                Some(CursorViewport {
                    x: out.viewport_x,
                    y: out.viewport_y,
                    wide_tail: out.wide_tail,
                })
            } else {
                None
            },
            visible: out.visible,
            blinking: out.blinking,
            password_input: out.password_input,
            visual_style: CursorStyle::from_lib(out.visual_style),
        })
    }
    /// Read all colors together.
    pub fn colors(&self) -> Result<Colors> {
        self.require_ready()?;
        let mut out = unsafe { ffi::sized::<sys::GhosttyRenderStateColors>() };
        Error::check(unsafe {
            sys::ghostty_render_state_get(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_DATA_COLORS,
                (&mut out as *mut sys::GhosttyRenderStateColors).cast(),
            )
        })?;
        Ok(Colors {
            background: out.background.into(),
            foreground: out.foreground.into(),
            cursor: if out.cursor_has_value {
                Some(out.cursor.into())
            } else {
                None
            },
            palette: out.palette.map(Into::into),
        })
    }
    /// Extra rows captured by the last update.
    pub fn overscan(&self) -> Result<Overscan> {
        let out: sys::GhosttyRenderStateOverscan =
            unsafe { self.get(sys::GHOSTTY_RENDER_STATE_DATA_OVERSCAN)? };
        Ok(Overscan {
            above: out.above,
            below: out.below,
        })
    }
    /// Extra rows requested for the next update.
    pub fn overscan_request(&self) -> Result<Overscan> {
        let out: sys::GhosttyRenderStateOverscan =
            unsafe { self.get(sys::GHOSTTY_RENDER_STATE_DATA_OVERSCAN_REQUEST)? };
        Ok(Overscan {
            above: out.above,
            below: out.below,
        })
    }
    /// Override the global dirty state.
    pub fn set_dirty(&mut self, dirty: Dirty) -> Result<()> {
        self.require_ready()?;
        let value = dirty.to_raw();
        Error::check(unsafe {
            sys::ghostty_render_state_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_OPT_DIRTY,
                (&value as *const sys::GhosttyRenderStateDirty).cast(),
            )
        })
    }
    /// Request extra rows for future updates; zero on both sides restores the default.
    pub fn set_overscan(&mut self, overscan: Overscan) -> Result<()> {
        self.require_ready()?;
        let value = sys::GhosttyRenderStateOverscan {
            above: overscan.above,
            below: overscan.below,
        };
        Error::check(unsafe {
            sys::ghostty_render_state_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_OPT_OVERSCAN,
                (&value as *const sys::GhosttyRenderStateOverscan).cast(),
            )
        })
    }
    /// Populate a reusable row iterator, exclusively borrowing the frame to permit row dirty updates.
    pub fn row_iterator<'a>(&'a mut self, iterator: &'a mut RowIterator) -> Result<Rows<'a>> {
        self.require_ready()?;
        let mut raw = iterator.raw.as_ptr();
        Error::check(unsafe {
            sys::ghostty_render_state_get(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
                (&mut raw as *mut sys::GhosttyRenderStateRowIterator).cast(),
            )
        })?;
        Ok(Rows {
            iterator,
            _state: PhantomData,
        })
    }
    /// Populate a reusable image iterator; the frame cannot update while its view exists.
    pub fn images<'a>(&'a self, iterator: &'a mut ImageIterator) -> Result<Images<'a>> {
        self.require_ready()?;
        let mut raw = iterator.raw.as_ptr();
        Error::check(unsafe {
            sys::ghostty_render_state_get(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_DATA_IMAGE_ITERATOR,
                (&mut raw as *mut sys::GhosttyRenderStateImageIterator).cast(),
            )
        })?;
        Ok(Images {
            iterator,
            _state: PhantomData,
        })
    }
}
impl Drop for RenderState {
    fn drop(&mut self) {
        unsafe { sys::ghostty_render_state_free(self.raw.as_ptr()) }
    }
}
fn optional<T>(value: Result<T>) -> Result<Option<T>> {
    match value {
        Ok(v) => Ok(Some(v)),
        Err(Error::NoValue) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Owning reusable allocation for iterating render rows.
pub struct RowIterator {
    raw: NonNull<sys::GhosttyRenderStateRowIteratorImpl>,
}
impl RowIterator {
    /// Allocate an iterator; populate it with `RenderState::row_iterator` before use.
    pub fn new() -> Result<Self> {
        let mut raw = std::ptr::null_mut();
        Error::check(unsafe {
            sys::ghostty_render_state_row_iterator_new(std::ptr::null(), &mut raw)
        })?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
        })
    }
}
impl Drop for RowIterator {
    fn drop(&mut self) {
        unsafe { sys::ghostty_render_state_row_iterator_free(self.raw.as_ptr()) }
    }
}
/// Populated lending row iterator, retaining exclusive access to its frame.
pub struct Rows<'a> {
    iterator: &'a mut RowIterator,
    _state: PhantomData<&'a mut RenderState>,
}
impl Rows<'_> {
    /// Advance to the next captured row.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<RenderRow<'_>> {
        unsafe { sys::ghostty_render_state_row_iterator_next(self.iterator.raw.as_ptr()) }
            .then_some(RenderRow {
                raw: self.iterator.raw,
                _state: PhantomData,
            })
    }
    /// Advance to the next dirty row, returning its capture index and row view.
    pub fn next_dirty(&mut self) -> Option<(u16, RenderRow<'_>)> {
        let mut y = 0;
        unsafe {
            sys::ghostty_render_state_row_iterator_next_dirty(self.iterator.raw.as_ptr(), &mut y)
        }
        .then_some((
            y,
            RenderRow {
                raw: self.iterator.raw,
                _state: PhantomData,
            },
        ))
    }
}
/// Current row view, invalidated only after this borrow ends.
pub struct RenderRow<'a> {
    raw: NonNull<sys::GhosttyRenderStateRowIteratorImpl>,
    _state: PhantomData<&'a mut RenderState>,
}
impl RenderRow<'_> {
    // SAFETY: callers use exactly the header's output type.
    unsafe fn get<T: Default>(&self, key: sys::GhosttyRenderStateRowData) -> Result<T> {
        let mut out = T::default();
        Error::check(unsafe {
            sys::ghostty_render_state_row_get(self.raw.as_ptr(), key, (&mut out as *mut T).cast())
        })?;
        Ok(out)
    }
    getters! {
        /// Whether the row changed.
        dirty: bool = GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY;
        /// Signed position relative to the viewport.
        viewport_y: i32 = GHOSTTY_RENDER_STATE_ROW_DATA_VIEWPORT_Y;
    }
    /// Copy the raw row value.
    pub fn raw(&self) -> Result<Row> {
        Ok(Row::from_raw(unsafe {
            self.get(sys::GHOSTTY_RENDER_STATE_ROW_DATA_RAW)?
        }))
    }
    /// Stable opaque identity of this row across updates.
    pub fn id(&self) -> Result<RowId> {
        let out: sys::GhosttyRenderStateRowId =
            unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_DATA_ID)? };
        Ok(RowId(out.bits))
    }
    /// Inclusive selected columns, if this row intersects the selection.
    pub fn selection(&self) -> Result<Option<RowSelection>> {
        let mut out = unsafe { ffi::sized::<sys::GhosttyRenderStateRowSelection>() };
        let code = unsafe {
            sys::ghostty_render_state_row_get(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_ROW_DATA_SELECTION,
                (&mut out as *mut sys::GhosttyRenderStateRowSelection).cast(),
            )
        };
        if code == sys::GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        Error::check(code)?;
        Ok(Some(RowSelection {
            start_x: out.start_x,
            end_x: out.end_x,
        }))
    }
    /// Borrow every raw cell in this row without per-cell calls.
    pub fn cells_raw(&self) -> Result<&[Cell]> {
        let out: sys::GhosttyCellsView =
            unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_DATA_CELLS_RAW)? };
        if out.len == 0 {
            return Ok(&[]);
        }
        // SAFETY: Cell is repr(transparent) over GhosttyCell. The row borrow prevents state updates.
        Ok(unsafe { std::slice::from_raw_parts(out.ptr.cast(), out.len) })
    }
    /// Set this row's dirty flag without modifying terminal dirty state.
    pub fn set_dirty(&mut self, dirty: bool) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_render_state_row_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_ROW_OPT_DIRTY,
                (&dirty as *const bool).cast(),
            )
        })
    }
    /// Populate reusable cell storage, borrowing this row while it is read.
    pub fn cells<'a>(&'a self, cells: &'a mut RowCells) -> Result<Cells<'a>> {
        let mut raw = cells.raw.as_ptr();
        Error::check(unsafe {
            sys::ghostty_render_state_row_get(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,
                (&mut raw as *mut sys::GhosttyRenderStateRowCells).cast(),
            )
        })?;
        Ok(Cells {
            cells,
            _state: PhantomData,
        })
    }
}
/// Reusable owning allocation for viewing one row's cells.
pub struct RowCells {
    raw: NonNull<sys::GhosttyRenderStateRowCellsImpl>,
}
impl RowCells {
    /// Allocate cell storage; populate using `RenderRow::cells`.
    pub fn new() -> Result<Self> {
        let mut raw = std::ptr::null_mut();
        Error::check(unsafe {
            sys::ghostty_render_state_row_cells_new(std::ptr::null(), &mut raw)
        })?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
        })
    }
}
impl Drop for RowCells {
    fn drop(&mut self) {
        unsafe { sys::ghostty_render_state_row_cells_free(self.raw.as_ptr()) }
    }
}
/// Lending view of populated row cells.
pub struct Cells<'a> {
    cells: &'a mut RowCells,
    _state: PhantomData<&'a RenderState>,
}
impl Cells<'_> {
    /// Advance to the next cell.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<RenderCell<'_>> {
        unsafe { sys::ghostty_render_state_row_cells_next(self.cells.raw.as_ptr()) }.then_some(
            RenderCell {
                raw: self.cells.raw,
                _state: PhantomData,
            },
        )
    }
    /// Select a zero-based column.
    pub fn select(&mut self, x: u16) -> Result<RenderCell<'_>> {
        Error::check(unsafe {
            sys::ghostty_render_state_row_cells_select(self.cells.raw.as_ptr(), x)
        })?;
        Ok(RenderCell {
            raw: self.cells.raw,
            _state: PhantomData,
        })
    }
}
/// Current cell view.
pub struct RenderCell<'a> {
    raw: NonNull<sys::GhosttyRenderStateRowCellsImpl>,
    _state: PhantomData<&'a RenderState>,
}
impl RenderCell<'_> {
    // SAFETY: callers use the exact output type associated with the key.
    unsafe fn get<T: Default>(&self, key: sys::GhosttyRenderStateRowCellsData) -> Result<T> {
        let mut out = T::default();
        Error::check(unsafe {
            sys::ghostty_render_state_row_cells_get(
                self.raw.as_ptr(),
                key,
                (&mut out as *mut T).cast(),
            )
        })?;
        Ok(out)
    }
    getters! {
        /// Number of codepoints, including the base codepoint.
        graphemes_len: u32 = GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_LEN;
        /// Whether the cell belongs to the selection.
        selected: bool = GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_SELECTED;
        /// Whether explicit styling is present.
        has_styling: bool = GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_HAS_STYLING;
    }
    /// Copy the raw cell value.
    pub fn raw(&self) -> Result<Cell> {
        Ok(Cell::from_raw(unsafe {
            self.get(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW)?
        }))
    }
    /// Resolved style, or the default style when no explicit styling exists.
    pub fn style(&self) -> Result<Style> {
        Ok(Style::from_raw(&unsafe {
            self.get(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE)?
        }))
    }
    /// Explicit resolved background, or `None` to use the frame default.
    pub fn background(&self) -> Result<Option<Rgb>> {
        optional(unsafe {
            self.get::<sys::GhosttyColorRgb>(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR)
        })
        .map(|v| v.map(Into::into))
    }
    /// Explicit resolved foreground, without bold color handling.
    pub fn foreground(&self) -> Result<Option<Rgb>> {
        optional(unsafe {
            self.get::<sys::GhosttyColorRgb>(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR)
        })
        .map(|v| v.map(Into::into))
    }
    /// Write as many codepoints as fit and return the total required length.
    pub fn graphemes_buf(&self, buffer: &mut [u32]) -> Result<usize> {
        let mut out = sys::GhosttyCodepointBuffer {
            ptr: buffer.as_mut_ptr(),
            cap: buffer.len(),
            len: 0,
        };
        let code = unsafe {
            sys::ghostty_render_state_row_cells_get(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_BUF,
                (&mut out as *mut sys::GhosttyCodepointBuffer).cast(),
            )
        };
        if code != sys::GHOSTTY_OUT_OF_SPACE {
            Error::check(code)?;
        }
        Ok(out.len)
    }
    /// Copy all grapheme codepoints.
    pub fn graphemes(&self) -> Result<Vec<u32>> {
        ffi::encode_vec(|ptr, cap, written| {
            let mut out = sys::GhosttyCodepointBuffer { ptr, cap, len: 0 };
            let code = unsafe {
                sys::ghostty_render_state_row_cells_get(
                    self.raw.as_ptr(),
                    sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_BUF,
                    (&mut out as *mut sys::GhosttyCodepointBuffer).cast(),
                )
            };
            *written = out.len;
            code
        })
    }
    /// Encode UTF-8 into a caller buffer; returns required length, writing nothing if too small.
    pub fn graphemes_utf8(&self, buffer: &mut [u8]) -> Result<usize> {
        let mut out = sys::GhosttyBuffer {
            ptr: buffer.as_mut_ptr(),
            cap: buffer.len(),
            len: 0,
        };
        let code = unsafe {
            sys::ghostty_render_state_row_cells_get(
                self.raw.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                (&mut out as *mut sys::GhosttyBuffer).cast(),
            )
        };
        if code != sys::GHOSTTY_OUT_OF_SPACE {
            Error::check(code)?;
        }
        Ok(out.len)
    }
    /// Copy the complete grapheme cluster as UTF-8.
    pub fn text(&self) -> Result<String> {
        let bytes = ffi::encode_vec(|ptr, cap, written| {
            let mut out = sys::GhosttyBuffer { ptr, cap, len: 0 };
            let code = unsafe {
                sys::ghostty_render_state_row_cells_get(
                    self.raw.as_ptr(),
                    sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                    (&mut out as *mut sys::GhosttyBuffer).cast(),
                )
            };
            *written = out.len;
            code
        })?;
        // The C API guarantees UTF-8 encoding of Unicode codepoints.
        Ok(String::from_utf8(bytes).expect("libghostty UTF-8"))
    }
}

/// Reusable allocation for render-state image placement iteration.
pub struct ImageIterator {
    raw: NonNull<sys::GhosttyRenderStateImageIteratorImpl>,
}
impl ImageIterator {
    /// Allocate an empty image iterator.
    pub fn new() -> Result<Self> {
        let mut raw = std::ptr::null_mut();
        Error::check(unsafe {
            sys::ghostty_render_state_image_iterator_new(std::ptr::null(), &mut raw)
        })?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
        })
    }
}
impl Drop for ImageIterator {
    fn drop(&mut self) {
        unsafe { sys::ghostty_render_state_image_iterator_free(self.raw.as_ptr()) }
    }
}
/// Populated lending image iterator; placements must be sorted by layer and z before drawing.
pub struct Images<'a> {
    iterator: &'a mut ImageIterator,
    _state: PhantomData<&'a RenderState>,
}
impl Images<'_> {
    /// Advance to the next captured image placement.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<RenderImage<'_>> {
        unsafe { sys::ghostty_render_state_image_iterator_next(self.iterator.raw.as_ptr()) }
            .then_some(RenderImage {
                raw: self.iterator.raw,
                _state: PhantomData,
            })
    }
}
/// A render-state image placement, independent of terminal image storage.
pub struct RenderImage<'a> {
    raw: NonNull<sys::GhosttyRenderStateImageIteratorImpl>,
    _state: PhantomData<&'a RenderState>,
}
impl RenderImage<'_> {
    // SAFETY: callers use the documented type for each key.
    unsafe fn get<T: Default>(&self, key: sys::GhosttyRenderStateImageData) -> Result<T> {
        let mut out = T::default();
        Error::check(unsafe {
            sys::ghostty_render_state_image_get(self.raw.as_ptr(), key, (&mut out as *mut T).cast())
        })?;
        Ok(out)
    }
    getters! {
        /// Image ID within its protocol.
        image_id: u32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_IMAGE_ID;
        /// Generation of the image pixels.
        image_generation: u64 = GHOSTTY_RENDER_STATE_IMAGE_DATA_IMAGE_GENERATION;
        /// Z-index within its drawing layer.
        z: i32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_Z;
        /// Left column relative to the viewport.
        viewport_col: i32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_VIEWPORT_COL;
        /// Top row relative to the viewport.
        viewport_row: i32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_VIEWPORT_ROW;
        /// Occupied columns.
        grid_cols: u32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_GRID_COLS;
        /// Occupied rows.
        grid_rows: u32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_GRID_ROWS;
        /// Pixel x offset from the first cell.
        cell_offset_x: u32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_CELL_OFFSET_X;
        /// Pixel y offset from the first cell.
        cell_offset_y: u32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_CELL_OFFSET_Y;
        /// Drawn width in pixels.
        pixel_width: u32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_PIXEL_WIDTH;
        /// Drawn height in pixels.
        pixel_height: u32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_PIXEL_HEIGHT;
        /// Source rectangle left edge.
        source_x: u32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_X;
        /// Source rectangle top edge.
        source_y: u32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_Y;
        /// Source rectangle width.
        source_width: u32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_WIDTH;
        /// Source rectangle height.
        source_height: u32 = GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_HEIGHT;
    }
    /// Protocol whose storage holds this image's pixels.
    pub fn protocol(&self) -> Result<ImageProtocol> {
        Ok(ImageProtocol::from_lib(unsafe {
            self.get(sys::GHOSTTY_RENDER_STATE_IMAGE_DATA_PROTOCOL)?
        }))
    }
    /// Drawing layer.
    pub fn layer(&self) -> Result<ImageLayer> {
        Ok(ImageLayer::from_lib(unsafe {
            self.get(sys::GHOSTTY_RENDER_STATE_IMAGE_DATA_LAYER)?
        }))
    }
    /// Kitty placement ID, unavailable for other protocols.
    pub fn kitty_placement_id(&self) -> Result<Option<u32>> {
        optional(unsafe { self.get(sys::GHOSTTY_RENDER_STATE_IMAGE_DATA_KITTY_PLACEMENT_ID) })
    }
    /// Kitty placement kind, unavailable for other protocols.
    pub fn kitty_kind(&self) -> Result<Option<KittyPlacementKind>> {
        optional(unsafe { self.get(sys::GHOSTTY_RENDER_STATE_IMAGE_DATA_KITTY_KIND) })
            .map(|value| value.map(KittyPlacementKind::from_lib))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{StyleColor, StyleFlags};
    #[test]
    fn styled_frame_and_reusable_cells() -> Result<()> {
        let mut terminal = Terminal::new(40, 5)?;
        terminal.vt_write(b"Hello, \x1b[1;32mworld\x1b[0m!\r\n\x1b[4munderlined\x1b[0m text\r\n\x1b[38;2;255;128;0morange\x1b[0m\r\n");
        terminal.set_selection(
            crate::point::Point::active(0, 1),
            crate::point::Point::active(9, 1),
            false,
        )?;
        let mut state = RenderState::new()?;
        state.update(&terminal)?;
        assert_eq!((state.cols()?, state.rows()?), (40, 5));
        assert_eq!(state.dirty()?, Dirty::Full);
        assert_eq!(state.cursor()?.viewport.unwrap().y, 3);
        let colors = state.colors()?;
        let mut iterator = RowIterator::new()?;
        let mut storage = RowCells::new()?;
        let mut rows = state.row_iterator(&mut iterator)?;
        let row = rows.next().unwrap();
        assert_eq!(row.cells_raw()?.len(), 40);
        let mut cells = row.cells(&mut storage)?;
        assert_eq!(cells.select(0)?.text()?, "H");
        let world = cells.select(7)?;
        assert_eq!(world.text()?, "w");
        assert!(world.style()?.flags.contains(StyleFlags::BOLD));
        assert_eq!(world.style()?.foreground, StyleColor::Palette(2));
        assert_eq!(world.foreground()?, Some(colors.palette[2]));
        let row = rows.next().unwrap();
        assert_eq!(
            row.selection()?,
            Some(RowSelection {
                start_x: 0,
                end_x: 9
            })
        );
        let mut cells = row.cells(&mut storage)?;
        assert!(cells.select(0)?.selected()?);
        assert!(!cells.select(10)?.selected()?);
        assert_ne!(
            cells.select(0)?.style()?.underline,
            crate::style::Underline::None
        );
        let row = rows.next().unwrap();
        let mut cells = row.cells(&mut storage)?;
        assert_eq!(
            cells.select(0)?.foreground()?,
            Some(Rgb {
                r: 255,
                g: 128,
                b: 0
            })
        );
        Ok(())
    }
    #[test]
    fn dirty_tracking_split_updates_and_iterator_reuse() -> Result<()> {
        let mut terminal = Terminal::new(8, 3)?;
        let mut state = RenderState::new()?;
        let mut iterator = RowIterator::new()?;
        state.update(&terminal)?;
        let first_id = state.row_iterator(&mut iterator)?.next().unwrap().id()?;
        state.clean()?;
        state.update(&terminal)?;
        assert_eq!(state.dirty()?, Dirty::Clean);
        assert!(state.row_iterator(&mut iterator)?.next_dirty().is_none());
        terminal.vt_write(b"x");
        state.begin_update(&terminal)?;
        assert_eq!(state.cols(), Err(Error::InvalidValue));
        // End phase is explicitly independent of the terminal.
        terminal.vt_write(b"y");
        state.end_update()?;
        assert_ne!(state.dirty()?, Dirty::Clean);
        let mut rows = state.row_iterator(&mut iterator)?;
        let (y, mut row) = rows.next_dirty().unwrap();
        assert_eq!(y, 0);
        assert_eq!(row.id()?, first_id);
        row.set_dirty(false)?;
        assert!(!row.dirty()?);
        state.clean()?;
        state.update(&terminal)?;
        assert_ne!(state.dirty()?, Dirty::Clean);
        Ok(())
    }
    #[test]
    fn grapheme_buffers_and_overscan() -> Result<()> {
        let mut terminal = Terminal::new(8, 2)?;
        terminal.vt_write("a\u{301}".as_bytes());
        let mut state = RenderState::new()?;
        state.set_overscan(Overscan { above: 2, below: 2 })?;
        state.update(&terminal)?;
        assert_eq!(state.overscan_request()?.above, 2);
        assert_eq!(state.overscan()?.below, 0);
        let mut iterator = RowIterator::new()?;
        let mut storage = RowCells::new()?;
        let mut rows = state.row_iterator(&mut iterator)?;
        let row = rows.next().unwrap();
        let mut cells = row.cells(&mut storage)?;
        let cell = cells.select(0)?;
        assert_eq!(cell.graphemes()?, vec![97, 769]);
        assert_eq!(cell.text()?, "a\u{301}");
        let mut codepoints = [0];
        assert_eq!(cell.graphemes_buf(&mut codepoints)?, 2);
        assert_eq!(codepoints, [97]);
        let mut tiny = [0];
        assert_eq!(cell.graphemes_utf8(&mut tiny)?, 3);
        Ok(())
    }
}
