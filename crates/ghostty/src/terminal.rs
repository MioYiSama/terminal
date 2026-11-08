//! The terminal emulator.

use core::ffi::c_void;
use core::ptr::{self, NonNull};
use std::sync::atomic::{AtomicU64, Ordering};

use ghostty_sys as sys;

use crate::color::{Palette, Rgb};
use crate::ffi::{self, c_enum};
use crate::grid_ref::{GridRef, TrackedGridRef, TrackedGridRefId};
use crate::modes::Mode;
use crate::point::{Coordinate, Point, PointTag};
use crate::selection::Selection;
use crate::style::{CursorStyle, Style};
use crate::{Error, Result};

mod effects;
pub use effects::*;

c_enum! {
    /// The active terminal screen buffer.
    pub enum ActiveScreen {
        /// Normal screen with scrollback.
        Primary = sys::GHOSTTY_TERMINAL_SCREEN_PRIMARY,
        /// Alternate application screen.
        Alternate = sys::GHOSTTY_TERMINAL_SCREEN_ALTERNATE,
    }
}
c_enum! {
    /// Amount of scrollback compression work.
    pub enum CompressionMode {
        /// One bounded idle step.
        Incremental = sys::GHOSTTY_TERMINAL_COMPRESSION_MODE_INCREMENTAL,
        /// Inspect all eligible pages synchronously.
        Full = sys::GHOSTTY_TERMINAL_COMPRESSION_MODE_FULL,
    }
}
c_enum! {
    /// Scheduling result of scrollback compression.
    pub enum CompressionResult {
        /// Compression cannot reclaim memory on this platform.
        Unsupported = sys::GHOSTTY_TERMINAL_COMPRESSION_RESULT_UNSUPPORTED,
        /// More work remains.
        Pending = sys::GHOSTTY_TERMINAL_COMPRESSION_RESULT_PENDING,
        /// No continuation needs scheduling.
        Complete = sys::GHOSTTY_TERMINAL_COMPRESSION_RESULT_COMPLETE,
    }
}
c_enum! {
    /// Kind of VT control string.
    pub enum StringKind {
        /// APC, SOS, or PM.
        Apc = sys::GHOSTTY_TERMINAL_STRING_KIND_APC,
        /// Device control string.
        Dcs = sys::GHOSTTY_TERMINAL_STRING_KIND_DCS,
        /// Operating system command.
        Osc = sys::GHOSTTY_TERMINAL_STRING_KIND_OSC,
    }
}
c_enum! {
    /// How a control string starts.
    pub enum StringIntroducer {
        /// Escape followed by the introducer byte.
        Esc = sys::GHOSTTY_TERMINAL_STRING_INTRODUCER_ESC,
        /// Single C1 control byte.
        C1 = sys::GHOSTTY_TERMINAL_STRING_INTRODUCER_C1,
    }
}
c_enum! {
    /// How a control string ends.
    pub enum StringEnd {
        /// Escape, leaving the following backslash unconsumed.
        StEsc = sys::GHOSTTY_TERMINAL_STRING_END_ST_ESC,
        /// Single C1 string terminator.
        StC1 = sys::GHOSTTY_TERMINAL_STRING_END_ST_C1,
        /// CAN or SUB.
        Cancelled = sys::GHOSTTY_TERMINAL_STRING_END_CANCELLED,
        /// Another C1 control interrupted the string.
        Aborted = sys::GHOSTTY_TERMINAL_STRING_END_ABORTED,
        /// Bell terminated an OSC.
        Bel = sys::GHOSTTY_TERMINAL_STRING_END_BEL,
    }
}

bitflags::bitflags! {
    /// Parser boundaries at which a VT write may stop.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Boundaries: u32 {
        /// Outside any sequence or partial UTF-8 character.
        const GROUND = sys::GHOSTTY_TERMINAL_BOUNDARY_GROUND as u32;
        /// APC, SOS, or PM introducer.
        const APC_START = sys::GHOSTTY_TERMINAL_BOUNDARY_APC_START as u32;
        /// APC, SOS, or PM terminator.
        const APC_END = sys::GHOSTTY_TERMINAL_BOUNDARY_APC_END as u32;
        /// DCS introducer.
        const DCS_START = sys::GHOSTTY_TERMINAL_BOUNDARY_DCS_START as u32;
        /// DCS terminator.
        const DCS_END = sys::GHOSTTY_TERMINAL_BOUNDARY_DCS_END as u32;
        /// OSC introducer.
        const OSC_START = sys::GHOSTTY_TERMINAL_BOUNDARY_OSC_START as u32;
        /// OSC terminator.
        const OSC_END = sys::GHOSTTY_TERMINAL_BOUNDARY_OSC_END as u32;
    }
    /// XTCHECKSUM calculation extensions.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ChecksumExtension: u8 {
        /// Do not negate the result.
        const NO_NEGATE = 1;
        /// Do not add video attributes.
        const NO_ATTRIBUTES = 2;
        /// Include blank cells.
        const INCLUDE_BLANKS = 4;
        /// Treat unwritten cells as spaces.
        const UNWRITTEN_AS_SPACE = 8;
        /// Use full Unicode codepoints.
        const FULL_CODEPOINTS = 16;
    }
}

