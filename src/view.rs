//! The terminal view: owns the libghostty terminal and the shell's pty, and
//! routes keyboard, IME, mouse and clipboard input between them.

use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, Font, KeyDownEvent, KeyUpEvent, ModifiersChangedEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, PromptLevel, Render, RenderImage, ScrollDelta,
    ScrollWheelEvent, SharedString, Subscription, Task, UTF16Selection, Window, actions, div, font,
    prelude::*, px, size,
};

use crate::clipboard::{self, Content};
use crate::element::TerminalElement;
use crate::grid::GridLayout;
use crate::i18n::tr;
use crate::images::ImageCache;
use crate::keys;
use crate::links::{self, Link};
use crate::pty::{Pty, PtyEvent};
use crate::settings::{MAX_FONT_SIZE, MIN_FONT_SIZE, Settings};
use crate::shell::ShellKind;
use crate::theme;
use crate::utf16;
use crate::vt::{
    self, Frame, ImagePlacement, KeyAction, KeyEncoder, KeyInput, MouseAction, MouseEncoder,
    MouseInput, PasteOutcome, RenderState, Scrollbar, Terminal,
};

actions!(
    terminal,
    [
        Copy,
        Paste,
        SelectAll,
        IncreaseFontSize,
        DecreaseFontSize,
        ResetFontSize,
    ]
);

pub const KEY_CONTEXT: &str = "Terminal";
const MODE_FOCUS_EVENT: u16 = 1004;
const MODE_ALTERNATE_SCROLL: u16 = 1007;
/// Shortest wait between animation redraws (about 120 Hz).
const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(8);
/// Mouse buttons the view listens to, for presses and releases alike.
const MOUSE_BUTTONS: [MouseButton; 3] = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];

pub enum TerminalEvent {
    TitleChanged,
    /// The shell exited.
    Exited,
}

/// IME composition text, shown at the cursor until committed. Offsets the
/// platform exchanges are UTF-16 and relative to this text.
#[derive(Default)]
struct Composition {
    text: String,
    /// Caret or selection within `text`, in UTF-16 units.
    selected: Range<usize>,
}

pub struct TerminalView {
    terminal: Terminal,
    render_state: RenderState,
    frame: Frame,
    key_encoder: KeyEncoder,
    mouse_encoder: MouseEncoder,
    pty: Option<Pty>,
    focus_handle: FocusHandle,
    font_family: String,
    font_size: Pixels,
    /// Font size from the settings; zooming changes `font_size` only.
    base_font_size: Pixels,
    layout: Option<GridLayout>,
    composition: Composition,
    /// Fractional scroll lines carried between wheel events.
    scroll_remainder: f32,
    /// Button currently reported to the program via mouse tracking.
    reported_button: Option<vt::MouseButton>,
    selecting: bool,
    /// Scroll position as of the last frame.
    scrollbar: Scrollbar,
    /// While dragging the scrollbar thumb: the pointer's distance from the
    /// thumb's top edge.
    scrollbar_drag: Option<Pixels>,
    scrollbar_hovered: bool,
    /// Last pointer position in window coordinates.
    pointer: Option<Point<Pixels>>,
    /// Cmd (Ctrl off macOS) is held: links in the text are found too, and
    /// links open on click.
    link_modifier: bool,
    /// The cell `hovered_link` was resolved for.
    hovered_cell: Option<(u16, u16)>,
    hovered_link: Option<Link>,
    /// Title set by the program (OSC 0/2); empty if none.
    title: SharedString,
    epoch: Instant,
    image_cache: ImageCache,
    /// Redraw for the next Kitty graphics animation frame.
    animation: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    _io: Task<()>,
}

