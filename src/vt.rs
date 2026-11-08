//! Safe wrapper over the parts of libghostty-vt this app uses: the terminal
//! state machine, the render-state snapshot, input encoders, selection and
//! paste. Everything here is single-threaded and owned by the UI thread.

use core::ffi::c_void;
use core::{mem, ptr, slice};
use std::ops::Range;

use ghostty_sys::*;

/// Scrollback budget per terminal. Cells take 8 bytes, so this is about
/// 290,000 rows at 200 columns; pages are allocated as history grows.
const SCROLLBACK_MAX_BYTES: usize = 500_000_000;
/// Kitty graphics storage per screen, Ghostty's `image-storage-limit`
/// default.
const KITTY_IMAGE_STORAGE_LIMIT: u64 = 320_000_000;
/// Character that places a Kitty graphics image with text (virtual
/// placements).
const KITTY_PLACEHOLDER: char = '\u{10EEEE}';

/// Decode Kitty graphics PNG payloads with the `image` crate. libghostty
/// ships no decoder and rejects PNG images until one is installed.
/// Process-global: call once at startup, before any terminal exists.
pub fn install_png_decoder() {
    let decode: unsafe extern "C" fn(
        *mut c_void,
        *const GhosttyAllocator,
        *const u8,
        usize,
        *mut GhosttySysImage,
    ) -> bool = decode_png;
    unsafe { ghostty_sys_set(GHOSTTY_SYS_OPT_DECODE_PNG, decode as *const c_void) };
}