/// The point at which a bounded VT write stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteStop {
    /// Input bytes consumed, including the boundary byte.
    pub consumed: usize,
    /// Reached requested boundaries; empty means all input was consumed.
    pub reached: Boundaries,
    /// String kind, when a string boundary was reached.
    pub kind: Option<StringKind>,
    /// Introducer, when a start boundary was reached.
    pub introducer: Option<StringIntroducer>,
    /// Terminator, when an end boundary was reached.
    pub end: Option<StringEnd>,
    /// Boundary byte, when a string boundary was reached.
    pub byte: Option<u8>,
}

/// How to move the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollViewport {
    /// Oldest scrollback row.
    Top,
    /// Active screen.
    Bottom,
    /// Relative rows; negative scrolls up.
    Delta(isize),
    /// Absolute row offset from the oldest row.
    Row(usize),
    /// Relative semantic shell prompts.
    DeltaPrompt(isize),
}

/// Scrollbar geometry in rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrollbar {
    /// Total scrollable rows.
    pub total: u64,
    /// Viewport offset from the oldest row.
    pub offset: u64,
    /// Visible rows.
    pub len: u64,
}

/// Memory accounting for both terminal screens, including image storage.
#[derive(Debug, Clone, Copy)]
pub struct MemoryUsage {
    /// Whether compression can reclaim memory on this target.
    pub compression_supported: bool,
    /// Primary screen page count.
    pub primary_pages: u64,
    /// Primary screen reserved address space.
    pub primary_virtual_bytes: u64,
    /// Primary screen resident bytes (includes compressed data).
    pub primary_resident_bytes: u64,
    /// Primary screen compressed page count.
    pub primary_compressed_pages: u64,
    /// Primary screen compressed bytes.
    pub primary_compressed_bytes: u64,
    /// Primary screen image bytes, separate from resident page bytes.
    pub primary_image_bytes: u64,
    /// Alternate screen page count.
    pub alternate_pages: u64,
    /// Alternate screen reserved address space.
    pub alternate_virtual_bytes: u64,
    /// Alternate screen resident bytes.
    pub alternate_resident_bytes: u64,
    /// Alternate screen compressed page count.
    pub alternate_compressed_pages: u64,
    /// Alternate screen compressed bytes.
    pub alternate_compressed_bytes: u64,
    /// Alternate screen image bytes.
    pub alternate_image_bytes: u64,
}

/// A terminal emulator: screen, scrollback, cursor, modes, and VT parser.
///
/// Mutating methods require exclusive access, so borrowed strings and grid
/// references remain valid. The terminal can move between threads, but cannot
/// be accessed concurrently. Callbacks are synchronous and cannot reenter it.
pub struct Terminal {
    raw: NonNull<sys::GhosttyTerminalImpl>,
    callbacks: Box<effects::Callbacks>,
    tracked: Vec<Option<TrackedGridRef>>,
    identity: u64,
}

// SAFETY: ownership is exclusive; all stored closures are Send, and all C
// access (including owned tracked references) remains serialized by Rust borrows.
unsafe impl Send for Terminal {}
// NonNull deliberately prevents Sync. Verify Send at compile time.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<Terminal>();
    trait AmbiguousIfSync<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}
    let _ = <Terminal as AmbiguousIfSync<_>>::marker;
};

