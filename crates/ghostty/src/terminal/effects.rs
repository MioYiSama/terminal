//! Synchronous terminal effects and lifetime-bound clipboard replies.
//!
//! Callbacks run while the terminal processes input. Borrowed bytes must be
//! copied to retain them. Callback panics are caught at the FFI boundary and
//! resumed after the terminal operation returns. Every callback must be `Send`.

use core::ffi::c_void;
use core::marker::PhantomData;

use ghostty_sys as sys;

use super::Terminal;
use crate::color_scheme::ColorScheme;
use crate::device::DeviceAttributes;
use crate::ffi::{self, PanicSlot};
use crate::size_report::Size;

ffi::c_enum! {
    /// A normalized clipboard destination.
    pub enum ClipboardLocation {
        /// The standard system clipboard.
        Standard = sys::GHOSTTY_CLIPBOARD_LOCATION_STANDARD,
        /// The selection clipboard.
        Selection = sys::GHOSTTY_CLIPBOARD_LOCATION_SELECTION,
        /// The primary selection clipboard.
        Primary = sys::GHOSTTY_CLIPBOARD_LOCATION_PRIMARY,
    }
}
ffi::c_enum! {
    /// Outcome of an atomic clipboard write.
    pub enum ClipboardWriteResult {
        /// The write succeeded.
        Success = sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_SUCCESS,
        /// Policy or the user denied the write.
        Denied = sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_DENIED,
        /// The destination or representations are unsupported.
        Unsupported = sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_UNSUPPORTED,
        /// The clipboard is temporarily unavailable.
        Busy = sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_BUSY,
        /// A representation contains invalid data.
        InvalidData = sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_INVALID_DATA,
        /// An I/O error prevented the write.
        IoError = sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_IO_ERROR,
    }
}
ffi::c_enum! {
    /// Outcome of a clipboard read.
    pub enum ClipboardReadResult {
        /// The reply contains clipboard contents.
        Success = sys::GHOSTTY_CLIPBOARD_READ_RESULT_SUCCESS,
        /// Policy or the user denied the read.
        Denied = sys::GHOSTTY_CLIPBOARD_READ_RESULT_DENIED,
        /// The clipboard cannot be read.
        Unsupported = sys::GHOSTTY_CLIPBOARD_READ_RESULT_UNSUPPORTED,
        /// The clipboard is temporarily unavailable.
        Busy = sys::GHOSTTY_CLIPBOARD_READ_RESULT_BUSY,
        /// An I/O error prevented the read.
        IoError = sys::GHOSTTY_CLIPBOARD_READ_RESULT_IO_ERROR,
    }
}

/// A borrowed MIME type, suitable for the available-types list in a read reply.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct ClipboardMime<'a> {
    raw: sys::GhosttyString,
    lifetime: PhantomData<&'a [u8]>,
}

impl<'a> ClipboardMime<'a> {
    /// Borrows a MIME type without copying or validating its bytes.
    pub fn new(mime: &'a [u8]) -> Self {
        Self {
            raw: ffi::ghostty_string(mime),
            lifetime: PhantomData,
        }
    }

    /// Returns the MIME type's bytes.
    pub fn as_bytes(&self) -> &'a [u8] {
        // SAFETY: construction ties this string to the source bytes' lifetime.
        unsafe { ffi::string_bytes(self.raw) }
    }
}

/// One borrowed binary-safe MIME representation of a clipboard value.
///
/// An empty data value is a representation, not a request to clear the clipboard.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct ClipboardContent<'a> {
    raw: sys::GhosttyClipboardContent,
    lifetime: PhantomData<&'a [u8]>,
}

impl<'a> ClipboardContent<'a> {
    /// Borrows one representation without copying its MIME type or data.
    pub fn new(mime: &'a [u8], data: &'a [u8]) -> Self {
        Self {
            raw: sys::GhosttyClipboardContent {
                mime: ffi::ghostty_string(mime),
                data: ffi::ghostty_string(data),
            },
            lifetime: PhantomData,
        }
    }

    /// Returns the representation's MIME type.
    pub fn mime(&self) -> &'a [u8] {
        // SAFETY: both strings remain valid for the wrapper's lifetime.
        unsafe { ffi::string_bytes(self.raw.mime) }
    }

    /// Returns the decoded binary representation.
    pub fn data(&self) -> &'a [u8] {
        // SAFETY: both strings remain valid for the wrapper's lifetime.
        unsafe { ffi::string_bytes(self.raw.data) }
    }
}

/// A synchronous atomic clipboard write, valid only inside its callback.
///
/// Commit every representation atomically. Returning without replying denies
/// the write; repeated replies are ignored. A reply cannot outlive this request.
pub struct ClipboardWrite<'a> {
    raw: &'a sys::GhosttyClipboardWrite,
}

impl<'a> ClipboardWrite<'a> {
    /// Returns the normalized destination.
    pub fn location(&self) -> ClipboardLocation {
        ClipboardLocation::from_lib(self.raw.location)
    }