unsafe extern "C" fn decode_png(
    _userdata: *mut c_void,
    allocator: *const GhosttyAllocator,
    data: *const u8,
    len: usize,
    out: *mut GhosttySysImage,
) -> bool {
    let bytes = unsafe { slice::from_raw_parts(data, len) };
    let Ok(image) = image::load_from_memory_with_format(bytes, image::ImageFormat::Png) else {
        return false;
    };
    let image = image.into_rgba8();
    let pixels = image.as_raw();
    // The allocator caps one image's memory: NULL means too large.
    let buffer = unsafe { ghostty_alloc(allocator, pixels.len()) };
    if buffer.is_null() {
        return false;
    }
    unsafe {
        ptr::copy_nonoverlapping(pixels.as_ptr(), buffer, pixels.len());
        *out = GhosttySysImage {
            width: image.width(),
            height: image.height(),
            data: buffer,
            data_len: pixels.len(),
        };
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KittyImageFormat {
    Rgb,
    Rgba,
    GrayAlpha,
    Gray,
}

/// Decoded pixels of a Kitty graphics image (its current animation frame),
/// rows top to bottom without padding.
pub struct KittyImage<'a> {
    pub width: u32,
    pub height: u32,
    pub format: KittyImageFormat,
    pub pixels: &'a [u8],
}

/// A zeroed "sized struct" (`GHOSTTY_INIT_SIZED`) with its `size` field set.
macro_rules! sized {
    ($ty:ty) => {{
        let mut value = <$ty>::default();
        value.size = mem::size_of::<$ty>();
        value
    }};
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<GhosttyColorRgb> for Rgb {
    fn from(c: GhosttyColorRgb) -> Self {
        Self { r: c.r, g: c.g, b: c.b }
    }
}

impl From<Rgb> for GhosttyColorRgb {
    fn from(c: Rgb) -> Self {
        Self { r: c.r, g: c.g, b: c.b }
    }
}

/// Default colors: what the terminal shows until a program overrides them
/// (OSC 4/10/11/12) and returns to after a reset.
pub struct TerminalColors {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub palette: [Rgb; 256],
}

/// Where the viewport sits in the scrollable area (scrollback plus active
/// screen), in rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scrollbar {
    pub total: u64,
    /// First visible row.
    pub offset: u64,
    /// Visible rows.
    pub len: u64,
}

impl Scrollbar {
    pub fn scrollable(&self) -> bool {
        self.total > self.len
    }

    /// Offset of the viewport when scrolled to the bottom.
    pub fn max_offset(&self) -> u64 {
        self.total.saturating_sub(self.len)
    }
}

/// Side effects the VT stream produced during the last write. Shared with
/// the C callbacks through the terminal's userdata pointer.
#[derive(Default)]
struct Effects {
    pty_out: Vec<u8>,
    title_changed: bool,
    size: GhosttySizeReportSize,
}

pub struct Terminal {
    raw: GhosttyTerminal,
    /// Owned via `Box::into_raw`; freed in `Drop`. Kept raw so the callbacks
    /// can mutate it while `raw` is borrowed by a C call.
    effects: *mut Effects,
}

unsafe extern "C" fn on_write_pty(
    _terminal: GhosttyTerminal,
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) {
    let effects = unsafe { &mut *(userdata as *mut Effects) };
    if len > 0 {
        effects
            .pty_out
            .extend_from_slice(unsafe { slice::from_raw_parts(data, len) });
    }
}

unsafe extern "C" fn on_title_changed(_terminal: GhosttyTerminal, userdata: *mut c_void) {
    unsafe { (*(userdata as *mut Effects)).title_changed = true };
}

unsafe extern "C" fn on_size(
    _terminal: GhosttyTerminal,
    userdata: *mut c_void,
    out: *mut GhosttySizeReportSize,
) -> bool {
    unsafe { *out = (*(userdata as *mut Effects)).size };
    true
}

impl Terminal {
    pub fn new(cols: u16, rows: u16) -> Self {
        let mut raw: GhosttyTerminal = ptr::null_mut();
        let result = unsafe { ghostty_terminal_new(ptr::null(), &mut raw, cols, rows) };
        assert_eq!(result, GHOSTTY_SUCCESS, "ghostty_terminal_new failed");

        let effects = Box::into_raw(Box::new(Effects::default()));
        let write_pty: unsafe extern "C" fn(GhosttyTerminal, *mut c_void, *const u8, usize) =
            on_write_pty;
        let title_changed: unsafe extern "C" fn(GhosttyTerminal, *mut c_void) = on_title_changed;
        let size: unsafe extern "C" fn(
            GhosttyTerminal,
            *mut c_void,
            *mut GhosttySizeReportSize,
        ) -> bool = on_size;
        unsafe {
            ghostty_terminal_set(raw, GHOSTTY_TERMINAL_OPT_USERDATA, effects as *const c_void);
            ghostty_terminal_set(raw, GHOSTTY_TERMINAL_OPT_WRITE_PTY, write_pty as *const c_void);
            ghostty_terminal_set(
                raw,
                GHOSTTY_TERMINAL_OPT_TITLE_CHANGED,
                title_changed as *const c_void,
            );
            ghostty_terminal_set(raw, GHOSTTY_TERMINAL_OPT_SIZE, size as *const c_void);
            // libghostty's default keeps a single page of history, a few
            // hundred rows.
            ghostty_terminal_set(
                raw,
                GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,
                &SCROLLBACK_MAX_BYTES as *const usize as *const c_void,
            );
            // ConPTY repaints its own scrollback-less screen after a resize;
            // pulling history rows back into view would make the two disagree
            // and the repaint would overwrite them.
            #[cfg(windows)]
            ghostty_terminal_set(
                raw,
                GHOSTTY_TERMINAL_OPT_RESIZE_PULL_SCROLLBACK,
                &false as *const bool as *const c_void,
            );
            // Grapheme clustering (mode 2027) on by default, as in Ghostty:
            // emoji sequences and modifiers occupy one cell instead of several.
            let grapheme_cluster = GhosttyTerminalModeConfig {
                mode: 2027,
                value: true,
            };
            ghostty_terminal_set(
                raw,
                GHOSTTY_TERMINAL_OPT_MODE_DEFAULT,
                &grapheme_cluster as *const _ as *const c_void,
            );
            // Kitty graphics, configured like Ghostty: its storage limit,
            // and every transmission medium, temporary files only from the
            // temp directory programs started here inherit.
            ghostty_terminal_set(
                raw,
                GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_STORAGE_LIMIT,
                &KITTY_IMAGE_STORAGE_LIMIT as *const u64 as *const c_void,
            );
            for option in [
                GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_MEDIUM_FILE,
                GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_MEDIUM_SHARED_MEM,
            ] {
                ghostty_terminal_set(raw, option, &true as *const bool as *const c_void);
            }
            let temp_dir = std::env::temp_dir();
            let temp_dir = temp_dir.to_string_lossy();
            let temp_dir = GhosttyString {
                ptr: temp_dir.as_ptr(),
                len: temp_dir.len(),
            };
            ghostty_terminal_set(
                raw,
                GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_MEDIUM_TEMP_FILE,
                &temp_dir as *const GhosttyString as *const c_void,
            );
        }
        Self { raw, effects }
    }

    /// The active screen's Kitty graphics storage. Borrowed: valid until the
    /// terminal is next modified.
    fn kitty_graphics(&self) -> Option<GhosttyKittyGraphics> {
        let mut graphics: GhosttyKittyGraphics = ptr::null_mut();
        let result = unsafe {
            ghostty_terminal_get(
                self.raw,
                GHOSTTY_TERMINAL_DATA_KITTY_GRAPHICS,
                &mut graphics as *mut GhosttyKittyGraphics as *mut c_void,
            )
        };
        (result == GHOSTTY_SUCCESS && !graphics.is_null()).then_some(graphics)
    }

    /// The Kitty graphics image `id` on the active screen, if it exists and
    /// its pixels have arrived.
    pub fn kitty_image(&self, id: u32) -> Option<KittyImage<'_>> {
        let graphics = self.kitty_graphics()?;
        let image = unsafe { ghostty_kitty_graphics_image(graphics, id) };
        if image.is_null() {
            return None;
        }
        let mut width = 0u32;
        let mut height = 0u32;
        let mut format: GhosttyKittyImageFormat = GHOSTTY_KITTY_IMAGE_FORMAT_RGBA;
        let mut pixels = GhosttyString {
            ptr: ptr::null(),
            len: 0,
        };
        let keys = [
            GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_WIDTH,
            GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_HEIGHT,
            GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_FORMAT,
            GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_PIXELS,
        ];
        let mut values = [
            &mut width as *mut u32 as *mut c_void,
            &mut height as *mut u32 as *mut c_void,
            &mut format as *mut GhosttyKittyImageFormat as *mut c_void,
            &mut pixels as *mut GhosttyString as *mut c_void,
        ];
        let result = unsafe {
            ghostty_kitty_graphics_image_get_multi(
                image,
                keys.len(),
                keys.as_ptr(),
                values.as_mut_ptr(),
                ptr::null_mut(),
            )
        };
        if result != GHOSTTY_SUCCESS || pixels.ptr.is_null() {
            return None;
        }
        let format = match format {
            GHOSTTY_KITTY_IMAGE_FORMAT_RGB => KittyImageFormat::Rgb,
            GHOSTTY_KITTY_IMAGE_FORMAT_RGBA => KittyImageFormat::Rgba,
            GHOSTTY_KITTY_IMAGE_FORMAT_GRAY_ALPHA => KittyImageFormat::GrayAlpha,
            GHOSTTY_KITTY_IMAGE_FORMAT_GRAY => KittyImageFormat::Gray,
            _ => return None,
        };
        Some(KittyImage {
            width,
            height,
            format,
            // SAFETY: owned by the terminal and valid until it is next
            // modified, which needs `&mut self`.
            pixels: unsafe { slice::from_raw_parts(pixels.ptr, pixels.len) },
        })
    }

    /// Advance playing Kitty graphics animations to `now_ms` (any clock
    /// with a fixed start). Returns when the next frame is due on that
    /// clock, or `None` if nothing is playing.
    pub fn tick_animations(&mut self, now_ms: u64) -> Option<u64> {
        let graphics = self.kitty_graphics()?;
        let mut next_ms = 0u64;
        let result =
            unsafe { ghostty_kitty_graphics_animation_tick(graphics, now_ms, &mut next_ms) };
        (result == GHOSTTY_SUCCESS).then_some(next_ms)
    }

    fn effects(&mut self) -> &mut Effects {
        // SAFETY: only reachable while no C call holding the terminal is running.
        unsafe { &mut *self.effects }
    }

    /// Feed program output through the VT parser.
    pub fn vt_write(&mut self, data: &[u8]) {
        unsafe { ghostty_terminal_vt_write(self.raw, data.as_ptr(), data.len()) };
    }

    /// Bytes the terminal wants to send back to the program (query replies,
    /// paste data). Drains the buffer.
    pub fn take_pty_output(&mut self) -> Vec<u8> {
        mem::take(&mut self.effects().pty_out)
    }

    /// The new title, if a title change happened since the last call.
    pub fn take_title_change(&mut self) -> Option<String> {
        if !mem::take(&mut self.effects().title_changed) {
            return None;
        }
        let mut title = GhosttyString::default();
        let result = unsafe {
            ghostty_terminal_get(
                self.raw,
                GHOSTTY_TERMINAL_DATA_TITLE,
                &mut title as *mut _ as *mut c_void,
            )
        };
        if result != GHOSTTY_SUCCESS || title.ptr.is_null() {
            return Some(String::new());
        }
        let bytes = unsafe { slice::from_raw_parts(title.ptr, title.len) };
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    pub fn set_colors(&mut self, colors: &TerminalColors) {
        let foreground = GhosttyColorRgb::from(colors.foreground);
        let background = GhosttyColorRgb::from(colors.background);
        let cursor = GhosttyColorRgb::from(colors.cursor);
        let palette = colors.palette.map(GhosttyColorRgb::from);
        let options = [
            (GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND, &foreground as *const _ as *const c_void),
            (GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND, &background as *const _ as *const c_void),
            (GHOSTTY_TERMINAL_OPT_COLOR_CURSOR, &cursor as *const _ as *const c_void),
            (GHOSTTY_TERMINAL_OPT_COLOR_PALETTE, palette.as_ptr() as *const c_void),
        ];
        for (option, value) in options {
            unsafe { ghostty_terminal_set(self.raw, option, value) };
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_width: u32, cell_height: u32) {
        self.effects().size = GhosttySizeReportSize {
            rows,
            columns: cols,
            cell_width,
            cell_height,
        };
        unsafe { ghostty_terminal_resize(self.raw, cols, rows, cell_width, cell_height) };
    }

    fn get<T: Default>(&self, data: GhosttyTerminalData) -> Option<T> {
        let mut out = T::default();
        let result =
            unsafe { ghostty_terminal_get(self.raw, data, &mut out as *mut T as *mut c_void) };
        (result == GHOSTTY_SUCCESS).then_some(out)
    }

    /// Current value of a DEC private mode (e.g. 1004 focus events).
    pub fn dec_mode(&self, mode: u16) -> bool {
        let mut config = GhosttyTerminalModeConfig {
            mode: mode & 0x7FFF,
            value: false,
        };
        let result = unsafe {
            ghostty_terminal_get(
                self.raw,
                GHOSTTY_TERMINAL_DATA_MODE,
                &mut config as *mut _ as *mut c_void,
            )
        };
        result == GHOSTTY_SUCCESS && config.value
    }

    pub fn mouse_tracking(&self) -> bool {
        self.get::<bool>(GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING)
            .unwrap_or(false)
    }

    pub fn alternate_screen(&self) -> bool {
        self.get::<GhosttyTerminalScreen>(GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN)
            == Some(GHOSTTY_TERMINAL_SCREEN_ALTERNATE)
    }

    pub fn kitty_keyboard_flags(&self) -> u8 {
        self.get::<u8>(GHOSTTY_TERMINAL_DATA_KITTY_KEYBOARD_FLAGS)
            .unwrap_or(0)
    }

    pub fn select_all(&mut self) {
        let mut selection = GhosttySelection::default();
        if unsafe { ghostty_terminal_select_all(self.raw, &mut selection) } == GHOSTTY_SUCCESS {
            self.set_selection(Some(&selection));
        }
    }

    /// Scroll the viewport by `delta` rows (negative = up into history).
    pub fn scroll_by(&mut self, delta: isize) {
        let scroll = GhosttyTerminalScrollViewport {
            tag: GHOSTTY_TERMINAL_SCROLL_VIEWPORT_DELTA,
            value: GhosttyTerminalScrollViewportValue { delta },
        };
        unsafe { ghostty_terminal_scroll_viewport(self.raw, scroll) };
    }

    /// Scroll so that scrollback row `row` (same row space as
    /// [`Scrollbar::offset`]) is the first visible row.
    pub fn scroll_to_row(&mut self, row: u64) {
        let scroll = GhosttyTerminalScrollViewport {
            tag: GHOSTTY_TERMINAL_SCROLL_VIEWPORT_ROW,
            value: GhosttyTerminalScrollViewportValue { row: row as usize },
        };
        unsafe { ghostty_terminal_scroll_viewport(self.raw, scroll) };
    }

    pub fn scrollbar(&self) -> Scrollbar {
        self.get::<GhosttyTerminalScrollbar>(GHOSTTY_TERMINAL_DATA_SCROLLBAR)
            .map_or(Scrollbar::default(), |bar| Scrollbar {
                total: bar.total,
                offset: bar.offset,
                len: bar.len,
            })
    }

    pub fn scroll_to_bottom(&mut self) {
        let scroll = GhosttyTerminalScrollViewport {
            tag: GHOSTTY_TERMINAL_SCROLL_VIEWPORT_BOTTOM,
            value: GhosttyTerminalScrollViewportValue { delta: 0 },
        };
        unsafe { ghostty_terminal_scroll_viewport(self.raw, scroll) };
    }

    /// Grid reference for a viewport cell. Valid until the next mutating call.
    pub fn viewport_ref(&self, x: u16, y: u16) -> Option<GhosttyGridRef> {
        let point = GhosttyPoint {
            tag: GHOSTTY_POINT_TAG_VIEWPORT,
            value: GhosttyPointValue {
                coordinate: GhosttyPointCoordinate { x, y: y as u32 },
            },
        };
        let mut out = GhosttyGridRef::default();
        let result = unsafe { ghostty_terminal_grid_ref(self.raw, point, &mut out) };
        (result == GHOSTTY_SUCCESS).then_some(out)
    }

    pub fn set_selection(&mut self, selection: Option<&GhosttySelection>) {
        let value = selection.map_or(ptr::null(), |s| s as *const _ as *const c_void);
        unsafe { ghostty_terminal_set(self.raw, GHOSTTY_TERMINAL_OPT_SELECTION, value) };
    }

    /// Plain text of the current selection, unwrapped and trimmed the way
    /// Ghostty copies to the clipboard.
    pub fn selection_text(&self) -> Option<String> {
        let selection = self.get::<GhosttySelection>(GHOSTTY_TERMINAL_DATA_SELECTION)?;
        unsafe {
            let mut formatter: GhosttyFormatter = ptr::null_mut();
            if ghostty_formatter_terminal_new(ptr::null(), &mut formatter, self.raw)
                != GHOSTTY_SUCCESS
            {
                return None;
            }
            let yes = true;
            let emit: GhosttyFormatterFormat = GHOSTTY_FORMATTER_FORMAT_PLAIN;
            ghostty_formatter_set(
                formatter,
                GHOSTTY_FORMATTER_OPT_EMIT,
                &emit as *const _ as *const c_void,
            );
            ghostty_formatter_set(
                formatter,
                GHOSTTY_FORMATTER_OPT_UNWRAP,
                &yes as *const _ as *const c_void,
            );
            ghostty_formatter_set(
                formatter,
                GHOSTTY_FORMATTER_OPT_TRIM,
                &yes as *const _ as *const c_void,
            );
            ghostty_formatter_set(
                formatter,
                GHOSTTY_FORMATTER_OPT_SELECTION,
                &selection as *const _ as *const c_void,
            );
            let mut buf: *mut u8 = ptr::null_mut();
            let mut len = 0usize;
            let result = ghostty_formatter_format_alloc(formatter, ptr::null(), &mut buf, &mut len);
            ghostty_formatter_free(formatter);
            if result != GHOSTTY_SUCCESS {
                return None;
            }
            let text = if buf.is_null() {
                String::new()
            } else {
                String::from_utf8_lossy(slice::from_raw_parts(buf, len)).into_owned()
            };
            ghostty_free(ptr::null(), buf, len);
            Some(text)
        }
    }

    /// Paste text according to the terminal's modes (bracketed paste, Kitty
    /// paste events). Output goes to `take_pty_output`. Returns `false` if
    /// the text is unsafe and `allow_unsafe` was not set.
    pub fn paste(&mut self, text: &str, allow_unsafe: bool) -> bool {
        unsafe extern "C" fn read(
            userdata: *mut c_void,
            mime: GhosttyString,
            writer: GhosttyWriter,
        ) -> bool {
            let text = unsafe { &*(userdata as *const &str) };
            let mime = unsafe { slice::from_raw_parts(mime.ptr, mime.len) };
            if mime != b"text/plain" {
                return false;
            }
            match writer.write {
                Some(write) => unsafe { write(writer.userdata, text.as_ptr(), text.len()) },
                None => false,
            }
        }

        let mime = b"text/plain";
        let mimes = [GhosttyString {
            ptr: mime.as_ptr(),
            len: mime.len(),
        }];
        let text_ref: &str = text;
        let paste = GhosttyPaste {
            size: mem::size_of::<GhosttyPaste>(),
            location: GHOSTTY_CLIPBOARD_LOCATION_STANDARD,
            source: GHOSTTY_PASTE_SOURCE_CLIPBOARD,
            mimes: mimes.as_ptr(),
            mimes_len: mimes.len(),
            reader: GhosttyMimeReader {
                read: Some(read),
                userdata: &text_ref as *const &str as *mut c_void,
            },
            allow_unsafe,
        };
        let mut pasted = false;
        let result = unsafe { ghostty_terminal_paste(self.raw, &paste, &mut pasted) };
        result != GHOSTTY_REJECTED
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        unsafe {
            ghostty_terminal_free(self.raw);
            drop(Box::from_raw(self.effects));
        }
    }
}

// ---------------------------------------------------------------------------
// Render snapshot
// ---------------------------------------------------------------------------

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
}

#[derive(Default)]
pub struct Line {
    pub text: String,
    pub cells: Vec<Cell>,
}

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
}