macro_rules! scalar_getters {
    ($($(#[$meta:meta])* $name:ident: $ty:ty => $key:ident),* $(,)?) => {$ (
        $(#[$meta])*
        pub fn $name(&self) -> Result<$ty> {
            let mut out = <$ty>::default();
            self.get(sys::$key, &mut out)?;
            Ok(out)
        }
    )*};
}
macro_rules! scalar_setters {
    ($($(#[$meta:meta])* $name:ident: $ty:ty => $key:ident),* $(,)?) => {$ (
        $(#[$meta])*
        pub fn $name(&mut self, value: Option<$ty>) -> Result<()> {
            self.set(sys::$key, value.as_ref())
        }
    )*};
}

impl Terminal {
    /// Creates a terminal of `cols` × `rows` nonzero cells.
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        let mut raw = ptr::null_mut();
        Error::check(unsafe { sys::ghostty_terminal_new(ptr::null(), &mut raw, cols, rows) })?;
        let raw = NonNull::new(raw).ok_or(Error::OutOfMemory)?;
        // SAFETY: newly allocated with the default allocator and uniquely owned.
        Ok(unsafe { Self::from_raw_owned(raw) })
    }

    /// Takes sole ownership of a live default-allocator terminal.
    pub(crate) unsafe fn from_raw_owned(raw: NonNull<sys::GhosttyTerminalImpl>) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        // A u64 counter cannot realistically wrap.
        let identity = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let mut terminal = Self {
            raw,
            callbacks: Box::default(),
            tracked: Vec::new(),
            identity,
        };
        let userdata = (&raw mut *terminal.callbacks).cast::<c_void>();
        terminal
            .set_callback(sys::GHOSTTY_TERMINAL_OPT_USERDATA, userdata)
            .expect("valid terminal userdata option");
        terminal
    }

    /// The borrowed raw handle, still owned by this terminal.
    pub fn as_raw(&self) -> sys::GhosttyTerminal {
        self.raw.as_ptr()
    }

    pub(crate) fn resume_callback_panic(&mut self) {
        self.callbacks.resume();
    }

    fn set_callback(
        &mut self,
        option: sys::GhosttyTerminalOption,
        value: *const c_void,
    ) -> Result<()> {
        let code = unsafe { sys::ghostty_terminal_set(self.as_raw(), option, value) };
        self.resume_callback_panic();
        Error::check(code)
    }

    fn set<T>(&mut self, option: sys::GhosttyTerminalOption, value: Option<&T>) -> Result<()> {
        self.set_callback(
            option,
            value.map_or(ptr::null(), |v| (v as *const T).cast()),
        )
    }

    fn get<T>(&self, key: sys::GhosttyTerminalData, out: &mut T) -> Result<()> {
        // SAFETY: callers pair each key with its documented output type.
        Error::check(unsafe {
            sys::ghostty_terminal_get(self.as_raw(), key, (out as *mut T).cast())
        })
    }

    fn optional_get<T: Default>(&self, key: sys::GhosttyTerminalData) -> Result<Option<T>> {
        let mut out = T::default();
        match self.get(key, &mut out) {
            Ok(()) => Ok(Some(out)),
            Err(Error::NoValue) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn string(&self, key: sys::GhosttyTerminalData) -> Result<&[u8]> {
        let mut out = sys::GhosttyString::default();
        self.get(key, &mut out)?;
        // SAFETY: C borrows until the next mutation; the result borrows self.
        Ok(unsafe { ffi::string_bytes(out) })
    }

    fn set_string(&mut self, key: sys::GhosttyTerminalOption, value: Option<&[u8]>) -> Result<()> {
        self.set(key, value.map(ffi::ghostty_string).as_ref())
    }

    /// Feeds raw VT bytes through the terminal parser.
    pub fn vt_write(&mut self, data: &[u8]) {
        unsafe { sys::ghostty_terminal_vt_write(self.as_raw(), data.as_ptr(), data.len()) };
        self.resume_callback_panic();
    }

    /// Feeds input up to the first requested parser boundary.
    pub fn vt_write_until(&mut self, data: &[u8], boundaries: Boundaries) -> Result<WriteStop> {
        // SAFETY: GhosttyTerminalWriteStop begins with its size field.
        let mut out = unsafe { ffi::sized::<sys::GhosttyTerminalWriteStop>() };
        let code = unsafe {
            sys::ghostty_terminal_vt_write_until(
                self.as_raw(),
                data.as_ptr(),
                data.len(),
                boundaries.bits(),
                &mut out,
            )
        };
        self.resume_callback_panic();
        if code != sys::GHOSTTY_NO_VALUE {
            Error::check(code)?;
        }
        let reached = Boundaries::from_bits_retain(out.reached);
        let start = reached
            .intersects(Boundaries::APC_START | Boundaries::DCS_START | Boundaries::OSC_START);
        let end =
            reached.intersects(Boundaries::APC_END | Boundaries::DCS_END | Boundaries::OSC_END);
        Ok(WriteStop {
            consumed: out.consumed,
            reached,
            kind: (start || end).then(|| StringKind::from_lib(out.kind)),
            introducer: start.then(|| StringIntroducer::from_lib(out.introducer)),
            end: end.then(|| StringEnd::from_lib(out.end)),
            byte: (start || end).then_some(out.byte),
        })
    }

    /// Performs a full terminal reset, preserving dimensions and configured defaults.
    pub fn reset(&mut self) {
        unsafe { sys::ghostty_terminal_reset(self.as_raw()) };
        self.resume_callback_panic();
    }

    /// Resizes the grid and updates the per-cell pixel dimensions.
    pub fn resize(
        &mut self,
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> Result<()> {
        let code = unsafe {
            sys::ghostty_terminal_resize(self.as_raw(), cols, rows, cell_width_px, cell_height_px)
        };
        self.resume_callback_panic();
        Error::check(code)
    }

    /// Writes replay-safe unfinished parser bytes to a synchronous writer.
    pub fn continuation_write<W: std::io::Write + ?Sized>(
        &mut self,
        writer: &mut W,
    ) -> std::io::Result<()> {
        ffi::with_writer(writer, |w| unsafe {
            sys::ghostty_terminal_continuation_write(self.as_raw(), w)
        })
    }

    /// Copies replay-safe unfinished parser bytes into a vector.
    pub fn continuation_buf(&self) -> Result<Vec<u8>> {
        ffi::encode_vec(|buf, len, written| unsafe {
            sys::ghostty_terminal_continuation_buf(self.as_raw(), buf, len, written)
        })
    }

    /// Returns an allocated copy of replay-safe unfinished parser bytes.
    pub fn continuation_alloc(&self) -> Result<Vec<u8>> {
        let mut ptr = ptr::null_mut();
        let mut len = 0;
        Error::check(unsafe {
            sys::ghostty_terminal_continuation_alloc(self.as_raw(), ptr::null(), &mut ptr, &mut len)
        })?;
        // SAFETY: allocation came from the default allocator and is consumed once.
        Ok(unsafe { ffi::take_alloc(ptr, len) })
    }

    /// Moves the viewport without changing terminal contents.
    pub fn scroll_viewport(&mut self, behavior: ScrollViewport) -> Result<()> {
        let mut raw = sys::GhosttyTerminalScrollViewport::default();
        match behavior {
            ScrollViewport::Top => raw.tag = sys::GHOSTTY_TERMINAL_SCROLL_VIEWPORT_TOP,
            ScrollViewport::Bottom => raw.tag = sys::GHOSTTY_TERMINAL_SCROLL_VIEWPORT_BOTTOM,
            ScrollViewport::Delta(delta) => {
                raw.tag = sys::GHOSTTY_TERMINAL_SCROLL_VIEWPORT_DELTA;
                raw.value.delta = delta;
            }
            ScrollViewport::Row(row) => {
                raw.tag = sys::GHOSTTY_TERMINAL_SCROLL_VIEWPORT_ROW;
                raw.value.row = row;
            }
            ScrollViewport::DeltaPrompt(delta) => {
                raw.tag = sys::GHOSTTY_TERMINAL_SCROLL_VIEWPORT_DELTA_PROMPT;
                raw.value.delta_prompt = delta;
            }
        }
        Error::check(unsafe { sys::ghostty_terminal_scroll_viewport(self.as_raw(), raw) })
    }

    /// Returns an opaque activity token for scheduling idle compression.
    pub fn compression_activity(&self) -> Result<u64> {
        let mut out = 0;
        Error::check(unsafe {
            sys::ghostty_terminal_compression_activity(self.as_raw(), &mut out)
        })?;
        Ok(out)
    }

    /// Compresses eligible scrollback without changing logical contents.
    pub fn compress(&mut self, mode: CompressionMode) -> Result<CompressionResult> {
        let mut out = 0;
        Error::check(unsafe {
            sys::ghostty_terminal_compress(self.as_raw(), mode.to_raw(), &mut out)
        })?;
        Ok(CompressionResult::from_lib(out))
    }

    scalar_getters! {
        /// Width in cells.
        cols: u16 => GHOSTTY_TERMINAL_DATA_COLS,
        /// Height in cells.
        rows: u16 => GHOSTTY_TERMINAL_DATA_ROWS,
        /// Cursor column, zero based.
        cursor_x: u16 => GHOSTTY_TERMINAL_DATA_CURSOR_X,
        /// Cursor row in the active area, zero based.
        cursor_y: u16 => GHOSTTY_TERMINAL_DATA_CURSOR_Y,
        /// Whether the next character will wrap.
        cursor_pending_wrap: bool => GHOSTTY_TERMINAL_DATA_CURSOR_PENDING_WRAP,
        /// Whether the cursor is visible.
        cursor_visible: bool => GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE,
        /// Whether a mouse tracking mode is enabled.
        mouse_tracking: bool => GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING,
        /// Number of rows including scrollback.
        total_rows: usize => GHOSTTY_TERMINAL_DATA_TOTAL_ROWS,
        /// Number of scrollback rows.
        scrollback_rows: usize => GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS,
        /// Total width in pixels.
        width_px: u32 => GHOSTTY_TERMINAL_DATA_WIDTH_PX,
        /// Total height in pixels.
        height_px: u32 => GHOSTTY_TERMINAL_DATA_HEIGHT_PX,
        /// Whether the viewport is pinned to the active area.
        viewport_active: bool => GHOSTTY_TERMINAL_DATA_VIEWPORT_ACTIVE,
        /// Whether any VT processing error has occurred.
        vt_processing_error: bool => GHOSTTY_TERMINAL_DATA_VT_PROCESSING_ERROR,
        /// Configured continuation byte limit; zero means disabled.
        continuation_max_bytes: usize => GHOSTTY_TERMINAL_DATA_CONTINUATION_MAX_BYTES,
        /// Whether parsing is outside any sequence or partial character.
        vt_ground: bool => GHOSTTY_TERMINAL_DATA_VT_GROUND,
        /// Whether the cursor is at a semantic shell prompt.
        cursor_at_prompt: bool => GHOSTTY_TERMINAL_DATA_CURSOR_AT_PROMPT,
        /// Configured maximum decoded clipboard write bytes.
        clipboard_write_max_bytes: usize => GHOSTTY_TERMINAL_DATA_CLIPBOARD_WRITE_MAX_BYTES,
        /// Kitty image storage limit for the active screen.
        kitty_image_storage_limit: u64 => GHOSTTY_TERMINAL_DATA_KITTY_IMAGE_STORAGE_LIMIT,
        /// Whether Kitty file loading is enabled.
        kitty_image_medium_file: bool => GHOSTTY_TERMINAL_DATA_KITTY_IMAGE_MEDIUM_FILE,
        /// Whether Kitty shared memory loading is enabled.
        kitty_image_medium_shared_mem: bool => GHOSTTY_TERMINAL_DATA_KITTY_IMAGE_MEDIUM_SHARED_MEM,
    }

    scalar_setters! {
        /// Sets the Kitty storage byte limit; None restores its default.
        set_kitty_image_storage_limit: u64 => GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_STORAGE_LIMIT,
        /// Enables Kitty file loading; None disables it.
        set_kitty_image_medium_file: bool => GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_MEDIUM_FILE,
        /// Enables Kitty shared memory loading; None disables it.
        set_kitty_image_medium_shared_mem: bool => GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_MEDIUM_SHARED_MEM,
        /// Sets all APC handlers' byte limit; None restores defaults.
        set_apc_max_bytes: usize => GHOSTTY_TERMINAL_OPT_APC_MAX_BYTES,
        /// Sets the Kitty APC byte limit; None restores its default.
        set_apc_max_bytes_kitty: usize => GHOSTTY_TERMINAL_OPT_APC_MAX_BYTES_KITTY,
        /// Sets default cursor blinking; None restores false.
        set_default_cursor_blink: bool => GHOSTTY_TERMINAL_OPT_DEFAULT_CURSOR_BLINK,
        /// Enables Glyph Protocol handling; None disables it.
        set_glyph_protocol: bool => GHOSTTY_TERMINAL_OPT_GLYPH_PROTOCOL,
        /// Sets scrollback byte limit; None restores default, usize::MAX is unlimited.
        set_scrollback_max_bytes: usize => GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,
        /// Sets scrollback line limit; None removes the limit.
        set_scrollback_max_lines: usize => GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,
        /// Sets continuation tracking byte limit; None disables it.
        set_continuation_max_bytes: usize => GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES,
        /// Enables potentially unsafe title reporting; None disables it.
        set_title_report: bool => GHOSTTY_TERMINAL_OPT_TITLE_REPORT,
        /// Sets unsupported sequence retention limit; None disables retention.
        set_unknown_max_bytes: usize => GHOSTTY_TERMINAL_OPT_UNKNOWN_MAX_BYTES,
        /// Sets the decoded clipboard transaction byte limit; None restores default.
        set_clipboard_write_max_bytes: usize => GHOSTTY_TERMINAL_OPT_CLIPBOARD_WRITE_MAX_BYTES,
        /// Sets whether resize can reveal scrollback; None restores true.
        set_resize_pull_scrollback: bool => GHOSTTY_TERMINAL_OPT_RESIZE_PULL_SCROLLBACK,
        /// Enables potentially unsafe screen checksums; None disables them.
        set_xt_checksum_report: bool => GHOSTTY_TERMINAL_OPT_XT_CHECKSUM_REPORT,
    }

    /// Sets the title, copying the bytes; None clears it.
    pub fn set_title(&mut self, value: Option<&[u8]>) -> Result<()> {
        self.set_string(sys::GHOSTTY_TERMINAL_OPT_TITLE, value)
    }
    /// Sets the working directory, copying the bytes; None clears it.
    pub fn set_pwd(&mut self, value: Option<&[u8]>) -> Result<()> {
        self.set_string(sys::GHOSTTY_TERMINAL_OPT_PWD, value)
    }
    /// Sets the terminfo entry name; None clears it.
    pub fn set_terminfo_name(&mut self, value: Option<&[u8]>) -> Result<()> {
        self.set_string(sys::GHOSTTY_TERMINAL_OPT_TERMINFO_NAME, value)
    }
    /// Enables Kitty temporary-file loading in the copied directory; None disables it.
    pub fn set_kitty_image_medium_temp_file(&mut self, value: Option<&[u8]>) -> Result<()> {
        self.set_string(
            sys::GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_MEDIUM_TEMP_FILE,
            value,
        )
    }
    /// Borrows the terminal title until the next mutable terminal call.
    pub fn title(&self) -> Result<&[u8]> {
        self.string(sys::GHOSTTY_TERMINAL_DATA_TITLE)
    }
    /// Borrows the raw working directory bytes until the next mutable terminal call.
    pub fn pwd(&self) -> Result<&[u8]> {
        self.string(sys::GHOSTTY_TERMINAL_DATA_PWD)
    }
    /// Borrows the allowed Kitty temporary-file directory.
    pub fn kitty_image_medium_temp_file(&self) -> Result<&[u8]> {
        self.string(sys::GHOSTTY_TERMINAL_DATA_KITTY_IMAGE_MEDIUM_TEMP_FILE)
    }
    /// Configured scrollback byte limit, or None for unlimited.
    pub fn scrollback_max_bytes(&self) -> Result<Option<usize>> {
        self.optional_get(sys::GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_BYTES)
    }
    /// Configured scrollback line limit, or None for unlimited.
    pub fn scrollback_max_lines(&self) -> Result<Option<usize>> {
        self.optional_get(sys::GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_LINES)
    }
    /// Returns which screen buffer is active.
    pub fn active_screen(&self) -> Result<ActiveScreen> {
        let mut out = 0;
        self.get(sys::GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN, &mut out)?;
        Ok(ActiveScreen::from_lib(out))
    }
    /// Current Kitty keyboard protocol flags.
    pub fn kitty_keyboard_flags(&self) -> Result<crate::key::KittyKeyFlags> {
        let mut out = 0u8;
        self.get(sys::GHOSTTY_TERMINAL_DATA_KITTY_KEYBOARD_FLAGS, &mut out)?;
        Ok(crate::key::KittyKeyFlags::from_bits_retain(out))
    }
    /// Mouse pointer shape requested through OSC 22.
    pub fn mouse_shape(&self) -> Result<crate::mouse::MouseShape> {
        let mut out = 0;
        self.get(sys::GHOSTTY_TERMINAL_DATA_MOUSE_SHAPE, &mut out)?;
        Ok(crate::mouse::MouseShape::from_lib(out))
    }
    /// Current SGR style used for newly printed cells.
    pub fn cursor_sgr_style(&self) -> Result<Style> {
        let mut out = sys::GhosttyStyle::default();
        self.get(sys::GHOSTTY_TERMINAL_DATA_CURSOR_SGR_STYLE, &mut out)?;
        Ok(Style::from_raw(&out))
    }
    /// Returns viewport scrollbar geometry.
    pub fn scrollbar(&self) -> Result<Scrollbar> {
        let mut out = sys::GhosttyTerminalScrollbar::default();
        self.get(sys::GHOSTTY_TERMINAL_DATA_SCROLLBAR, &mut out)?;
        Ok(Scrollbar {
            total: out.total,
            offset: out.offset,
            len: out.len,
        })
    }
    /// Sets the default shape used by cursor-style reset; None restores block.
    pub fn set_default_cursor_style(&mut self, value: Option<CursorStyle>) -> Result<()> {
        self.set(
            sys::GHOSTTY_TERMINAL_OPT_DEFAULT_CURSOR_STYLE,
            value.map(CursorStyle::to_raw).as_ref(),
        )
    }
    /// Sets reset-time checksum calculation extensions.
    pub fn set_xt_checksum_extension(&mut self, value: Option<ChecksumExtension>) -> Result<()> {
        self.set(
            sys::GHOSTTY_TERMINAL_OPT_XT_CHECKSUM_EXTENSION,
            value.map(|v| v.bits()).as_ref(),
        )
    }
    /// Queries a recognized terminal mode.
    pub fn mode(&self, mode: Mode) -> Result<bool> {
        let mut out = sys::GhosttyTerminalModeConfig {
            mode: mode.to_raw(),
            value: false,
        };
        self.get(sys::GHOSTTY_TERMINAL_DATA_MODE, &mut out)?;
        Ok(out.value)
    }
    /// Sets a mode without changing its reset default.
    pub fn set_mode(&mut self, mode: Mode, value: bool) -> Result<()> {
        self.set(
            sys::GHOSTTY_TERMINAL_OPT_MODE,
            Some(&sys::GhosttyTerminalModeConfig {
                mode: mode.to_raw(),
                value,
            }),
        )
    }
    /// Sets both a mode's current value and reset default.
    pub fn set_mode_default(&mut self, mode: Mode, value: bool) -> Result<()> {
        self.set(
            sys::GHOSTTY_TERMINAL_OPT_MODE_DEFAULT,
            Some(&sys::GhosttyTerminalModeConfig {
                mode: mode.to_raw(),
                value,
            }),
        )
    }

    /// Sets the selection using points resolved in this terminal's active screen.
    ///
    /// Points avoid taking a Selection that borrows this same terminal: the C
    /// setter is mutating and invalidates untracked references. The selection
    /// is copied immediately into terminal-owned tracked state.
    pub fn set_selection(&mut self, start: Point, end: Point, rectangle: bool) -> Result<()> {
        let mut selection = sys::GhosttySelection::default();
        Error::check(unsafe {
            sys::ghostty_terminal_grid_ref(self.as_raw(), start.to_raw(), &mut selection.start)
        })?;
        Error::check(unsafe {
            sys::ghostty_terminal_grid_ref(self.as_raw(), end.to_raw(), &mut selection.end)
        })?;
        selection.rectangle = rectangle;
        self.set(sys::GHOSTTY_TERMINAL_OPT_SELECTION, Some(&selection))
    }
    /// Clears the active screen's selection.
    pub fn clear_selection(&mut self) -> Result<()> {
        self.set::<sys::GhosttySelection>(sys::GHOSTTY_TERMINAL_OPT_SELECTION, None)
    }
    /// Borrows a snapshot of the active selection until the next terminal mutation.
    pub fn selection(&self) -> Result<Option<Selection<'_>>> {
        let Some(raw) =
            self.optional_get::<sys::GhosttySelection>(sys::GHOSTTY_TERMINAL_DATA_SELECTION)?
        else {
            return Ok(None);
        };
        // SAFETY: C produced the selection for this terminal and the result borrows it.
        Ok(Some(unsafe { Selection::from_raw(self, &raw) }))
    }

    /// Reads memory accounting without decompressing scrollback.
    pub fn memory_usage(&self) -> Result<MemoryUsage> {
        // SAFETY: this struct starts with size_t size.
        let mut r = unsafe { ffi::sized::<sys::GhosttyTerminalMemoryUsage>() };
        self.get(sys::GHOSTTY_TERMINAL_DATA_MEMORY_USAGE, &mut r)?;
        Ok(MemoryUsage {
            compression_supported: r.compression_supported,
            primary_pages: r.primary_pages,
            primary_virtual_bytes: r.primary_virtual_bytes,
            primary_resident_bytes: r.primary_resident_bytes,
            primary_compressed_pages: r.primary_compressed_pages,
            primary_compressed_bytes: r.primary_compressed_bytes,
            primary_image_bytes: r.primary_image_bytes,
            alternate_pages: r.alternate_pages,
            alternate_virtual_bytes: r.alternate_virtual_bytes,
            alternate_resident_bytes: r.alternate_resident_bytes,
            alternate_compressed_pages: r.alternate_compressed_pages,
            alternate_compressed_bytes: r.alternate_compressed_bytes,
            alternate_image_bytes: r.alternate_image_bytes,
        })
    }

    /// Creates a terminal-owned tracked reference; it follows scrolling and reflow.
    pub fn grid_ref_track(&mut self, point: Point) -> Result<TrackedGridRefId> {
        self.track(point)
    }
    /// Creates a terminal-owned tracked reference; IDs from another terminal are rejected.
    pub fn track(&mut self, point: Point) -> Result<TrackedGridRefId> {
        let mut raw = ptr::null_mut();
        Error::check(unsafe {
            sys::ghostty_terminal_grid_ref_track(self.as_raw(), point.to_raw(), &mut raw)
        })?;
        let raw = NonNull::new(raw).ok_or(Error::OutOfMemory)?;
        let id = TrackedGridRefId::new(self.identity, self.tracked.len());
        // SAFETY: the newly allocated handle is exclusively owned by this slot.
        self.tracked
            .push(Some(unsafe { TrackedGridRef::from_raw(raw) }));
        Ok(id)
    }
    fn tracked_ref(&self, id: TrackedGridRefId) -> Result<&TrackedGridRef> {
        let index = id.index(self.identity).ok_or(Error::InvalidValue)?;
        self.tracked
            .get(index)
            .and_then(Option::as_ref)
            .ok_or(Error::InvalidValue)
    }
    /// Whether the tracked cell still exists; invalid IDs return false.
    pub fn tracked_has_value(&self, id: TrackedGridRefId) -> bool {
        self.tracked_ref(id)
            .is_ok_and(|r| unsafe { sys::ghostty_tracked_grid_ref_has_value(r.as_raw()) })
    }
    /// Converts a tracked cell to the requested coordinate space.
    pub fn tracked_point(&self, id: TrackedGridRefId, tag: PointTag) -> Result<Coordinate> {
        let mut out = sys::GhosttyPointCoordinate::default();
        Error::check(unsafe {
            sys::ghostty_tracked_grid_ref_point(
                self.tracked_ref(id)?.as_raw(),
                tag.to_raw(),
                &mut out,
            )
        })?;
        Ok(out.into())
    }
    /// Borrows an untracked snapshot until the next terminal mutation.
    pub fn tracked_snapshot(&self, id: TrackedGridRefId) -> Result<GridRef<'_>> {
        let mut out = sys::GhosttyGridRef::default();
        Error::check(unsafe {
            sys::ghostty_tracked_grid_ref_snapshot(self.tracked_ref(id)?.as_raw(), &mut out)
        })?;
        // SAFETY: owned by this terminal, and the returned snapshot borrows it.
        Ok(unsafe { GridRef::from_raw(self, out) })
    }
    /// Moves an existing tracked reference to a new point in the active screen.
    pub fn retrack(&mut self, id: TrackedGridRefId, point: Point) -> Result<()> {
        Error::check(unsafe {
            sys::ghostty_tracked_grid_ref_set(
                self.tracked_ref(id)?.as_raw(),
                self.as_raw(),
                point.to_raw(),
            )
        })
    }
    /// Frees a tracked reference; stale or foreign IDs are rejected.
    pub fn untrack(&mut self, id: TrackedGridRefId) -> Result<()> {
        self.tracked_ref(id)?;
        self.tracked[id.index(self.identity).expect("validated identity")] = None;
        Ok(())
    }
}

macro_rules! color_accessors {
    ($($(#[$meta:meta])* $name:ident => $key:ident),* $(,)?) => { impl Terminal { $(
        $(#[$meta])*
        pub fn $name(&self) -> Result<Option<Rgb>> { Ok(self.optional_get::<sys::GhosttyColorRgb>(sys::$key)?.map(Into::into)) }
    )* } };
}
color_accessors! {
    /// Effective foreground color.
    color_foreground => GHOSTTY_TERMINAL_DATA_COLOR_FOREGROUND,
    /// Effective background color.
    color_background => GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND,
    /// Effective cursor color.
    color_cursor => GHOSTTY_TERMINAL_DATA_COLOR_CURSOR,
    /// Default foreground, ignoring OSC overrides.
    color_foreground_default => GHOSTTY_TERMINAL_DATA_COLOR_FOREGROUND_DEFAULT,
    /// Default background, ignoring OSC overrides.
    color_background_default => GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND_DEFAULT,
    /// Default cursor color, ignoring OSC overrides.
    color_cursor_default => GHOSTTY_TERMINAL_DATA_COLOR_CURSOR_DEFAULT,
}
impl Terminal {
    /// Sets the default foreground; None unsets it.
    pub fn set_color_foreground(&mut self, value: Option<Rgb>) -> Result<()> {
        self.set(
            sys::GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND,
            value.map(sys::GhosttyColorRgb::from).as_ref(),
        )
    }
    /// Sets the default background; None unsets it.
    pub fn set_color_background(&mut self, value: Option<Rgb>) -> Result<()> {
        self.set(
            sys::GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND,
            value.map(sys::GhosttyColorRgb::from).as_ref(),
        )
    }
    /// Sets the default cursor color; None unsets it.
    pub fn set_color_cursor(&mut self, value: Option<Rgb>) -> Result<()> {
        self.set(
            sys::GHOSTTY_TERMINAL_OPT_COLOR_CURSOR,
            value.map(sys::GhosttyColorRgb::from).as_ref(),
        )
    }
    /// Sets the default palette; None restores the built-in palette.
    pub fn set_color_palette(&mut self, value: Option<&Palette>) -> Result<()> {
        // SAFETY: Rgb has the same repr(C) layout as GhosttyColorRgb.
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_COLOR_PALETTE,
            value.map_or(ptr::null(), |p| p.as_ptr().cast()),
        )
    }
    /// Returns the effective 256-color palette.
    pub fn color_palette(&self) -> Result<Palette> {
        self.palette(sys::GHOSTTY_TERMINAL_DATA_COLOR_PALETTE)
    }
    /// Returns the default palette, ignoring OSC overrides.
    pub fn color_palette_default(&self) -> Result<Palette> {
        self.palette(sys::GHOSTTY_TERMINAL_DATA_COLOR_PALETTE_DEFAULT)
    }
    fn palette(&self, key: sys::GhosttyTerminalData) -> Result<Palette> {
        let mut out = [sys::GhosttyColorRgb::default(); 256];
        self.get(key, &mut out)?;
        Ok(out.map(Into::into))
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Tracked refs mutate their owning screen when freed; keep destruction
        // serialized with all terminal access by owning them here, not externally.
        self.tracked.clear();
        unsafe { sys::ghostty_terminal_free(self.as_raw()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_resize_cursor_modes_and_title() {
        let mut t = Terminal::new(20, 4).unwrap();
        t.vt_write(b"abc\x1b[?2004h\x1b]2;hello\x07");
        assert_eq!(t.cursor_x().unwrap(), 3);
        assert!(t.mode(Mode::BRACKETED_PASTE).unwrap());
        assert_eq!(t.title().unwrap(), b"hello");
        t.resize(30, 6, 8, 16).unwrap();
        assert_eq!((t.cols().unwrap(), t.rows().unwrap()), (30, 6));
        assert_eq!((t.width_px().unwrap(), t.height_px().unwrap()), (240, 96));
    }

    #[test]
    fn terminal_boundaries_and_continuation() {
        let mut t = Terminal::new(20, 4).unwrap();
        t.set_continuation_max_bytes(Some(1024)).unwrap();
        let stop = t
            .vt_write_until(b"a\x1b]2;hi\x07z", Boundaries::OSC_START)
            .unwrap();
        assert_eq!(stop.consumed, 3);
        assert_eq!(stop.kind, Some(StringKind::Osc));
        t.vt_write(b"2;hi");
        assert_eq!(t.continuation_buf().unwrap(), b"\x1b]2;hi");
        assert_eq!(t.continuation_alloc().unwrap(), b"\x1b]2;hi");
        let mut bytes = Vec::new();
        t.continuation_write(&mut bytes).unwrap();
        assert_eq!(bytes, b"\x1b]2;hi");
        t.vt_write(b"\x07");
        assert!(t.continuation_buf().unwrap().is_empty());
    }

    #[test]
    fn terminal_scroll_tracking_and_compression() {
        let mut t = Terminal::new(10, 2).unwrap();
        t.set_scrollback_max_bytes(Some(usize::MAX)).unwrap();
        t.vt_write(b"first");
        let id = t.track(Point::active(0, 0)).unwrap();
        t.vt_write(b"\r\nsecond\r\nthird\r\nfourth");
        assert!(t.tracked_has_value(id));
        assert_eq!(t.tracked_snapshot(id).unwrap().text().unwrap(), "f");
        assert_eq!(t.tracked_point(id, PointTag::Screen).unwrap().y, 0);
        t.scroll_viewport(ScrollViewport::Top).unwrap();
        assert_eq!(t.scrollbar().unwrap().offset, 0);
        assert!(!t.viewport_active().unwrap());
        t.scroll_viewport(ScrollViewport::Bottom).unwrap();
        assert!(t.viewport_active().unwrap());
        let token = t.compression_activity().unwrap();
        t.compress(CompressionMode::Full).unwrap();
        assert_eq!(t.compression_activity().unwrap(), token);
        assert!(t.memory_usage().unwrap().primary_pages > 0);
        t.untrack(id).unwrap();
        assert!(!t.tracked_has_value(id));
    }

    #[test]
    fn terminal_colors_selection_and_reset_defaults() {
        let mut t = Terminal::new(10, 2).unwrap();
        let color = Rgb {
            r: 12,
            g: 34,
            b: 56,
        };
        t.set_color_foreground(Some(color)).unwrap();
        assert_eq!(t.color_foreground().unwrap(), Some(color));
        t.vt_write(b"hello");
        t.set_selection(Point::active(0, 0), Point::active(4, 0), false)
            .unwrap();
        assert!(t.selection().unwrap().is_some());
        t.clear_selection().unwrap();
        assert!(t.selection().unwrap().is_none());
        t.set_mode_default(Mode::BRACKETED_PASTE, true).unwrap();
        t.set_mode(Mode::BRACKETED_PASTE, false).unwrap();
        t.reset();
        assert!(t.mode(Mode::BRACKETED_PASTE).unwrap());
    }
}