    /// Returns all representations; an empty slice requests a clipboard clear.
    pub fn contents(&self) -> &'a [ClipboardContent<'a>] {
        // SAFETY: the callback owns a valid borrowed array; the transparent
        // wrapper has identical layout and ties its strings to this callback.
        unsafe { borrowed_slice(self.raw.contents.cast(), self.raw.contents_len) }
    }

    /// Returns the requesting program's name, or empty bytes when omitted.
    pub fn name(&self) -> &'a [u8] {
        // SAFETY: request strings remain valid throughout the callback.
        unsafe { ffi::string_bytes(self.raw.name) }
    }

    /// Whether a session grant already permits this request without prompting.
    pub fn granted(&self) -> bool {
        self.raw.granted
    }

    /// Whether a successful reply may remember the user's decision.
    pub fn can_remember(&self) -> bool {
        self.raw.can_remember
    }

    /// Replies synchronously; `remember` is honored only on success when allowed.
    pub fn reply(&self, result: ClipboardWriteResult, remember: bool) {
        // SAFETY: this is the sized write-reply struct required by the header.
        let mut reply: sys::GhosttyClipboardWriteReply = unsafe { ffi::sized() };
        reply.result = result.to_raw();
        reply.remember = remember;
        if let Some(f) = self.raw.reply {
            // SAFETY: the original request and reply are alive for this call.
            unsafe { f(self.raw, &reply) };
        }
    }
}

/// A synchronous clipboard read, valid only inside its callback.
///
/// Reading exposes the user's clipboard to the running program: mediate consent
/// unless `granted()` is true. A listing-only request (no MIME types, `list()`
/// true) should be served without prompting. Returning without replying denies
/// the request (empty contents for OSC 52); repeated replies are ignored.
pub struct ClipboardRead<'a> {
    raw: &'a sys::GhosttyClipboardRead,
}

impl<'a> ClipboardRead<'a> {
    /// Returns the clipboard to read.
    pub fn location(&self) -> ClipboardLocation {
        ClipboardLocation::from_lib(self.raw.location)
    }

    /// Returns requested MIME types in preference order.
    pub fn mimes(&self) -> &'a [ClipboardMime<'a>] {
        // SAFETY: this borrowed array is valid during the callback, and the
        // transparent wrapper has exactly the layout of GhosttyString.
        unsafe { borrowed_slice(self.raw.mimes.cast(), self.raw.mimes_len) }
    }

    /// Whether the reply should also list all available clipboard MIME types.
    pub fn list(&self) -> bool {
        self.raw.list
    }

    /// Returns the requesting program's name, or empty bytes when omitted.
    pub fn name(&self) -> &'a [u8] {
        // SAFETY: request strings remain valid throughout the callback.
        unsafe { ffi::string_bytes(self.raw.name) }
    }

    /// Whether a session grant already permits this request without prompting.
    pub fn granted(&self) -> bool {
        self.raw.granted
    }

    /// Whether a successful reply may remember the user's decision.
    pub fn can_remember(&self) -> bool {
        self.raw.can_remember
    }

    /// Replies before this callback returns, borrowing all data only for this call.
    ///
    /// On success, include each requested representation that is available;
    /// unrequested representations are ignored. OSC 52 uses the first text MIME
    /// entry. `available` is used only for listing requests. `remember` is honored
    /// only on success when `can_remember()` is true. Other outcomes ignore data.
    pub fn reply(
        &self,
        result: ClipboardReadResult,
        contents: &[ClipboardContent<'_>],
        available: &[ClipboardMime<'_>],
        remember: bool,
    ) {
        // SAFETY: this is the sized read-reply struct required by the header.
        let mut reply: sys::GhosttyClipboardReadReply = unsafe { ffi::sized() };
        reply.result = result.to_raw();
        // Transparent wrappers allow borrowing these arrays without allocation.
        reply.contents = contents.as_ptr().cast();
        reply.contents_len = contents.len();
        reply.available = available.as_ptr().cast();
        reply.available_len = available.len();
        reply.remember = remember;
        if let Some(f) = self.raw.reply {
            // SAFETY: request, arrays, strings and reply all outlive this call.
            unsafe { f(self.raw, &reply) };
        }
    }
}

/// A borrowed request to show a desktop notification.
#[derive(Debug, Clone, Copy)]
pub struct DesktopNotification<'a> {
    /// Notification title, empty when omitted by the protocol.
    pub title: &'a [u8],
    /// Notification body.
    pub body: &'a [u8],
}

ffi::c_enum! {
    /// State of a progress indication reported with OSC 9;4.
    pub enum ProgressState {
        /// Remove the progress indication.
        Remove = sys::GHOSTTY_TERMINAL_PROGRESS_STATE_REMOVE,
        /// Show determinate progress.
        Set = sys::GHOSTTY_TERMINAL_PROGRESS_STATE_SET,
        /// Show failure.
        Error = sys::GHOSTTY_TERMINAL_PROGRESS_STATE_ERROR,
        /// Show indeterminate progress.
        Indeterminate = sys::GHOSTTY_TERMINAL_PROGRESS_STATE_INDETERMINATE,
        /// Show paused progress.
        Pause = sys::GHOSTTY_TERMINAL_PROGRESS_STATE_PAUSE,
    }
}

/// A progress report from the running program.
#[derive(Debug, Clone, Copy)]
pub struct ProgressReport {
    /// Literal state reported by the program.
    pub state: ProgressState,
    /// Percentage from 0 through 100, or `None` when omitted.
    pub progress: Option<u8>,
}