impl RenderState {
    pub fn new() -> Self {
        let mut raw = ptr::null_mut();
        let mut rows = ptr::null_mut();
        let mut cells = ptr::null_mut();
        let mut images = ptr::null_mut();
        unsafe {
            assert_eq!(ghostty_render_state_new(ptr::null(), &mut raw), GHOSTTY_SUCCESS);
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
        unsafe { ghostty_render_state_row_cells_get(self.cells, data, out as *mut T as *mut c_void) }
    }

    /// Capture the terminal's visible state into `frame`.
    pub fn snapshot(&mut self, terminal: &Terminal, frame: &mut Frame) {
        unsafe {
            ghostty_render_state_update(self.raw, terminal.raw);
        }

        let mut colors = sized!(GhosttyRenderStateColors);
        unsafe {
            ghostty_render_state_get(
                self.raw,
                GHOSTTY_RENDER_STATE_DATA_COLORS,
                &mut colors as *mut _ as *mut c_void,
            );
        }
        let background = Rgb::from(colors.background);
        let foreground = Rgb::from(colors.foreground);
        let resolve = |color: &GhosttyStyleColor| -> Option<Rgb> {
            match color.tag as GhosttyStyleColorTag {
                GHOSTTY_STYLE_COLOR_RGB => Some(unsafe { color.value.rgb }.into()),
                GHOSTTY_STYLE_COLOR_PALETTE => {
                    Some(colors.palette[unsafe { color.value.palette } as usize].into())
                }
                _ => None,
            }
        };

        let mut cursor = sized!(GhosttyRenderStateCursor);
        unsafe {
            ghostty_render_state_get(
                self.raw,
                GHOSTTY_RENDER_STATE_DATA_CURSOR,
                &mut cursor as *mut _ as *mut c_void,
            );
        }

        let mut cols: u16 = 0;
        let mut rows: u16 = 0;
        unsafe {
            ghostty_render_state_get(
                self.raw,
                GHOSTTY_RENDER_STATE_DATA_COLS,
                &mut cols as *mut _ as *mut c_void,
            );
            ghostty_render_state_get(
                self.raw,
                GHOSTTY_RENDER_STATE_DATA_ROWS,
                &mut rows as *mut _ as *mut c_void,
            );
            ghostty_render_state_get(
                self.raw,
                GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
                &mut self.rows as *mut _ as *mut c_void,
            );
        }

        frame.cols = cols;
        frame.rows = rows;
        frame.background = background;
        frame.foreground = foreground;
        frame.lines.resize_with(rows as usize, Line::default);

        let mut grapheme = [0u8; 64];
        let mut row_index = 0usize;
        while row_index < rows as usize && unsafe { ghostty_render_state_row_iterator_next(self.rows) }
        {
            let line = &mut frame.lines[row_index];
            line.text.clear();
            line.cells.clear();

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
                let mut cell = Cell::default();

                let mut raw: GhosttyCell = 0;
                self.cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW, &mut raw);
                let mut wide: GhosttyCellWide = GHOSTTY_CELL_WIDE_NARROW;
                unsafe {
                    ghostty_cell_get(raw, GHOSTTY_CELL_DATA_WIDE, &mut wide as *mut _ as *mut c_void)
                };
                cell.wide = wide == GHOSTTY_CELL_WIDE_WIDE;
                let spacer =
                    wide == GHOSTTY_CELL_WIDE_SPACER_TAIL || wide == GHOSTTY_CELL_WIDE_SPACER_HEAD;

                if !spacer {
                    let start = line.text.len() as u32;
                    let mut buf = GhosttyBuffer {
                        ptr: grapheme.as_mut_ptr(),
                        cap: grapheme.len(),
                        len: 0,
                    };
                    let result =
                        self.cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &mut buf);
                    if result == GHOSTTY_SUCCESS {
                        push_utf8(&mut line.text, &grapheme[..buf.len]);
                    } else if result == GHOSTTY_OUT_OF_SPACE {
                        let mut big = vec![0u8; buf.len];
                        buf = GhosttyBuffer {
                            ptr: big.as_mut_ptr(),
                            cap: big.len(),
                            len: 0,
                        };
                        if self.cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8, &mut buf)
                            == GHOSTTY_SUCCESS
                        {
                            push_utf8(&mut line.text, &big[..buf.len]);
                        }
                    }
                    // Kitty graphics placeholders stand for the image the
                    // render state reports; they have no glyph of their own.
                    if line.text[start as usize..].starts_with(KITTY_PLACEHOLDER) {
                        line.text.truncate(start as usize);
                    }
                    cell.text = start..line.text.len() as u32;
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
                cell.underline = match (flags & GHOSTTY_STYLE_UNDERLINE_MASK)
                    >> GHOSTTY_STYLE_UNDERLINE_SHIFT
                {
                    0 => Underline::None,
                    1 => Underline::Single,
                    2 => Underline::Double,
                    3 => Underline::Curly,
                    4 => Underline::Dotted,
                    _ => Underline::Dashed,
                };
                cell.underline_color = resolve(&style.underline_color);

                let mut rgb = GhosttyColorRgb::default();
                let fg = (self.cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR, &mut rgb)
                    == GHOSTTY_SUCCESS)
                    .then(|| Rgb::from(rgb));
                let bg = (self.cell_get(GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR, &mut rgb)
                    == GHOSTTY_SUCCESS)
                    .then(|| Rgb::from(rgb));

