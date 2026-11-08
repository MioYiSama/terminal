//! Safe wrapper over the parts of libghostty-vt this app uses: the terminal
//! state machine, the render-state snapshot, input encoders, selection and
//! paste. Everything here is single-threaded and owned by the UI thread.
//! No libghostty handle or struct is part of this module's public API.

use core::ffi::c_void;
use core::{mem, ptr, slice};

use ghostty_sys::*;

/// A zeroed "sized struct" (`GHOSTTY_INIT_SIZED`) with its `size` field set.
/// Defined before the submodules so they can use it.
macro_rules! sized {
    ($ty:ty) => {{
        let mut value = <$ty>::default();
        value.size = core::mem::size_of::<$ty>();
        value
    }};
}

mod graphics;
mod input;
mod render;
mod selection;

pub use graphics::{KittyImage, KittyImageFormat, install_png_decoder};
pub use input::{
    KeyAction, KeyEncoder, KeyInput, MouseAction, MouseButton, MouseEncoder, MouseInput,
    SurfaceSize, encode_focus,
};
pub use render::{CursorShape, Frame, ImageLayer, ImagePlacement, RenderState, Underline};
pub use selection::GesturePoint;

/// Scrollback budget per terminal. Cells take 8 bytes, so this is about
/// 290,000 rows at 200 columns; pages are allocated as history grows.
const SCROLLBACK_MAX_BYTES: usize = 500_000_000;
/// Kitty graphics storage per screen, Ghostty's `image-storage-limit`
/// default.
const KITTY_IMAGE_STORAGE_LIMIT: u64 = 320_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<GhosttyColorRgb> for Rgb {
    fn from(c: GhosttyColorRgb) -> Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

impl From<Rgb> for GhosttyColorRgb {
    fn from(c: Rgb) -> Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
        }
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
    /// Mouse selection state. It pins grid rows of `raw`, so it is freed
    /// together with the terminal and never handed out.
    gesture: selection::Gesture,
}

/// What [`Terminal::paste`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteOutcome {
    /// The text went to `take_pty_output`.
    Written,
    /// Nothing to paste (empty text).
    Empty,
    /// The text could run commands and unsafe pastes were not allowed;
    /// nothing was written.
    Unsafe,
    /// libghostty failed (out of memory, …); nothing was written.
    Failed,
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
            ghostty_terminal_set(
                raw,
                GHOSTTY_TERMINAL_OPT_WRITE_PTY,
                write_pty as *const c_void,
            );
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
        Self {
            raw,
            effects,
            gesture: selection::Gesture::new(),
        }
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
            (
                GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND,
                &foreground as *const _ as *const c_void,
            ),
            (
                GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND,
                &background as *const _ as *const c_void,
            ),
            (
                GHOSTTY_TERMINAL_OPT_COLOR_CURSOR,
                &cursor as *const _ as *const c_void,
            ),
            (
                GHOSTTY_TERMINAL_OPT_COLOR_PALETTE,
                palette.as_ptr() as *const c_void,
            ),
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

    /// Whether the program asked for key release events (Kitty keyboard
    /// protocol with event reporting).
    pub fn reports_key_releases(&self) -> bool {
        let flags = self
            .get::<u8>(GHOSTTY_TERMINAL_DATA_KITTY_KEYBOARD_FLAGS)
            .unwrap_or(0);
        flags & GHOSTTY_KITTY_KEY_REPORT_EVENTS as u8 != 0
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

    /// Grid reference for a viewport cell. Valid until the next mutating
    /// call, so it never leaves this module.
    fn viewport_ref(&self, x: u16, y: u16) -> Option<GhosttyGridRef> {
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

    /// The OSC 8 hyperlink URI of a viewport cell, if it has one.
    pub fn hyperlink_at(&self, x: u16, y: u16) -> Option<String> {
        let grid_ref = self.viewport_ref(x, y)?;
        let mut len = 0usize;
        let result =
            unsafe { ghostty_grid_ref_hyperlink_uri(&grid_ref, ptr::null_mut(), 0, &mut len) };
        if result != GHOSTTY_OUT_OF_SPACE {
            return None;
        }
        let mut uri = vec![0u8; len];
        let result =
            unsafe { ghostty_grid_ref_hyperlink_uri(&grid_ref, uri.as_mut_ptr(), len, &mut len) };
        (result == GHOSTTY_SUCCESS && len > 0).then(|| {
            uri.truncate(len);
            String::from_utf8_lossy(&uri).into_owned()
        })
    }

    /// Paste text according to the terminal's modes (bracketed paste, Kitty
    /// paste events). Output goes to `take_pty_output`.
    pub fn paste(&mut self, text: &str, allow_unsafe: bool) -> PasteOutcome {
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
        match unsafe { ghostty_terminal_paste(self.raw, &paste, &mut pasted) } {
            GHOSTTY_SUCCESS if pasted => PasteOutcome::Written,
            GHOSTTY_SUCCESS => PasteOutcome::Empty,
            GHOSTTY_REJECTED => PasteOutcome::Unsafe,
            _ => PasteOutcome::Failed,
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        unsafe {
            // The gesture unpins its rows from the still-live terminal.
            self.gesture.free(self.raw);
            ghostty_terminal_free(self.raw);
            drop(Box::from_raw(self.effects));
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