ffi::c_enum! {
    /// What a program reports it is doing through OSC 7501.
    pub enum ProgramStatusState {
        /// Waiting for the user's next instruction.
        Idle = sys::GHOSTTY_PROGRAM_STATUS_STATE_IDLE,
        /// Running independently.
        Working = sys::GHOSTTY_PROGRAM_STATUS_STATE_WORKING,
        /// Finished work whose result is ready to inspect.
        Done = sys::GHOSTTY_PROGRAM_STATUS_STATE_DONE,
        /// Waiting for user action.
        Blocked = sys::GHOSTTY_PROGRAM_STATUS_STATE_BLOCKED,
        /// Failed and stopped.
        Error = sys::GHOSTTY_PROGRAM_STATUS_STATE_ERROR,
        /// Remove this record and descendants; an empty id removes everything.
        Clear = sys::GHOSTTY_PROGRAM_STATUS_STATE_CLEAR,
    }
}
ffi::c_enum! {
    /// What a blocked program needs from the user.
    pub enum ProgramStatusKind {
        /// Unspecified, or not blocked.
        None = sys::GHOSTTY_PROGRAM_STATUS_KIND_NONE,
        /// Approval to perform an action.
        Permission = sys::GHOSTTY_PROGRAM_STATUS_KIND_PERMISSION,
        /// An answer the user must type.
        Question = sys::GHOSTTY_PROGRAM_STATUS_KIND_QUESTION,
        /// A login or other credential.
        Auth = sys::GHOSTTY_PROGRAM_STATUS_KIND_AUTH,
    }
}

/// A validated, borrowed OSC 7501 report; text is untrusted, not markup.
///
/// Replace the entire record for `id`; clear removes it and descendants. Empty
/// `app` inherits from the nearest ancestor. Keep 64–256 records, evicting the
/// least recently updated. On a new shell prompt or process exit remove working
/// and blocked records; keep done/error until seen. RIS emits a root clear.
/// Sanitize invisible formatting characters before displaying text outside the
/// terminal, and identify its source so programs cannot impersonate other tabs.
#[derive(Debug, Clone, Copy)]
pub struct ProgramStatus<'a> {
    /// The program's activity.
    pub state: ProgramStatusState,
    /// What user action a blocked program needs.
    pub kind: ProgramStatusKind,
    /// Percentage for working/blocked states, or `None` for indeterminate work.
    pub progress: Option<u8>,
    /// Hierarchical record id; empty identifies the root.
    pub id: &'a [u8],
    /// Stable machine-readable program name.
    pub app: &'a [u8],
    /// Short human-readable record label.
    pub title: &'a [u8],
    /// Decoded one-line human-readable message.
    pub message: &'a [u8],
}

ffi::c_enum! {
    /// A step reported by shell integration.
    pub enum SemanticPromptKind {
        /// Invalid zero value; never emitted by the terminal.
        Invalid = sys::GHOSTTY_SEMANTIC_PROMPT_INVALID,
        /// The shell started drawing a prompt.
        PromptStart = sys::GHOSTTY_SEMANTIC_PROMPT_PROMPT_START,
        /// The prompt is drawn and command input starts.
        InputStart = sys::GHOSTTY_SEMANTIC_PROMPT_INPUT_START,
        /// A submitted command starts producing output.
        OutputStart = sys::GHOSTTY_SEMANTIC_PROMPT_OUTPUT_START,
        /// A command finished running.
        CommandEnd = sys::GHOSTTY_SEMANTIC_PROMPT_COMMAND_END,
    }
}
ffi::c_enum! {
    /// Which prompt a prompt-start event begins.
    pub enum PromptKind {
        /// Main prompt (also the default for non-prompt events).
        Primary = sys::GHOSTTY_SEMANTIC_PROMPT_PROMPT_PRIMARY,
        /// Right-aligned prompt.
        Right = sys::GHOSTTY_SEMANTIC_PROMPT_PROMPT_RIGHT,
        /// Prompt for an additional command line.
        Continuation = sys::GHOSTTY_SEMANTIC_PROMPT_PROMPT_CONTINUATION,
        /// Secondary prompt, often treated like continuation.
        Secondary = sys::GHOSTTY_SEMANTIC_PROMPT_PROMPT_SECONDARY,
    }
}

/// A borrowed shell integration event, emitted after screen state is updated.
///
/// Shells may omit steps or repeat prompt-start events; do not require a strict
/// event order. Fields are empty/default outside the event kinds noted below.
#[derive(Debug, Clone, Copy)]
pub struct SemanticPrompt<'a> {
    /// Command step reported by the shell.
    pub kind: SemanticPromptKind,
    /// Prompt being drawn, meaningful for prompt-start events.
    pub prompt_kind: PromptKind,
    /// Reported command exit code, only present for command-end events.
    pub exit_code: Option<i32>,
    /// Decoded command line for output-start, or empty if absent/undecodable.
    pub command: &'a [u8],
    /// Optional error description for command-end.
    pub error: &'a [u8],
}

/// A program's request to resize the text area; it does not resize the terminal.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum ResizeWindow {
    /// Resize the text grid; zero keeps the corresponding dimension unchanged.
    Grid {
        /// Requested height in rows, or zero to keep it.
        rows: u16,
        /// Requested width in columns, or zero to keep it.
        columns: u16,
    },
}