                let inverse = flags & GHOSTTY_STYLE_FLAG_INVERSE != 0;
                let in_selection =
                    selected && col >= selection.start_x && col <= selection.end_x;
                let (mut fg, mut bg) = (fg.unwrap_or(foreground), bg);
                // Inverse video and selection both swap foreground and background.
                if inverse != in_selection {
                    let new_fg = bg.unwrap_or(background);
                    bg = Some(fg);
                    fg = new_fg;
                }
                cell.fg = fg;
                cell.bg = bg;

                line.cells.push(cell);
                col += 1;
            }
            row_index += 1;
        }
        frame.lines.truncate(row_index);
        frame.rows = row_index as u16;

        frame.cursor = (cursor.visible && cursor.viewport_has_value).then(|| {
            let mut x = cursor.viewport_x;
            if cursor.wide_tail && x > 0 {
                x -= 1;
            }
            let y = cursor.viewport_y;
            let wide = frame
                .lines
                .get(y as usize)
                .and_then(|line| line.cells.get(x as usize))
                .is_some_and(|cell| cell.wide);
            Cursor {
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
                    foreground
                },
            }
        });

        self.read_images(&mut frame.images);

        unsafe { ghostty_render_state_clean(self.raw) };
    }
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

// ---------------------------------------------------------------------------
// Input encoders
// ---------------------------------------------------------------------------