impl TerminalView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (cols, rows) = (80, 24);
        let mut terminal = Terminal::new(cols, rows);
        let settings = Settings::get(cx);
        let font_family = settings.font_family.clone();
        let font_size = settings.font_size();
        terminal.set_colors(&theme::terminal_colors(settings.flavor(window.appearance())));

        let (pty, events) = match Pty::spawn(settings.shell(), cols, rows, 8, 16) {
            Ok((pty, events)) => (Some(pty), Some(events)),
            Err(err) => {
                terminal.vt_write(format!("\x1b[31mfailed to start shell: {err}\x1b[0m\r\n").as_bytes());
                (None, None)
            }
        };

        let io = match events {
            Some(mut events) => cx.spawn(async move |this, cx| {
                while let Some(event) = events.next().await {
                    let mut exited = matches!(event, PtyEvent::Exited);
                    let mut output = match event {
                        PtyEvent::Output(bytes) => bytes,
                        PtyEvent::Exited => Vec::new(),
                    };
                    // Coalesce whatever else is already queued into one update.
                    while !exited && let Ok(event) = events.try_recv() {
                        match event {
                            PtyEvent::Output(bytes) => output.extend_from_slice(&bytes),
                            PtyEvent::Exited => exited = true,
                        }
                    }
                    let updated = this.update(cx, |view, cx| {
                        if !output.is_empty() {
                            view.process_output(&output, cx);
                        }
                        if exited {
                            cx.emit(TerminalEvent::Exited);
                        }
                    });
                    if updated.is_err() || exited {
                        break;
                    }
                }
            }),
            None => Task::ready(()),
        };

        let focus_handle = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus_handle, window, |view, _, cx| view.focus_changed(true, cx)),
            cx.on_blur(&focus_handle, window, |view, _, cx| view.focus_changed(false, cx)),
            cx.observe_global_in::<Settings>(window, Self::apply_settings),
            cx.observe_window_appearance(window, Self::apply_theme),
        ];
        focus_handle.focus(window);

        Self {
            terminal,
            render_state: RenderState::new(),
            frame: Frame::default(),
            key_encoder: KeyEncoder::new(),
            mouse_encoder: MouseEncoder::new(),
            pty,
            focus_handle,
            font_family,
            font_size,
            base_font_size: font_size,
            layout: None,
            composition: Composition::default(),
            scroll_remainder: 0.,
            reported_button: None,
            selecting: false,
            scrollbar: Scrollbar::default(),
            scrollbar_drag: None,
            scrollbar_hovered: false,
            pointer: None,
            link_modifier: false,
            hovered_cell: None,
            hovered_link: None,
            title: SharedString::default(),
            epoch: Instant::now(),
            image_cache: ImageCache::default(),
            animation: None,
            _subscriptions: subscriptions,
            _io: io,
        }
    }

    pub fn font(&self) -> Font {
        font(self.font_family.clone())
    }

    pub fn font_size(&self) -> Pixels {
        self.font_size
    }

    pub fn marked_text(&self) -> &str {
        &self.composition.text
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    pub fn focus(&self, window: &mut Window) {
        self.focus_handle.focus(window);
    }

    /// Title set by the program, or empty.
    pub fn title(&self) -> &SharedString {
        &self.title
    }

    /// The scrollbar thumb's slot as of the last frame, and whether it is
    /// highlighted (hovered or dragged).
    pub fn scrollbar_thumb(&self) -> Option<(Bounds<Pixels>, bool)> {
        let thumb = self.layout?.scrollbar_thumb(self.scrollbar)?;
        Some((thumb, self.scrollbar_hovered || self.scrollbar_drag.is_some()))
    }

    /// Apply the grid size the element computed, resizing the terminal and
    /// the pty when the cell grid changes.
    pub fn set_layout(&mut self, layout: GridLayout) {
        let resized = self.layout.is_none_or(|old| old.grid_changed(&layout));
        self.layout = Some(layout);
        if !resized {
            return;
        }
        let cell_width = layout.device(layout.cell_width);
        let cell_height = layout.device(layout.line_height);
        self.terminal
            .resize(layout.cols, layout.rows, cell_width, cell_height);
        if let Some(pty) = &self.pty {
            pty.resize(layout.cols, layout.rows, cell_width as u16, cell_height as u16);
        }
        self.flush_replies();
    }

    /// Refresh the frame to draw, advancing image animations first.
    pub fn snapshot(&mut self, cx: &mut Context<Self>) -> &Frame {
        self.tick_animations(cx);
        self.render_state.snapshot(&self.terminal, &mut self.frame);
        self.scrollbar = self.terminal.scrollbar();
        // Output may have moved or replaced the link under the pointer.
        self.hovered_cell = self.pointer_cell();
        self.hovered_link = self.link_at(self.hovered_cell);
        &self.frame
    }

    /// The link to underline and preview, as of the last frame.
    pub fn hovered_link(&self) -> Option<&Link> {
        self.hovered_link.as_ref()
    }

    /// Whether a click would open the hovered link.
    pub fn link_clickable(&self) -> bool {
        self.link_modifier
    }

    fn pointer_cell(&self) -> Option<(u16, u16)> {
        self.layout?.cell_containing(self.pointer?)
    }

    /// OSC 8 hyperlinks are shown on hover; URLs and paths in the text only
    /// while the link modifier is held, since finding them is a guess.
    fn link_at(&self, cell: Option<(u16, u16)>) -> Option<Link> {
        let (col, row) = cell?;
        let text_links = self
            .link_modifier
            .then_some(|| self.pty.as_ref()?.foreground_cwd());
        links::link_at(&self.terminal, &self.frame, col, row, text_links)
    }

    /// The pointer left the window.
    pub fn mouse_exit(&mut self, cx: &mut Context<Self>) {
        self.pointer = None;
        self.update_hovered_link(false, cx);
    }

    /// Resolve the hovered link again if the pointer is over another cell
    /// than last time, or if `force`d because what links are shown changed.
    fn update_hovered_link(&mut self, force: bool, cx: &mut Context<Self>) {
        let cell = self.pointer_cell();
        if !force && cell == self.hovered_cell {
            return;
        }
        self.hovered_cell = cell;
        let link = self.link_at(cell);
        if link != self.hovered_link {
            self.hovered_link = link;
            cx.notify();
        }
    }

    fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_link_modifier(event.modifiers.secondary(), cx);
    }

    fn set_link_modifier(&mut self, held: bool, cx: &mut Context<Self>) {
        let changed = held != self.link_modifier;
        if changed {
            self.link_modifier = held;
            // A hovered link's cursor changes even when the link stays.
            if self.hovered_link.is_some() {
                cx.notify();
            }
        }
        self.update_hovered_link(changed, cx);
    }

    /// The last snapshot's images with their textures.
    pub fn images(&mut self, window: &mut Window) -> Vec<(ImagePlacement, Arc<RenderImage>)> {
        self.image_cache
            .update(&self.terminal, &self.frame.images, window)
    }

    /// Show the animation frames due now and schedule a redraw for the next
    /// one. Programs start animations by writing, which redraws anyway.
    fn tick_animations(&mut self, cx: &mut Context<Self>) {
        let now = self.epoch.elapsed().as_millis() as u64;
        self.animation = self.terminal.tick_animations(now).map(|next| {
            let delay = Duration::from_millis(next.saturating_sub(now)).max(MIN_FRAME_INTERVAL);
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
        });
    }

    fn apply_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = Settings::get(cx);
        if self.font_family != settings.font_family {
            self.font_family = settings.font_family.clone();
        }
        if self.base_font_size != settings.font_size() {
            self.base_font_size = settings.font_size();
            self.font_size = self.base_font_size;
        }
        self.apply_theme(window, cx);
    }

    fn apply_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let flavor = Settings::get(cx).flavor(window.appearance());
        self.terminal.set_colors(&theme::terminal_colors(flavor));
        cx.notify();
    }

    fn process_output(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        self.terminal.vt_write(bytes);
        self.flush_replies();
        if let Some(title) = self.terminal.take_title_change() {
            self.title = title.into();
            cx.emit(TerminalEvent::TitleChanged);
        }
        cx.notify();
    }

    fn write_to_pty(&self, bytes: Vec<u8>) {
        if let Some(pty) = &self.pty {
            pty.write(bytes);
        }
    }

    /// Forward replies the terminal generated (device status, paste data, …).
    fn flush_replies(&mut self) {
        let replies = self.terminal.take_pty_output();
        self.write_to_pty(replies);
    }

    /// Send user-typed bytes: like other terminals, typing returns the
    /// viewport to the bottom and clears the selection.
    fn send_user_input(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        if bytes.is_empty() {
            return;
        }
        self.terminal.scroll_to_bottom();
        self.terminal.clear_selection();
        self.write_to_pty(bytes);
        cx.notify();
    }

    fn focus_changed(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.terminal.dec_mode(MODE_FOCUS_EVENT) {
            let mut out = Vec::new();
            vt::encode_focus(focused, &mut out);
            self.write_to_pty(out);
        }
        cx.notify();
    }

    // ---- keyboard ---------------------------------------------------------

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // While the IME is composing, it owns every key.
        if !self.composition.text.is_empty() {
            return;
        }
        // Ctrl+V pastes copied files and images as paths. Otherwise it stays
        // a key for the program (^V: literal next in shells, block selection
        // in Vim), which may read an image from the clipboard itself.
        let m = &event.keystroke.modifiers;
        if event.keystroke.key == "v"
            && m.control
            && !(m.shift || m.alt || m.platform || m.function)
            && let Some(content @ (Content::Paths(_) | Content::Image(_))) = clipboard::read(cx)
        {
            cx.stop_propagation();
            self.paste_content(content, window, cx);
            return;
        }
        let mut text = String::new();
        let Some(mut input) = keys::encodable(&event.keystroke, &mut text) else {
            return;
        };
        if event.is_held {
            input.action = KeyAction::Repeat;
        }
        let mut out = Vec::new();
        self.key_encoder.encode(&self.terminal, &input, &mut out);
        cx.stop_propagation();
        self.send_user_input(out, cx);
    }

    fn key_up(&mut self, event: &KeyUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // Releases only matter to programs using the Kitty keyboard protocol
        // with event reporting.
        if !self.terminal.reports_key_releases() {
            return;
        }
        let mut text = String::new();
        let Some(input) = keys::encodable(&event.keystroke, &mut text) else {
            return;
        };
        let input = KeyInput {
            action: KeyAction::Release,
            ..input
        };
        let mut out = Vec::new();
        self.key_encoder.encode(&self.terminal, &input, &mut out);
        cx.stop_propagation();
        self.write_to_pty(out);
    }

    // ---- actions ----------------------------------------------------------

    fn copy(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.terminal.selection_text().filter(|text| !text.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(content) = clipboard::read(cx) {
            self.paste_content(content, window, cx);
        }
    }

    /// Quoting rules for pasted paths: the shell's, or POSIX without one.
    fn shell(&self) -> ShellKind {
        self.pty.as_ref().map_or(ShellKind::Posix, Pty::shell)
    }

    fn paste_content(&mut self, content: Content, window: &mut Window, cx: &mut Context<Self>) {
        let image = match content {
            Content::Text(text) => return self.paste_or_confirm(text, window, cx),
            Content::Paths(paths) => {
                let text = self.shell().quote_paths(&paths);
                return self.paste_or_confirm(text, window, cx);
            }
            Content::Image(image) => image,
        };
        // Saving may mean converting a large TIFF: keep it off the UI thread.
        let saved = cx
            .background_executor()
            .spawn(async move { clipboard::save_image(&image) });
        cx.spawn_in(window, async move |this, cx| match saved.await {
            Ok(path) => {
                let _ = this.update_in(cx, |view, window, cx| {
                    let text = view.shell().quote_paths(&[path]);
                    view.paste_or_confirm(text, window, cx)
                });
            }
            Err(err) => eprintln!("failed to save the clipboard image: {err}"),
        })
        .detach();
    }

    fn paste_or_confirm(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.paste_text(&text, false, cx) != PasteOutcome::Unsafe {
            return;
        }
        // The text could run commands on its own (e.g. contains a newline
        // outside bracketed paste): ask first.
        let [title, detail, confirm, cancel] = [
            "paste-unsafe-title",
            "paste-unsafe-detail",
            "paste-unsafe-confirm",
            "paste-unsafe-cancel",
        ]
        .map(|id| tr(cx, id));
        let answer = window.prompt(
            PromptLevel::Warning,
            &title,
            Some(&detail),
            &[confirm.as_ref(), cancel.as_ref()],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, |view, cx| view.paste_text(&text, true, cx));
            }
        })
        .detach();
    }

    fn paste_text(&mut self, text: &str, allow_unsafe: bool, cx: &mut Context<Self>) -> PasteOutcome {
        let outcome = self.terminal.paste(text, allow_unsafe);
        match outcome {
            PasteOutcome::Written => {
                let out = self.terminal.take_pty_output();
                self.send_user_input(out, cx);
            }
            PasteOutcome::Failed => eprintln!("failed to paste {} bytes", text.len()),
            PasteOutcome::Empty | PasteOutcome::Unsafe => {}
        }
        outcome
    }

    fn select_all(&mut self, _: &SelectAll, _window: &mut Window, cx: &mut Context<Self>) {
        self.terminal.select_all();
        cx.notify();
    }

    fn change_font_size(&mut self, size: Pixels, cx: &mut Context<Self>) {
        self.font_size = size.clamp(px(MIN_FONT_SIZE), px(MAX_FONT_SIZE));
        cx.notify();
    }

    // ---- mouse ------------------------------------------------------------

    /// Whether mouse events go to the program instead of driving selection.
    /// Holding Shift always selects, as in other terminals.
    fn reports_mouse(&self, shift: bool) -> bool {
        !shift && self.terminal.mouse_tracking()
    }

    fn report_mouse(
        &mut self,
        action: MouseAction,
        button: Option<vt::MouseButton>,
        position: Point<Pixels>,
        modifiers: &gpui::Modifiers,
    ) {
        let Some(layout) = self.layout else { return };
        let (x, y) = layout.surface_position(position);
        let input = MouseInput {
            action,
            button,
            any_button_pressed: self.reported_button.is_some(),
            mods: keys::ghostty_mods(modifiers),
            x,
            y,
        };
        let mut out = Vec::new();
        self.mouse_encoder
            .encode(&self.terminal, layout.surface_size(), &input, &mut out);
        self.write_to_pty(out);
    }

    /// Start a thumb drag if the press hit the scrollbar. Pressing the track
    /// outside the thumb jumps there and keeps dragging from the thumb's
    /// center.
    fn scrollbar_press(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(layout) = self.layout else { return false };
        let Some(thumb) = layout.scrollbar_thumb(self.scrollbar) else {
            return false;
        };
        if event.button != MouseButton::Left
            || !layout.scrollbar_track().contains(&event.position)
        {
            return false;
        }
        let grab = if thumb.contains(&event.position) {
            event.position.y - thumb.top()
        } else {
            let grab = thumb.size.height / 2.;
            let offset = layout.scrollbar_offset_at(self.scrollbar, event.position.y - grab);
            self.terminal.scroll_to_row(offset);
            grab
        };
        self.scrollbar_drag = Some(grab);
        cx.notify();
        true
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window);
        if self.scrollbar_press(event, cx) {
            return;
        }
        // Cmd+click (Ctrl+click off macOS) opens a link, even over programs
        // that use the mouse.
        if event.button == MouseButton::Left && event.modifiers.secondary() {
            self.pointer = Some(event.position);
            self.link_modifier = true;
            if let Some(link) = self.link_at(self.pointer_cell()) {
                cx.open_url(&link.url);
                return;
            }
        }
        if self.reports_mouse(event.modifiers.shift) {
            let button = keys::mouse_button(event.button);
            self.reported_button = Some(button);
            self.report_mouse(MouseAction::Press, Some(button), event.position, &event.modifiers);
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        let Some(layout) = self.layout else { return };
        let now = self.epoch.elapsed().as_nanos() as u64;
        let point = layout.gesture_point(event.position);
        self.terminal
            .selection_press(&point, layout.surface_size(), now);
        self.selecting = true;
        cx.notify();
    }

    /// Registered window-wide by the element so drags keep working when the
    /// pointer leaves the terminal.
    pub fn mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = self.layout else { return };
        self.pointer = Some(event.position);
        self.set_link_modifier(event.modifiers.secondary(), cx);
        if let Some(grab) = self.scrollbar_drag {
            let offset = layout.scrollbar_offset_at(self.scrollbar, event.position.y - grab);
            self.terminal.scroll_to_row(offset);
            cx.notify();
            return;
        }
        let hovered = self.scrollbar.scrollable()
            && layout.scrollbar_track().contains(&event.position);
        if hovered != self.scrollbar_hovered {
            self.scrollbar_hovered = hovered;
            cx.notify();
        }
        if self.reports_mouse(event.modifiers.shift) {
            if layout.bounds.contains(&event.position) || self.reported_button.is_some() {
                self.report_mouse(
                    MouseAction::Motion,
                    self.reported_button,
                    event.position,
                    &event.modifiers,
                );
            }
            return;
        }
        if !self.selecting || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        // Alt-drag selects a rectangle, like Ghostty.
        let rectangle = event.modifiers.alt;
        let point = layout.gesture_point(event.position);
        if self
            .terminal
            .selection_drag(&point, layout.surface_size(), layout.cols, rectangle)
        {
            cx.notify();
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.button == MouseButton::Left && self.scrollbar_drag.take().is_some() {
            cx.notify();
            return;
        }
        let button = keys::mouse_button(event.button);
        if self.reported_button == Some(button) {
            self.reported_button = None;
            self.report_mouse(MouseAction::Release, Some(button), event.position, &event.modifiers);
            return;
        }
        if self.selecting && event.button == MouseButton::Left {
            self.selecting = false;
            let point = self.layout.map(|layout| layout.gesture_point(event.position));
            self.terminal.selection_release(point.as_ref());
            cx.notify();
        }
    }

    fn scroll_wheel(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = self.layout else { return };
        let delta = match event.delta {
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / f32::from(layout.line_height),
            ScrollDelta::Lines(delta) => delta.y,
        };
        self.scroll_remainder += delta;
        let lines = self.scroll_remainder.trunc();
        self.scroll_remainder -= lines;
        if lines == 0. {
            return;
        }
        let count = lines.abs() as usize;
        // Positive deltas scroll toward older content (wheel up).
        let up = lines > 0.;

        if self.reports_mouse(event.modifiers.shift) {
            let button = if up {
                vt::MouseButton::Four
            } else {
                vt::MouseButton::Five
            };
            for _ in 0..count {
                self.report_mouse(MouseAction::Press, Some(button), event.position, &event.modifiers);
            }
        } else if self.terminal.alternate_screen() && self.terminal.dec_mode(MODE_ALTERNATE_SCROLL)
        {
            // Full-screen programs without mouse support (less, man) get
            // arrow keys instead.
            let input = keys::arrow(up);
            let mut out = Vec::new();
            for _ in 0..count {
                self.key_encoder.encode(&self.terminal, &input, &mut out);
            }
            self.write_to_pty(out);
        } else {
            let rows = count as isize;
            self.terminal.scroll_by(if up { -rows } else { rows });
            cx.notify();
        }
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let text = &self.composition.text;
        let start = utf16::to_byte(text, range.start);
        let end = utf16::to_byte(text, range.end);
        adjusted_range.replace(utf16::from_byte(text, start)..utf16::from_byte(text, end));
        Some(text[start..end].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.composition.selected.clone(),
            reversed: false,
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        let text = &self.composition.text;
        (!text.is_empty()).then(|| 0..utf16::len(text))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.composition = Composition::default();
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Committed text (typed characters or an IME result) goes to the
        // program verbatim.
        self.composition = Composition::default();
        self.send_user_input(text.as_bytes().to_vec(), cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Without a range the whole composition is replaced.
        let composition = &mut self.composition;
        let replaced = match range {
            Some(range) => {
                utf16::to_byte(&composition.text, range.start)
                    ..utf16::to_byte(&composition.text, range.end)
            }
            None => 0..composition.text.len(),
        };
        composition.text.replace_range(replaced.clone(), new_text);
        // The new selection is relative to the inserted text.
        let start = utf16::from_byte(&composition.text, replaced.start);
        composition.selected = match new_selected_range {
            Some(selected) => start + selected.start..start + selected.end,
            None => {
                let end = start + utf16::len(new_text);
                end..end
            }
        };
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // Anchor the IME candidate window at the cursor cell.
        let layout = self.layout?;
        let cursor = self.frame.cursor?;
        let origin = layout.cell_origin(cursor.x as f32 + range.start as f32, cursor.y as f32);
        Some(Bounds::new(origin, size(layout.cell_width, layout.line_height)))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl EventEmitter<TerminalEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .id("terminal")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(|view, _: &IncreaseFontSize, _, cx| {
                view.change_font_size(view.font_size + px(1.), cx)
            }))
            .on_action(cx.listener(|view, _: &DecreaseFontSize, _, cx| {
                view.change_font_size(view.font_size - px(1.), cx)
            }))
            .on_action(cx.listener(|view, _: &ResetFontSize, _, cx| {
                view.change_font_size(view.base_font_size, cx)
            }))
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .on_scroll_wheel(cx.listener(Self::scroll_wheel));
        // Releases outside the terminal end drags and reported presses too.
        for button in MOUSE_BUTTONS {
            root = root
                .on_mouse_down(button, cx.listener(Self::mouse_down))
                .on_mouse_up(button, cx.listener(Self::mouse_up))
                .on_mouse_up_out(button, cx.listener(Self::mouse_up));
        }
        root.child(TerminalElement::new(cx.entity()))
    }
}

/// GPUI's element input handler with macOS press-and-hold disabled, so a held
/// key repeats as in other terminals instead of opening the accent picker.
pub struct TerminalInputHandler(gpui::ElementInputHandler<TerminalView>);

impl TerminalInputHandler {
    pub fn new(bounds: Bounds<Pixels>, view: gpui::Entity<TerminalView>) -> Self {
        Self(gpui::ElementInputHandler::new(bounds, view))
    }
}

impl gpui::InputHandler for TerminalInputHandler {
    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<UTF16Selection> {
        self.0.selected_text_range(ignore_disabled_input, window, cx)
    }

    fn marked_text_range(&mut self, window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.0.marked_text_range(window, cx)
    }

    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<String> {
        self.0.text_for_range(range_utf16, adjusted_range, window, cx)
    }

    fn replace_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0.replace_text_in_range(replacement_range, text, window, cx)
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0
            .replace_and_mark_text_in_range(range_utf16, new_text, new_selected_range, window, cx)
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        self.0.unmark_text(window, cx)
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        self.0.bounds_for_range(range_utf16, window, cx)
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<usize> {
        self.0.character_index_for_point(point, window, cx)
    }

    fn apple_press_and_hold_enabled(&mut self) -> bool {
        false
    }
}