/// A complete unsupported sequence, borrowed only for the callback duration.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum UnknownSequence<'a> {
    /// Unsupported APC bytes between introducer and terminator.
    Apc {
        /// Retained binary content, not null-terminated.
        content: &'a [u8],
        /// Whether a byte limit or allocation failure shortened the content.
        truncated: bool,
    },
    /// OSC with an unimplemented command number.
    Osc {
        /// Retained content, including the OSC command number.
        content: &'a [u8],
        /// Whether a byte limit or allocation failure shortened the content.
        truncated: bool,
        /// Terminator to use if replying to this sequence.
        terminator: crate::osc::OscTerminator,
    },
}

struct Slot<F: ?Sized> {
    function: Option<Box<F>>,
    panic: PanicSlot,
}

impl<F: ?Sized> Default for Slot<F> {
    fn default() -> Self {
        Self {
            function: None,
            panic: PanicSlot::default(),
        }
    }
}

#[derive(Default)]
struct ResponseSlot {
    slot: Slot<dyn FnMut() -> Vec<u8> + Send>,
    // The C caller reads the returned pointer after the trampoline returns.
    response: Vec<u8>,
}

type BytesCallback = dyn for<'a> FnMut(&'a [u8]) + Send;

#[derive(Default)]
pub(super) struct Callbacks {
    write_pty: Slot<BytesCallback>,
    bell: Slot<dyn FnMut() + Send>,
    enquiry: ResponseSlot,
    xtversion: ResponseSlot,
    title_changed: Slot<BytesCallback>,
    pwd_changed: Slot<BytesCallback>,
    size: Slot<dyn FnMut() -> Option<Size> + Send>,
    color_scheme: Slot<dyn FnMut() -> Option<ColorScheme> + Send>,
    device_attributes: Slot<dyn FnMut() -> Option<DeviceAttributes> + Send>,
    clipboard_write: Slot<dyn for<'a> FnMut(ClipboardWrite<'a>) + Send>,
    clipboard_read: Slot<dyn for<'a> FnMut(ClipboardRead<'a>) + Send>,
    desktop_notification: Slot<dyn for<'a> FnMut(DesktopNotification<'a>) + Send>,
    progress_report: Slot<dyn FnMut(ProgressReport) + Send>,
    program_status: Slot<dyn for<'a> FnMut(ProgramStatus<'a>) + Send>,
    semantic_prompt: Slot<dyn for<'a> FnMut(SemanticPrompt<'a>) + Send>,
    reset: Slot<dyn FnMut() + Send>,
    resize_window: Slot<dyn FnMut(ResizeWindow) + Send>,
    render_hold: Slot<dyn FnMut(bool) + Send>,
    unknown_sequence: Slot<dyn for<'a> FnMut(UnknownSequence<'a>) + Send>,
}

impl Callbacks {
    pub(super) fn resume(&mut self) {
        self.write_pty.panic.resume();
        self.bell.panic.resume();
        self.enquiry.slot.panic.resume();
        self.xtversion.slot.panic.resume();
        self.title_changed.panic.resume();
        self.pwd_changed.panic.resume();
        self.size.panic.resume();
        self.color_scheme.panic.resume();
        self.device_attributes.panic.resume();
        self.clipboard_write.panic.resume();
        self.clipboard_read.panic.resume();
        self.desktop_notification.panic.resume();
        self.progress_report.panic.resume();
        self.program_status.panic.resume();
        self.semantic_prompt.panic.resume();
        self.reset.panic.resume();
        self.resize_window.panic.resume();
        self.render_hold.panic.resume();
        self.unknown_sequence.panic.resume();
    }
}

// Borrow only the active field, never the complete callback state. Clipboard
// replies can synchronously invoke write_pty while another callback is active.
// No safe callback receives a Terminal, so it cannot recursively invoke itself.
macro_rules! callback_slot {
    ($userdata:expr, $field:ident) => {{
        // SAFETY: userdata points to Core's stable boxed Callbacks. Only this
        // field is borrowed; synchronous write_pty re-entry uses another field.
        unsafe { &mut *(&raw mut (*$userdata.cast::<Callbacks>()).$field) }
    }};
}

// SAFETY requirement: ptr identifies len initialized elements valid for 'a;
// zero-length arrays may use a null pointer, as permitted by the C API.
unsafe fn borrowed_slice<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        &[]
    } else {
        // SAFETY: caller guarantees a readable, initialized array.
        unsafe { core::slice::from_raw_parts(ptr, len) }
    }
}