pub struct KeyEncoder {
    encoder: GhosttyKeyEncoder,
    event: GhosttyKeyEvent,
}

pub struct KeyInput<'a> {
    pub action: GhosttyKeyAction,
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
            assert_eq!(ghostty_key_encoder_new(ptr::null(), &mut encoder), GHOSTTY_SUCCESS);
            assert_eq!(ghostty_key_event_new(ptr::null(), &mut event), GHOSTTY_SUCCESS);
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
            ghostty_key_event_set_action(self.event, input.action);
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

pub struct MouseEncoder {
    encoder: GhosttyMouseEncoder,
    event: GhosttyMouseEvent,
}

/// Geometry for mapping surface pixels to cells.
#[derive(Clone, Copy)]
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
            assert_eq!(ghostty_mouse_encoder_new(ptr::null(), &mut encoder), GHOSTTY_SUCCESS);
            assert_eq!(ghostty_mouse_event_new(ptr::null(), &mut event), GHOSTTY_SUCCESS);
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

    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        terminal: &Terminal,
        size: SurfaceSize,
        action: GhosttyMouseAction,
        button: Option<GhosttyMouseButton>,
        any_button_pressed: bool,
        mods: GhosttyMods,
        x: f32,
        y: f32,
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
                &any_button_pressed as *const _ as *const c_void,
            );
            ghostty_mouse_event_set_action(self.event, action);
            match button {
                Some(button) => ghostty_mouse_event_set_button(self.event, button),
                None => ghostty_mouse_event_clear_button(self.event),
            };
            ghostty_mouse_event_set_mods(self.event, mods);
            ghostty_mouse_event_set_position(self.event, GhosttyMousePosition { x, y });
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

