//! Mouse selection gestures and selection text. libghostty's grid
//! references and selections point into the terminal's pages, so they are
//! resolved and applied here and never leave this module.

use core::ffi::c_void;
use core::{ptr, slice};

use ghostty_sys::*;

use super::{SurfaceSize, Terminal};

/// A pointer position for a selection gesture: the viewport cell under it
/// (clamped to the grid) and its position on the surface in device pixels.
#[derive(Clone, Copy, Debug)]
pub struct GesturePoint {
    pub col: u16,
    pub row: u16,
    pub x: f32,
    pub y: f32,
}

/// Longest pause between the clicks of a double or triple click, the
/// common platform default.
const REPEAT_INTERVAL_NS: u64 = 500_000_000;

/// libghostty's gesture state machine (multi-click detection, drag
/// thresholds, word/line expansion) with one reusable event per kind.
pub(super) struct Gesture {
    gesture: GhosttySelectionGesture,
    press: GhosttySelectionGestureEvent,
    drag: GhosttySelectionGestureEvent,
    release: GhosttySelectionGestureEvent,
}

impl Gesture {
    pub(super) fn new() -> Self {
        let new_event = |kind| {
            let mut event = ptr::null_mut();
            let result =
                unsafe { ghostty_selection_gesture_event_new(ptr::null(), &mut event, kind) };
            assert_eq!(result, GHOSTTY_SUCCESS);
            event
        };
        let mut gesture = ptr::null_mut();
        unsafe {
            assert_eq!(
                ghostty_selection_gesture_new(ptr::null(), &mut gesture),
                GHOSTTY_SUCCESS
            );
        }
        let press = new_event(GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_PRESS);
        // libghostty's default interval is zero, which never repeats.
        set_option(
            press,
            GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_INTERVAL_NS,
            &REPEAT_INTERVAL_NS,
        );
        Self {
            gesture,
            press,
            drag: new_event(GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_DRAG),
            release: new_event(GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_RELEASE),
        }
    }

    /// Free the gesture, unpinning what it tracks in `terminal`.
    ///
    /// # Safety
    /// `terminal` must be the live terminal this gesture was used with, and
    /// the gesture must not be used afterwards.
    pub(super) unsafe fn free(&mut self, terminal: GhosttyTerminal) {
        unsafe {
            ghostty_selection_gesture_event_free(self.press);
            ghostty_selection_gesture_event_free(self.drag);
            ghostty_selection_gesture_event_free(self.release);
            ghostty_selection_gesture_free(self.gesture, terminal);
        }
    }
}

fn set_option<T>(
    event: GhosttySelectionGestureEvent,
    option: GhosttySelectionGestureEventOption,
    value: &T,
) {
    unsafe {
        ghostty_selection_gesture_event_set(event, option, value as *const T as *const c_void)
    };
}

impl Terminal {
    /// Point `event` at the cell and surface position of `point`. Returns
    /// `false` if the cell has no grid reference.
    fn set_gesture_point(&self, event: GhosttySelectionGestureEvent, point: &GesturePoint) -> bool {
        let Some(grid_ref) = self.viewport_ref(point.col, point.row) else {
            return false;
        };
        set_option(event, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF, &grid_ref);
        let position = GhosttySurfacePosition {
            x: point.x as f64,
            y: point.y as f64,
        };
        set_option(
            event,
            GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION,
            &position,
        );
        true
    }

    fn run_gesture(&mut self, event: GhosttySelectionGestureEvent) -> Option<GhosttySelection> {
        let mut selection = GhosttySelection::default();
        let result = unsafe {
            ghostty_selection_gesture_event(self.gesture.gesture, self.raw, event, &mut selection)
        };
        (result == GHOSTTY_SUCCESS).then_some(selection)
    }

    /// Mouse press. `time_ns` is a monotonic timestamp for multi-click
    /// detection. A single click clears the selection and anchors a new
    /// one; double/triple clicks (within a cell's width of the first)
    /// select a word/line immediately.
    pub fn selection_press(&mut self, point: &GesturePoint, surface: SurfaceSize, time_ns: u64) {
        let event = self.gesture.press;
        if !self.set_gesture_point(event, point) {
            return;
        }
        set_option(event, GHOSTTY_SELECTION_GESTURE_EVENT_OPT_TIME_NS, &time_ns);
        let distance = surface.cell_width as f64;
        set_option(
            event,
            GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_DISTANCE,
            &distance,
        );
        let selection = self.run_gesture(event);
        self.set_selection(selection.as_ref());
    }

    /// Mouse drag with the button held; `rectangle` selects a block.
    /// Returns whether the selection changed.
    pub fn selection_drag(
        &mut self,
        point: &GesturePoint,
        surface: SurfaceSize,
        columns: u16,
        rectangle: bool,
    ) -> bool {
        let event = self.gesture.drag;
        if !self.set_gesture_point(event, point) {
            return false;
        }
        let geometry = GhosttySelectionGestureGeometry {
            columns: columns as u32,
            cell_width: surface.cell_width,
            padding_left: surface.padding,
            screen_height: surface.height,
        };
        set_option(
            event,
            GHOSTTY_SELECTION_GESTURE_EVENT_OPT_GEOMETRY,
            &geometry,
        );
        set_option(
            event,
            GHOSTTY_SELECTION_GESTURE_EVENT_OPT_RECTANGLE,
            &rectangle,
        );
        let Some(selection) = self.run_gesture(event) else {
            return false;
        };
        self.set_selection(Some(&selection));
        true
    }

    /// Mouse release, at `point` if the pointer is over the grid.
    pub fn selection_release(&mut self, point: Option<&GesturePoint>) {
        let event = self.gesture.release;
        if !point.is_some_and(|point| self.set_gesture_point(event, point)) {
            unsafe {
                ghostty_selection_gesture_event_set(
                    event,
                    GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF,
                    ptr::null(),
                );
            }
        }
        self.run_gesture(event);
    }

    pub fn clear_selection(&mut self) {
        self.set_selection(None);
    }

    pub fn select_all(&mut self) {
        let mut selection = GhosttySelection::default();
        if unsafe { ghostty_terminal_select_all(self.raw, &mut selection) } == GHOSTTY_SUCCESS {
            self.set_selection(Some(&selection));
        }
    }

    fn set_selection(&mut self, selection: Option<&GhosttySelection>) {
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
}