macro_rules! register_callback {
    ($(#[$doc:meta])* $on:ident, $clear:ident, $field:ident, $trampoline:ident,
     $option:ident, $bound:ty) => {
        $(#[$doc])*
        pub fn $on(&mut self, f: $bound) {
            self.set_callback(sys::$option, $trampoline as *const () as *const c_void)
                .expect("valid terminal callback option");
            self.callbacks.$field.function = Some(Box::new(f));
        }

        /// Removes the registered callback and restores the library's default behavior.
        pub fn $clear(&mut self) {
            self.set_callback(sys::$option, core::ptr::null())
                .expect("valid terminal callback option");
            self.callbacks.$field.function = None;
        }
    };
}

impl Terminal {
    register_callback! {
        /// Receives borrowed response bytes to write back to the pty, in order.
        on_write_pty, clear_write_pty, write_pty, write_pty,
        GHOSTTY_TERMINAL_OPT_WRITE_PTY, impl FnMut(&[u8]) + Send + 'static
    }
    register_callback! {
        /// Receives BEL (0x07) events.
        on_bell, clear_bell, bell, bell,
        GHOSTTY_TERMINAL_OPT_BELL, impl FnMut() + Send + 'static
    }
    register_callback! {
        /// Receives the new borrowed title bytes after OSC 0/2 changes it.
        /// RIS clears the title without invoking this callback; use `on_reset`.
        on_title_changed, clear_title_changed, title_changed, title_changed,
        GHOSTTY_TERMINAL_OPT_TITLE_CHANGED, impl for<'a> FnMut(&'a [u8]) + Send + 'static
    }
    register_callback! {
        /// Receives raw working-directory bytes (URI or path), including clears.
        /// No URI parsing is performed. RIS clearing is reported through `on_reset`.
        on_pwd_changed, clear_pwd_changed, pwd_changed, pwd_changed,
        GHOSTTY_TERMINAL_OPT_PWD_CHANGED, impl for<'a> FnMut(&'a [u8]) + Send + 'static
    }
    register_callback! {
        /// Answers XTWINOPS/mode-2048 geometry queries; `None` suppresses a report.
        on_size, clear_size, size, size,
        GHOSTTY_TERMINAL_OPT_SIZE, impl FnMut() -> Option<Size> + Send + 'static
    }
    register_callback! {
        /// Answers CSI ? 996 n queries; `None` silently ignores the query.
        on_color_scheme, clear_color_scheme, color_scheme, color_scheme,
        GHOSTTY_TERMINAL_OPT_COLOR_SCHEME, impl FnMut() -> Option<ColorScheme> + Send + 'static
    }
    register_callback! {
        /// Answers DA1/DA2/DA3 queries; `None` selects the library's VT220 defaults.
        on_device_attributes, clear_device_attributes, device_attributes, device_attributes,
        GHOSTTY_TERMINAL_OPT_DEVICE_ATTRIBUTES, impl FnMut() -> Option<DeviceAttributes> + Send + 'static
    }
    register_callback! {
        /// Handles a synchronous atomic clipboard write; reply before returning.
        on_clipboard_write, clear_clipboard_write, clipboard_write, clipboard_write,
        GHOSTTY_TERMINAL_OPT_CLIPBOARD_WRITE, impl for<'a> FnMut(ClipboardWrite<'a>) + Send + 'static
    }
    register_callback! {
        /// Handles synchronous clipboard reads; mediate consent and reply before returning.
        /// Installing this also enables Kitty paste events when mode 5522 is active.
        on_clipboard_read, clear_clipboard_read, clipboard_read, clipboard_read,
        GHOSTTY_TERMINAL_OPT_CLIPBOARD_READ, impl for<'a> FnMut(ClipboardRead<'a>) + Send + 'static
    }
    register_callback! {
        /// Receives borrowed desktop notification requests from OSC 9/777.
        on_desktop_notification, clear_desktop_notification, desktop_notification, desktop_notification,
        GHOSTTY_TERMINAL_OPT_DESKTOP_NOTIFICATION, impl for<'a> FnMut(DesktopNotification<'a>) + Send + 'static
    }
    register_callback! {
        /// Receives OSC 9;4 progress reports, including removal before a full reset.
        on_progress_report, clear_progress_report, progress_report, progress_report,
        GHOSTTY_TERMINAL_OPT_PROGRESS_REPORT, impl FnMut(ProgressReport) + Send + 'static
    }
    register_callback! {
        /// Receives validated OSC 7501 reports; keep records as described by `ProgramStatus`.
        /// Set `on_write_pty` too so support queries can be answered.
        on_program_status, clear_program_status, program_status, program_status,
        GHOSTTY_TERMINAL_OPT_PROGRAM_STATUS, impl for<'a> FnMut(ProgramStatus<'a>) + Send + 'static
    }
    register_callback! {
        /// Receives shell command-step events; repeated and omitted steps are valid.
        on_semantic_prompt, clear_semantic_prompt, semantic_prompt, semantic_prompt,
        GHOSTTY_TERMINAL_OPT_SEMANTIC_PROMPT, impl for<'a> FnMut(SemanticPrompt<'a>) + Send + 'static
    }
    register_callback! {
        /// Runs after program-driven RIS, following progress/status removal callbacks.
        /// Titles and working directories are cleared without their change callbacks.
        /// Soft resets do not invoke this callback.
        on_reset, clear_reset, reset, reset,
        GHOSTTY_TERMINAL_OPT_RESET, impl FnMut() + Send + 'static
    }
    register_callback! {
        /// Receives window resize requests without resizing; consider user opt-in.
        on_resize_window, clear_resize_window, resize_window, resize_window,
        GHOSTTY_TERMINAL_OPT_RESIZE_WINDOW, impl FnMut(ResizeWindow) + Send + 'static
    }
    register_callback! {
        /// Receives paired render-hold start/end events for VT mode 2026 changes.
        /// Resets and resizes end holds; setting a mode through the API does not notify.
        on_render_hold, clear_render_hold, render_hold, render_hold,
        GHOSTTY_TERMINAL_OPT_RENDER_HOLD, impl FnMut(bool) + Send + 'static
    }
    register_callback! {
        /// Receives complete unsupported APC/OSC sequences; also set a nonzero
        /// unknown-sequence byte limit. Cancelled, malformed supported, and disabled
        /// protocol sequences are not reported. Future unknown kinds are ignored.
        on_unknown_sequence, clear_unknown_sequence, unknown_sequence, unknown_sequence,
        GHOSTTY_TERMINAL_OPT_UNKNOWN_SEQUENCE, impl for<'a> FnMut(UnknownSequence<'a>) + Send + 'static
    }

    /// Answers ENQ (0x05) with owned bytes; empty bytes send no response.
    /// The returned allocation is retained until the next response or removal.
    pub fn on_enquiry(&mut self, f: impl FnMut() -> Vec<u8> + Send + 'static) {
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_ENQUIRY,
            enquiry as *const () as *const c_void,
        )
        .expect("valid terminal callback option");
        self.callbacks.enquiry.slot.function = Some(Box::new(f));
    }

    /// Removes the ENQ callback and its retained response.
    pub fn clear_enquiry(&mut self) {
        self.set_callback(sys::GHOSTTY_TERMINAL_OPT_ENQUIRY, core::ptr::null())
            .expect("valid terminal callback option");
        self.callbacks.enquiry = ResponseSlot::default();
    }

    /// Answers XTVERSION with owned version bytes; empty selects libghostty's version.
    /// The returned allocation is retained until the next response or removal.
    pub fn on_xtversion(&mut self, f: impl FnMut() -> Vec<u8> + Send + 'static) {
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_XTVERSION,
            xtversion as *const () as *const c_void,
        )
        .expect("valid terminal callback option");
        self.callbacks.xtversion.slot.function = Some(Box::new(f));
    }

    /// Removes the XTVERSION callback and restores the libghostty version reply.
    pub fn clear_xtversion(&mut self) {
        self.set_callback(sys::GHOSTTY_TERMINAL_OPT_XTVERSION, core::ptr::null())
            .expect("valid terminal callback option");
        self.callbacks.xtversion = ResponseSlot::default();
    }
}