// ---------------------------------------------------------------------------
// Selection gestures
// ---------------------------------------------------------------------------

pub struct SelectionGesture {
    gesture: GhosttySelectionGesture,
    press: GhosttySelectionGestureEvent,
    drag: GhosttySelectionGestureEvent,
    release: GhosttySelectionGestureEvent,
    /// Terminal the gesture was last used with, needed to free it.
    terminal: GhosttyTerminal,
}

pub struct GesturePoint {
    pub grid_ref: GhosttyGridRef,
    pub x: f64,
    pub y: f64,
}

impl SelectionGesture {
    pub fn new(terminal: &Terminal) -> Self {
        let mut gesture = ptr::null_mut();
        let new_event = |kind| {
            let mut event = ptr::null_mut();
            let result =
                unsafe { ghostty_selection_gesture_event_new(ptr::null(), &mut event, kind) };
            assert_eq!(result, GHOSTTY_SUCCESS);
            event
        };
        unsafe {
            assert_eq!(ghostty_selection_gesture_new(ptr::null(), &mut gesture), GHOSTTY_SUCCESS);
        }
        Self {
            gesture,
            press: new_event(GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_PRESS),
            drag: new_event(GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_DRAG),
            release: new_event(GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_RELEASE),
            terminal: terminal.raw,
        }
    }