unsafe extern "C" fn write_pty(
    _: sys::GhosttyTerminal,
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) {
    let slot = callback_slot!(userdata, write_pty);
    slot.panic.catch(|| {
        // SAFETY: the library borrows response bytes for this callback duration.
        let bytes = unsafe { borrowed_slice(data, len) };
        if let Some(f) = slot.function.as_mut() {
            f(bytes);
        }
    });
}

macro_rules! simple_trampoline {
    ($name:ident $(, $arg:ident: $ty:ty)?) => {
        unsafe extern "C" fn $name(_: sys::GhosttyTerminal, userdata: *mut c_void $(, $arg: $ty)?) {
            let slot = callback_slot!(userdata, $name);
            slot.panic.catch(|| {
                if let Some(f) = slot.function.as_mut() { f($($arg)?); }
            });
        }
    };
}
simple_trampoline!(bell);
simple_trampoline!(reset);
simple_trampoline!(render_hold, held: bool);

macro_rules! response_trampoline {
    ($name:ident) => {
        unsafe extern "C" fn $name(
            _: sys::GhosttyTerminal,
            userdata: *mut c_void,
        ) -> sys::GhosttyString {
            let state = callback_slot!(userdata, $name);
            match state.slot.panic.catch(|| {
                state.response = state.slot.function.as_mut().map_or_else(Vec::new, |f| f());
                ffi::ghostty_string(&state.response)
            }) {
                Some(response) => response,
                None => ffi::ghostty_string(&[]),
            }
        }
    };
}
response_trampoline!(enquiry);
response_trampoline!(xtversion);

macro_rules! title_trampoline {
    ($name:ident, $key:ident) => {
        unsafe extern "C" fn $name(terminal: sys::GhosttyTerminal, userdata: *mut c_void) {
            let slot = callback_slot!(userdata, $name);
            slot.panic.catch(|| {
                let mut raw = sys::GhosttyString::default();
                // SAFETY: this getter is explicitly permitted in this callback;
                // it writes a GhosttyString and does not mutate the terminal.
                let result = unsafe {
                    sys::ghostty_terminal_get(terminal, sys::$key, (&raw mut raw).cast())
                };
                crate::Error::check(result).expect("valid terminal string getter");
                // SAFETY: no mutating terminal operation occurs before f returns,
                // and its higher-ranked argument cannot escape the callback.
                let bytes = unsafe { ffi::string_bytes(raw) };
                if let Some(f) = slot.function.as_mut() {
                    f(bytes);
                }
            });
        }
    };
}
title_trampoline!(title_changed, GHOSTTY_TERMINAL_DATA_TITLE);
title_trampoline!(pwd_changed, GHOSTTY_TERMINAL_DATA_PWD);