    fn set<T>(event: GhosttySelectionGestureEvent, option: GhosttySelectionGestureEventOption, value: &T) {
        unsafe { ghostty_selection_gesture_event_set(event, option, value as *const T as *const c_void) };
    }

    /// Mouse press. `time_ns` is a monotonic timestamp used for multi-click
    /// detection. Returns the selection a multi-click produced, if any.
    pub fn press(
        &mut self,
        terminal: &Terminal,
        point: &GesturePoint,
        time_ns: u64,
    ) -> Option<GhosttySelection> {
        Self::set(self.press, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF, &point.grid_ref);
        let position = GhosttySurfacePosition { x: point.x, y: point.y };
        Self::set(self.press, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION, &position);
        Self::set(self.press, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_TIME_NS, &time_ns);
        self.apply(terminal, self.press)
    }

    pub fn drag(
        &mut self,
        terminal: &Terminal,
        point: &GesturePoint,
        geometry: GhosttySelectionGestureGeometry,
        rectangle: bool,
    ) -> Option<GhosttySelection> {
        Self::set(self.drag, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF, &point.grid_ref);
        let position = GhosttySurfacePosition { x: point.x, y: point.y };
        Self::set(self.drag, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION, &position);
        Self::set(self.drag, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_GEOMETRY, &geometry);
        Self::set(self.drag, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_RECTANGLE, &rectangle);
        self.apply(terminal, self.drag)
    }

    pub fn release(&mut self, terminal: &Terminal, grid_ref: Option<GhosttyGridRef>) {
        match grid_ref {
            Some(grid_ref) => {
                Self::set(self.release, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF, &grid_ref)
            }
            None => unsafe {
                ghostty_selection_gesture_event_set(
                    self.release,
                    GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF,
                    ptr::null(),
                );
            },
        }
        self.apply(terminal, self.release);
    }

    fn apply(
        &mut self,
        terminal: &Terminal,
        event: GhosttySelectionGestureEvent,
    ) -> Option<GhosttySelection> {
        self.terminal = terminal.raw;
        let mut selection = GhosttySelection::default();
        let result = unsafe {
            ghostty_selection_gesture_event(self.gesture, terminal.raw, event, &mut selection)
        };
        (result == GHOSTTY_SUCCESS).then_some(selection)
    }
}

impl Drop for SelectionGesture {
    fn drop(&mut self) {
        unsafe {
            ghostty_selection_gesture_event_free(self.press);
            ghostty_selection_gesture_event_free(self.drag);
            ghostty_selection_gesture_event_free(self.release);
            ghostty_selection_gesture_free(self.gesture, self.terminal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollback_keeps_thousands_of_rows() {
        let mut terminal = Terminal::new(80, 24);
        for i in 0..3000 {
            terminal.vt_write(format!("line {i}\r\n").as_bytes());
        }
        // 3000 lines plus the empty row holding the cursor.
        assert_eq!(terminal.scrollbar().total, 3001);
    }
}