macro_rules! query_trampoline {
    ($name:ident, $raw:ty) => {
        unsafe extern "C" fn $name(
            _: sys::GhosttyTerminal,
            userdata: *mut c_void,
            out: *mut $raw,
        ) -> bool {
            let slot = callback_slot!(userdata, $name);
            slot.panic
                .catch(|| {
                    let Some(value) = slot.function.as_mut().and_then(|f| f()) else {
                        return false;
                    };
                    // SAFETY: the library supplies a valid writable output pointer.
                    unsafe { *out = value.to_raw() };
                    true
                })
                .unwrap_or(false)
        }
    };
}
query_trampoline!(size, sys::GhosttySizeReportSize);
query_trampoline!(color_scheme, sys::GhosttyColorScheme);
query_trampoline!(device_attributes, sys::GhosttyDeviceAttributes);

unsafe extern "C" fn clipboard_write(
    _: sys::GhosttyTerminal,
    userdata: *mut c_void,
    raw: *const sys::GhosttyClipboardWrite,
) {
    let slot = callback_slot!(userdata, clipboard_write);
    slot.panic.catch(|| {
        // SAFETY: the original request is valid only during this callback.
        let request = ClipboardWrite {
            raw: unsafe { &*raw },
        };
        if let Some(f) = slot.function.as_mut() {
            f(request);
        }
    });
}

unsafe extern "C" fn clipboard_read(
    _: sys::GhosttyTerminal,
    userdata: *mut c_void,
    raw: *const sys::GhosttyClipboardRead,
) {
    let slot = callback_slot!(userdata, clipboard_read);
    slot.panic.catch(|| {
        // SAFETY: the original request is valid only during this callback.
        let request = ClipboardRead {
            raw: unsafe { &*raw },
        };
        if let Some(f) = slot.function.as_mut() {
            f(request);
        }
    });
}

unsafe extern "C" fn desktop_notification(
    _: sys::GhosttyTerminal,
    userdata: *mut c_void,
    raw: *const sys::GhosttyTerminalDesktopNotification,
) {
    let slot = callback_slot!(userdata, desktop_notification);
    slot.panic.catch(|| {
        // SAFETY: the payload and both strings are borrowed for this callback.
        let notification = unsafe {
            DesktopNotification {
                title: ffi::string_bytes((*raw).title),
                body: ffi::string_bytes((*raw).body),
            }
        };
        if let Some(f) = slot.function.as_mut() {
            f(notification);
        }
    });
}

unsafe extern "C" fn progress_report(
    _: sys::GhosttyTerminal,
    userdata: *mut c_void,
    raw: *const sys::GhosttyTerminalProgressReport,
) {
    let slot = callback_slot!(userdata, progress_report);
    slot.panic.catch(|| {
        // SAFETY: the payload is valid throughout this callback.
        let raw = unsafe { &*raw };
        let report = ProgressReport {
            state: ProgressState::from_lib(raw.state),
            progress: u8::try_from(raw.progress).ok(),
        };
        if let Some(f) = slot.function.as_mut() {
            f(report);
        }
    });
}

unsafe extern "C" fn program_status(
    _: sys::GhosttyTerminal,
    userdata: *mut c_void,
    raw: *const sys::GhosttyTerminalProgramStatus,
) {
    let slot = callback_slot!(userdata, program_status);
    slot.panic.catch(|| {
        // SAFETY: the payload and all strings are borrowed for this callback.
        let report = unsafe {
            let raw = &*raw;
            ProgramStatus {
                state: ProgramStatusState::from_lib(raw.state),
                kind: ProgramStatusKind::from_lib(raw.kind),
                progress: u8::try_from(raw.progress).ok(),
                id: ffi::string_bytes(raw.id),
                app: ffi::string_bytes(raw.app),
                title: ffi::string_bytes(raw.title),
                message: ffi::string_bytes(raw.message),
            }
        };
        if let Some(f) = slot.function.as_mut() {
            f(report);
        }
    });
}

unsafe extern "C" fn semantic_prompt(
    _: sys::GhosttyTerminal,
    userdata: *mut c_void,
    raw: *const sys::GhosttyTerminalSemanticPrompt,
) {
    let slot = callback_slot!(userdata, semantic_prompt);
    slot.panic.catch(|| {
        // SAFETY: the payload and all strings are borrowed for this callback.
        let event = unsafe {
            let raw = &*raw;
            let Some(kind) = SemanticPromptKind::from_raw(raw.kind) else {
                return;
            };
            SemanticPrompt {
                kind,
                prompt_kind: PromptKind::from_lib(raw.prompt_kind),
                exit_code: raw.has_exit_code.then_some(raw.exit_code),
                command: ffi::string_bytes(raw.command),
                error: ffi::string_bytes(raw.error),
            }
        };
        if let Some(f) = slot.function.as_mut() {
            f(event);
        }
    });
}

unsafe extern "C" fn resize_window(
    _: sys::GhosttyTerminal,
    userdata: *mut c_void,
    raw: *const sys::GhosttyTerminalResizeWindow,
) {
    let slot = callback_slot!(userdata, resize_window);
    slot.panic.catch(|| {
        // SAFETY: the payload is borrowed for this callback; inspect its tag
        // before reading a union field, and ignore future unsupported tags.
        let request = unsafe {
            if (*raw).tag != sys::GHOSTTY_TERMINAL_RESIZE_WINDOW_GRID {
                return;
            }
            let grid = (*raw).value.grid;
            ResizeWindow::Grid {
                rows: grid.rows,
                columns: grid.columns,
            }
        };
        if let Some(f) = slot.function.as_mut() {
            f(request);
        }
    });
}

unsafe extern "C" fn unknown_sequence(
    _: sys::GhosttyTerminal,
    userdata: *mut c_void,
    raw: *const sys::GhosttyTerminalUnknownSequence,
) {
    let slot = callback_slot!(userdata, unknown_sequence);
    slot.panic.catch(|| {
        // SAFETY: the payload and strings are borrowed for this callback; only
        // read the union member matching the tag. Ignore future unknown tags.
        let sequence = unsafe {
            match (*raw).tag {
                sys::GHOSTTY_TERMINAL_UNKNOWN_SEQUENCE_APC => {
                    let apc = (*raw).value.apc;
                    UnknownSequence::Apc {
                        content: ffi::string_bytes(apc.content),
                        truncated: apc.truncated,
                    }
                }
                sys::GHOSTTY_TERMINAL_UNKNOWN_SEQUENCE_OSC => {
                    let osc = (*raw).value.osc;
                    UnknownSequence::Osc {
                        content: ffi::string_bytes(osc.content),
                        truncated: osc.truncated,
                        terminator: crate::osc::OscTerminator::from_lib(osc.terminator),
                    }
                }
                _ => return,
            }
        };
        if let Some(f) = slot.function.as_mut() {
            f(sequence);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn enquiry_and_version_replies_survive_callback_return() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        let output = Arc::new(Mutex::new(Vec::new()));
        let capture = output.clone();
        terminal.on_write_pty(move |bytes| capture.lock().unwrap().extend_from_slice(bytes));
        terminal.on_enquiry(|| b"answer".to_vec());
        terminal.on_xtversion(|| b"example 1.0".to_vec());
        terminal.vt_write(b"\x05\x1b[>q");
        assert_eq!(&*output.lock().unwrap(), b"answer\x1bP>|example 1.0\x1b\\");
        terminal.clear_enquiry();
        output.lock().unwrap().clear();
        terminal.vt_write(b"\x05");
        assert!(output.lock().unwrap().is_empty());
    }

    #[test]
    fn clipboard_read_replies_reenter_write_pty() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        let output = Arc::new(Mutex::new(Vec::new()));
        let capture = output.clone();
        terminal.on_write_pty(move |bytes| capture.lock().unwrap().extend_from_slice(bytes));
        terminal.on_clipboard_read(|read| {
            assert_eq!(read.location(), ClipboardLocation::Standard);
            assert_eq!(read.mimes()[0].as_bytes(), b"text/plain");
            read.reply(
                ClipboardReadResult::Success,
                &[ClipboardContent::new(b"text/plain", b"hello")],
                &[],
                false,
            );
        });
        terminal.vt_write(b"\x1b]52;c;?\x07");
        assert_eq!(&*output.lock().unwrap(), b"\x1b]52;c;aGVsbG8=\x07");
    }

    #[test]
    fn callback_panic_resumes_after_ffi_returns() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        terminal.on_bell(|| panic!("bell panic"));
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            terminal.vt_write(b"\x07");
        }));
        assert!(panic.is_err());
        terminal.clear_bell();
        terminal.vt_write(b"safe after panic");
    }

    #[test]
    fn default_device_reply_and_bell_replacement_and_clear() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        let output = Arc::new(Mutex::new(Vec::new()));
        let capture = output.clone();
        terminal.on_write_pty(move |bytes| capture.lock().unwrap().extend_from_slice(bytes));
        let bells = Arc::new(Mutex::new(Vec::new()));
        let first = bells.clone();
        terminal.on_bell(move || first.lock().unwrap().push(1));
        terminal.vt_write(b"\x1b[c\x07");
        assert_eq!(&*output.lock().unwrap(), b"\x1b[?62;22c");
        assert_eq!(&*bells.lock().unwrap(), &[1]);

        let replacement = bells.clone();
        terminal.on_bell(move || replacement.lock().unwrap().push(2));
        terminal.vt_write(b"\x07");
        assert_eq!(&*bells.lock().unwrap(), &[1, 2]);
        terminal.clear_bell();
        terminal.vt_write(b"\x07");
        assert_eq!(&*bells.lock().unwrap(), &[1, 2]);
    }

    #[test]
    fn title_and_pwd_callbacks_borrow_current_bytes() {
        let mut terminal = Terminal::new(80, 24).unwrap();
        let values = Arc::new(Mutex::new(Vec::new()));
        let titles = values.clone();
        terminal.on_title_changed(move |bytes| titles.lock().unwrap().push(bytes.to_vec()));
        let paths = values.clone();
        terminal.on_pwd_changed(move |bytes| paths.lock().unwrap().push(bytes.to_vec()));
        terminal.vt_write(b"\x1b]2;hello\x07\x1b]7;file:///tmp\x07\x1b]7;\x07");
        assert_eq!(
            &*values.lock().unwrap(),
            &[b"hello".to_vec(), b"file:///tmp".to_vec(), Vec::new()]
        );
    }
}
