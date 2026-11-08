//! Selections: ranges of terminal content between two cells.

use crate::{
    Error, Result, ffi,
    point::{Coordinate, Point},
};
use core::ptr::{self, NonNull};
use ghostty_sys as sys;

use crate::grid_ref::GridRef;
use crate::terminal::Terminal;

/// A range of terminal content between two cells, both included.
///
/// Like [`GridRef`], a selection borrows its terminal and is only valid
/// until the terminal changes. `start` keeps the direction the user
/// selected in, so it may come after `end`.
#[derive(Debug, Clone, Copy)]
pub struct Selection<'t> {
    start: GridRef<'t>,
    end: GridRef<'t>,
    rectangle: bool,
}

impl<'t> Selection<'t> {
    /// A selection from `start` to `end`. A rectangular selection selects
    /// the block with `start` and `end` at opposite corners; otherwise text
    /// flows from `start` to `end` across rows.
    ///
    /// # Panics
    ///
    /// If `start` and `end` point into different terminals.
    pub fn new(start: GridRef<'t>, end: GridRef<'t>, rectangle: bool) -> Self {
        assert!(
            core::ptr::eq(start.terminal(), end.terminal()),
            "selection ends must belong to the same terminal"
        );
        Self {
            start,
            end,
            rectangle,
        }
    }

    /// The logical starting cell.
    pub fn start(&self) -> GridRef<'t> {
        self.start
    }

    /// The logical ending cell.
    pub fn end(&self) -> GridRef<'t> {
        self.end
    }

    /// Whether the selection is rectangular.
    pub fn is_rectangle(&self) -> bool {
        self.rectangle
    }

    /// The terminal this selection belongs to.
    pub fn terminal(&self) -> &'t Terminal {
        self.start.terminal()
    }

    /// Returns the C representation, valid while this terminal borrow remains valid.
    pub fn to_raw(&self) -> sys::GhosttySelection {
        sys::GhosttySelection {
            start: *self.start.as_raw(),
            end: *self.end.as_raw(),
            rectangle: self.rectangle,
        }
    }

    /// Wraps a raw selection produced by the library.
    ///
    /// # Safety
    ///
    /// Both ends of `raw` must have been produced by libghostty-vt from
    /// `terminal`, with no change to that terminal since.
    pub(crate) unsafe fn from_raw(terminal: &'t Terminal, raw: &sys::GhosttySelection) -> Self {
        // SAFETY: the caller guarantees both endpoints belong to this unchanged terminal.
        unsafe {
            Self {
                start: GridRef::from_raw(terminal, raw.start),
                end: GridRef::from_raw(terminal, raw.end),
                rectangle: raw.rectangle,
            }
        }
    }

    /// Moves the logical end using terminal selection semantics.
    pub fn adjust(&mut self, adjustment: SelectionAdjust) -> Result<()> {
        let mut raw = self.to_raw();
        unsafe {
            Error::check(sys::ghostty_terminal_selection_adjust(
                self.terminal().as_raw(),
                &mut raw,
                adjustment.to_raw(),
            ))?;
        }
        // SAFETY: adjustment preserves valid references in this borrowed terminal.
        *self = unsafe { Self::from_raw(self.terminal(), &raw) };
        Ok(())
    }
    /// Returns the endpoint ordering.
    pub fn order(&self) -> Result<SelectionOrder> {
        let mut order = 0;
        unsafe {
            Error::check(sys::ghostty_terminal_selection_order(
                self.terminal().as_raw(),
                &self.to_raw(),
                &mut order,
            ))?;
        }
        Ok(SelectionOrder::from_lib(order))
    }
    /// Returns a copy with the requested endpoint ordering.
    pub fn ordered(&self, order: SelectionOrder) -> Result<Self> {
        let mut raw = sys::GhosttySelection::default();
        unsafe {
            Error::check(sys::ghostty_terminal_selection_ordered(
                self.terminal().as_raw(),
                &self.to_raw(),
                order.to_raw(),
                &mut raw,
            ))?;
        }
        // SAFETY: the output belongs to the same immutably borrowed terminal.
        Ok(unsafe { Self::from_raw(self.terminal(), &raw) })
    }
    /// Tests whether a terminal point lies in this selection.
    pub fn contains(&self, point: Point) -> Result<bool> {
        let mut contains = false;
        unsafe {
            Error::check(sys::ghostty_terminal_selection_contains(
                self.terminal().as_raw(),
                &self.to_raw(),
                point.to_raw(),
                &mut contains,
            ))?;
        }
        Ok(contains)
    }
    /// Compares endpoints and rectangular state using terminal semantics.
    pub fn equal(&self, other: &Selection<'_>) -> Result<bool> {
        if !ptr::eq(self.terminal(), other.terminal()) {
            return Ok(false);
        }
        let mut equal = false;
        unsafe {
            Error::check(sys::ghostty_terminal_selection_equal(
                self.terminal().as_raw(),
                &self.to_raw(),
                &other.to_raw(),
                &mut equal,
            ))?;
        }
        Ok(equal)
    }
}

ffi::c_enum! {
    /// Ordering of selection endpoints.
    pub enum SelectionOrder {
        /// Top-left to bottom-right.
        Forward = sys::GHOSTTY_SELECTION_ORDER_FORWARD,
        /// Bottom-right to top-left.
        Reverse = sys::GHOSTTY_SELECTION_ORDER_REVERSE,
        /// Rectangular top-right to bottom-left.
        MirroredForward = sys::GHOSTTY_SELECTION_ORDER_MIRRORED_FORWARD,
        /// Rectangular bottom-left to top-right.
        MirroredReverse = sys::GHOSTTY_SELECTION_ORDER_MIRRORED_REVERSE,
    }
}
ffi::c_enum! {
    /// Movement of a selection's logical end.
    pub enum SelectionAdjust {
        /// Previous nonempty cell.
        Left = sys::GHOSTTY_SELECTION_ADJUST_LEFT,
        /// Next nonempty cell.
        Right = sys::GHOSTTY_SELECTION_ADJUST_RIGHT,
        /// Previous row.
        Up = sys::GHOSTTY_SELECTION_ADJUST_UP,
        /// Next nonblank row.
        Down = sys::GHOSTTY_SELECTION_ADJUST_DOWN,
        /// Screen beginning.
        Home = sys::GHOSTTY_SELECTION_ADJUST_HOME,
        /// Last nonblank row's end.
        End = sys::GHOSTTY_SELECTION_ADJUST_END,
        /// Previous page.
        PageUp = sys::GHOSTTY_SELECTION_ADJUST_PAGE_UP,
        /// Next page.
        PageDown = sys::GHOSTTY_SELECTION_ADJUST_PAGE_DOWN,
        /// Current line's beginning.
        BeginningOfLine = sys::GHOSTTY_SELECTION_ADJUST_BEGINNING_OF_LINE,
        /// Current line's end.
        EndOfLine = sys::GHOSTTY_SELECTION_ADJUST_END_OF_LINE,
    }
}

impl Terminal {
    fn selection_result(
        &self,
        f: impl FnOnce(*mut sys::GhosttySelection) -> sys::GhosttyResult,
    ) -> Result<Selection<'_>> {
        let mut raw = sys::GhosttySelection::default();
        Error::check(f(&mut raw))?;
        // SAFETY: each caller supplies an operation producing references from self.
        Ok(unsafe { Selection::from_raw(self, &raw) })
    }
    fn check_selection_ref(&self, reference: GridRef<'_>) -> Result<()> {
        if ptr::eq(self, reference.terminal()) {
            Ok(())
        } else {
            Err(Error::InvalidValue)
        }
    }
    /// Selects a word, using default boundaries when `boundaries` is empty.
    pub fn select_word(&self, reference: GridRef<'_>, boundaries: &[u32]) -> Result<Selection<'_>> {
        self.check_selection_ref(reference)?;
        // SAFETY: this header struct begins with its size field.
        let mut options: sys::GhosttyTerminalSelectWordOptions = unsafe { ffi::sized() };
        options.ref_ = *reference.as_raw();
        options.boundary_codepoints = if boundaries.is_empty() {
            ptr::null()
        } else {
            boundaries.as_ptr()
        };
        options.boundary_codepoints_len = boundaries.len();
        self.selection_result(|out| unsafe {
            sys::ghostty_terminal_select_word(self.as_raw(), &options, out)
        })
    }
    /// Selects the first word found from `start` toward `end`, inclusively.
    pub fn select_word_between(
        &self,
        start: GridRef<'_>,
        end: GridRef<'_>,
        boundaries: &[u32],
    ) -> Result<Selection<'_>> {
        self.check_selection_ref(start)?;
        self.check_selection_ref(end)?;
        // SAFETY: this header struct begins with its size field.
        let mut options: sys::GhosttyTerminalSelectWordBetweenOptions = unsafe { ffi::sized() };
        options.start = *start.as_raw();
        options.end = *end.as_raw();
        options.boundary_codepoints = if boundaries.is_empty() {
            ptr::null()
        } else {
            boundaries.as_ptr()
        };
        options.boundary_codepoints_len = boundaries.len();
        self.selection_result(|out| unsafe {
            sys::ghostty_terminal_select_word_between(self.as_raw(), &options, out)
        })
    }
    /// Selects a logical line with optional trimming codepoints and semantic boundaries.
    pub fn select_line(
        &self,
        reference: GridRef<'_>,
        whitespace: &[u32],
        semantic_prompt_boundary: bool,
    ) -> Result<Selection<'_>> {
        self.check_selection_ref(reference)?;
        // SAFETY: this header struct begins with its size field.
        let mut options: sys::GhosttyTerminalSelectLineOptions = unsafe { ffi::sized() };
        options.ref_ = *reference.as_raw();
        options.whitespace = if whitespace.is_empty() {
            ptr::null()
        } else {
            whitespace.as_ptr()
        };
        options.whitespace_len = whitespace.len();
        options.semantic_prompt_boundary = semantic_prompt_boundary;
        self.selection_result(|out| unsafe {
            sys::ghostty_terminal_select_line(self.as_raw(), &options, out)
        })
    }
    /// Selects all selectable content, including scrollback.
    pub fn select_all(&self) -> Result<Selection<'_>> {
        self.selection_result(|out| unsafe { sys::ghostty_terminal_select_all(self.as_raw(), out) })
    }
    /// Selects semantic command output containing the reference.
    pub fn select_output(&self, reference: GridRef<'_>) -> Result<Selection<'_>> {
        self.check_selection_ref(reference)?;
        self.selection_result(|out| unsafe {
            sys::ghostty_terminal_select_output(self.as_raw(), reference.as_raw(), out)
        })
    }
}

ffi::c_enum! {
    /// Granularity for a click sequence.
    pub enum SelectionGestureBehavior {
        /// Cell-granular dragging.
        Cell = sys::GHOSTTY_SELECTION_GESTURE_BEHAVIOR_CELL,
        /// Word selection and dragging.
        Word = sys::GHOSTTY_SELECTION_GESTURE_BEHAVIOR_WORD,
        /// Line selection and dragging.
        Line = sys::GHOSTTY_SELECTION_GESTURE_BEHAVIOR_LINE,
        /// Semantic command output.
        Output = sys::GHOSTTY_SELECTION_GESTURE_BEHAVIOR_OUTPUT,
    }
}
ffi::c_enum! {
    /// Requested autoscroll direction.
    pub enum SelectionGestureAutoscroll {
        /// No scrolling.
        None = sys::GHOSTTY_SELECTION_GESTURE_AUTOSCROLL_NONE,
        /// Scroll upward.
        Up = sys::GHOSTTY_SELECTION_GESTURE_AUTOSCROLL_UP,
        /// Scroll downward.
        Down = sys::GHOSTTY_SELECTION_GESTURE_AUTOSCROLL_DOWN,
    }
}
ffi::c_enum! {
    /// Fixed type of a reusable gesture event.
    pub enum SelectionGestureEventType {
        /// Pointer press.
        Press = sys::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_PRESS,
        /// Pointer release.
        Release = sys::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_RELEASE,
        /// Pointer drag.
        Drag = sys::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_DRAG,
        /// Timer-driven autoscroll.
        AutoscrollTick = sys::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_AUTOSCROLL_TICK,
        /// Pressure-driven word selection.
        DeepPress = sys::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_DEEP_PRESS,
    }
}

/// Display geometry for selection dragging.
#[derive(Debug, Clone, Copy)]
pub struct SelectionGestureGeometry {
    /// Rendered grid columns; must be nonzero.
    pub columns: u32,
    /// Cell width in pixels; must be nonzero.
    pub cell_width: u32,
    /// Left padding in pixels.
    pub padding_left: u32,
    /// Surface height in pixels; must be nonzero.
    pub screen_height: u32,
}

/// Reusable gesture input. Cell positions are resolved against the live terminal at dispatch.
pub struct SelectionGestureEvent {
    raw: NonNull<sys::GhosttySelectionGestureEventImpl>,
    point: Option<Point>,
    kind: SelectionGestureEventType,
}

impl SelectionGestureEvent {
    /// Creates an event of a fixed type.
    pub fn new(kind: SelectionGestureEventType) -> Result<Self> {
        let mut raw = ptr::null_mut();
        unsafe {
            Error::check(sys::ghostty_selection_gesture_event_new(
                ptr::null(),
                &mut raw,
                kind.to_raw(),
            ))?;
        }
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
            point: None,
            kind,
        })
    }
    fn set<T>(
        &mut self,
        option: sys::GhosttySelectionGestureEventOption,
        value: Option<&T>,
    ) -> Result<()> {
        let value = value.map_or(ptr::null(), |value| (value as *const T).cast());
        unsafe {
            Error::check(sys::ghostty_selection_gesture_event_set(
                self.raw.as_ptr(),
                option,
                value,
            ))
        }
    }
    /// Sets or clears the pointer cell. It is resolved only when the event is applied.
    pub fn set_point(&mut self, point: Option<Point>) {
        self.point = point;
    }
    /// Sets or clears the pointer position in surface pixels.
    pub fn set_position(&mut self, position: Option<(f64, f64)>) -> Result<()> {
        self.set(
            sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION,
            position
                .map(|(x, y)| sys::GhosttySurfacePosition { x, y })
                .as_ref(),
        )
    }
    /// Sets or clears the maximum repeat-click distance in pixels.
    pub fn set_repeat_distance(&mut self, distance: Option<f64>) -> Result<()> {
        self.set(
            sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_DISTANCE,
            distance.as_ref(),
        )
    }
    /// Sets or clears the monotonic event timestamp in nanoseconds.
    pub fn set_time_ns(&mut self, time: Option<u64>) -> Result<()> {
        self.set(
            sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_TIME_NS,
            time.as_ref(),
        )
    }
    /// Sets or clears the repeat-click interval in nanoseconds.
    pub fn set_repeat_interval_ns(&mut self, interval: Option<u64>) -> Result<()> {
        self.set(
            sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_INTERVAL_NS,
            interval.as_ref(),
        )
    }
    /// Sets or clears word boundaries, copied into event-owned storage.
    pub fn set_word_boundaries(&mut self, boundaries: Option<&[u32]>) -> Result<()> {
        self.set(
            sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_WORD_BOUNDARY_CODEPOINTS,
            boundaries
                .map(|values| sys::GhosttyCodepoints {
                    ptr: values.as_ptr(),
                    len: values.len(),
                })
                .as_ref(),
        )
    }
    /// Sets or clears behaviors for single, double, and triple clicks.
    pub fn set_behaviors(
        &mut self,
        behaviors: Option<[SelectionGestureBehavior; 3]>,
    ) -> Result<()> {
        self.set(
            sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_BEHAVIORS,
            behaviors
                .map(|b| sys::GhosttySelectionGestureBehaviors {
                    single_click: b[0].to_raw(),
                    double_click: b[1].to_raw(),
                    triple_click: b[2].to_raw(),
                })
                .as_ref(),
        )
    }
    /// Sets or clears rectangular dragging.
    pub fn set_rectangle(&mut self, rectangle: Option<bool>) -> Result<()> {
        self.set(
            sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_RECTANGLE,
            rectangle.as_ref(),
        )
    }
    /// Sets or clears required drag geometry.
    pub fn set_geometry(&mut self, geometry: Option<SelectionGestureGeometry>) -> Result<()> {
        self.set(
            sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_GEOMETRY,
            geometry
                .map(|g| sys::GhosttySelectionGestureGeometry {
                    columns: g.columns,
                    cell_width: g.cell_width,
                    padding_left: g.padding_left,
                    screen_height: g.screen_height,
                })
                .as_ref(),
        )
    }
    /// Sets or clears the viewport coordinate for an autoscroll tick.
    pub fn set_viewport(&mut self, viewport: Option<Coordinate>) -> Result<()> {
        self.set(
            sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_VIEWPORT,
            viewport.map(sys::GhosttyPointCoordinate::from).as_ref(),
        )
    }
}
impl Drop for SelectionGestureEvent {
    fn drop(&mut self) {
        unsafe { sys::ghostty_selection_gesture_event_free(self.raw.as_ptr()) };
    }
}

/// Gesture state exclusively bound to one terminal, including for reset and destruction.
pub struct SelectionGesture<'t> {
    raw: NonNull<sys::GhosttySelectionGestureImpl>,
    terminal: &'t mut Terminal,
}
impl<'t> SelectionGesture<'t> {
    /// Creates gesture state bound to a terminal for its entire lifetime.
    pub fn new(terminal: &'t mut Terminal) -> Result<Self> {
        let mut raw = ptr::null_mut();
        unsafe {
            Error::check(sys::ghostty_selection_gesture_new(ptr::null(), &mut raw))?;
        }
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
            terminal,
        })
    }
    /// Returns the terminal for rendering or formatting.
    pub fn terminal(&self) -> &Terminal {
        self.terminal
    }
    /// Returns the terminal for live input between events.
    pub fn terminal_mut(&mut self) -> &mut Terminal {
        self.terminal
    }
    /// Cancels the click sequence and releases tracked references.
    pub fn reset(&mut self) {
        unsafe { sys::ghostty_selection_gesture_reset(self.raw.as_ptr(), self.terminal.as_raw()) };
    }
    /// Applies an event, returning an optional untracked selection snapshot.
    pub fn event(&mut self, event: &mut SelectionGestureEvent) -> Result<Option<Selection<'_>>> {
        if matches!(
            event.kind,
            SelectionGestureEventType::Press
                | SelectionGestureEventType::Drag
                | SelectionGestureEventType::Release
        ) {
            let reference = event
                .point
                .map(|point| self.terminal.grid_ref(point))
                .transpose()?;
            event.set(
                sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF,
                reference.as_ref().map(GridRef::as_raw),
            )?;
        }
        let mut raw = sys::GhosttySelection::default();
        let code = unsafe {
            sys::ghostty_selection_gesture_event(
                self.raw.as_ptr(),
                self.terminal.as_raw(),
                event.raw.as_ptr(),
                &mut raw,
            )
        };
        if code == sys::GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        Error::check(code)?;
        // SAFETY: event output belongs to this terminal; borrowing self prevents later mutation.
        Ok(Some(unsafe { Selection::from_raw(self.terminal, &raw) }))
    }
    fn get<T: Default>(&self, data: sys::GhosttySelectionGestureData) -> Result<T> {
        let mut value = T::default();
        unsafe {
            Error::check(sys::ghostty_selection_gesture_get(
                self.raw.as_ptr(),
                self.terminal.as_raw(),
                data,
                (&raw mut value).cast(),
            ))?;
        }
        Ok(value)
    }
    /// Current click count, or zero when inactive.
    pub fn click_count(&self) -> Result<u8> {
        self.get(sys::GHOSTTY_SELECTION_GESTURE_DATA_CLICK_COUNT)
    }
    /// Whether the current or last gesture dragged.
    pub fn dragged(&self) -> Result<bool> {
        self.get(sys::GHOSTTY_SELECTION_GESTURE_DATA_DRAGGED)
    }
    /// Current autoscroll request.
    pub fn autoscroll(&self) -> Result<SelectionGestureAutoscroll> {
        self.get(sys::GHOSTTY_SELECTION_GESTURE_DATA_AUTOSCROLL)
            .map(SelectionGestureAutoscroll::from_lib)
    }
    /// Current click-sequence behavior.
    pub fn behavior(&self) -> Result<SelectionGestureBehavior> {
        self.get(sys::GHOSTTY_SELECTION_GESTURE_DATA_BEHAVIOR)
            .map(SelectionGestureBehavior::from_lib)
    }
    /// The current anchor, if still valid in this terminal.
    pub fn anchor(&self) -> Result<Option<GridRef<'_>>> {
        match self.get::<sys::GhosttyGridRef>(sys::GHOSTTY_SELECTION_GESTURE_DATA_ANCHOR) {
            // SAFETY: get validates this terminal's tracked anchor, and self remains borrowed.
            Ok(raw) => Ok(Some(unsafe { GridRef::from_raw(self.terminal, raw) })),
            Err(Error::NoValue) => Ok(None),
            Err(error) => Err(error),
        }
    }
}
impl Drop for SelectionGesture<'_> {
    fn drop(&mut self) {
        // SAFETY: the exclusive terminal borrow ensures the correct terminal is still alive.
        unsafe { sys::ghostty_selection_gesture_free(self.raw.as_ptr(), self.terminal.as_raw()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::Formatter;
    fn text(selection: &Selection<'_>) -> Vec<u8> {
        let mut formatter = Formatter::new(selection.terminal()).unwrap();
        formatter.set_selection(Some(selection)).unwrap();
        formatter.format_to_vec().unwrap()
    }
    #[test]
    fn word_line_all_and_adjustment() -> Result<()> {
        let mut terminal = Terminal::new(20, 4)?;
        terminal.vt_write(b"hello world\r\nsecond line");
        let reference = terminal.grid_ref(Point::active(7, 0))?;
        let word = terminal.select_word(reference, &[])?;
        assert_eq!(text(&word), b"world");
        assert!(word.contains(Point::active(8, 0))?);
        assert!(!word.contains(Point::active(0, 0))?);
        let reverse = word.ordered(SelectionOrder::Reverse)?;
        assert_eq!(reverse.order()?, SelectionOrder::Reverse);
        assert!(reverse.ordered(SelectionOrder::Forward)?.equal(&word)?);
        let mut expanded = word;
        expanded.adjust(SelectionAdjust::BeginningOfLine)?;
        assert_eq!(text(&expanded), b"hello w");
        assert_eq!(
            text(&terminal.select_line(reference, &[], false)?),
            b"hello world"
        );
        assert_eq!(text(&terminal.select_all()?), b"hello world\nsecond line");
        let start = terminal.grid_ref(Point::active(0, 0))?;
        let first = terminal.select_word_between(start, reference, &[])?;
        let last = terminal.select_word_between(reference, start, &[])?;
        assert_eq!(text(&first), b"hello");
        assert_eq!(text(&last), b"world");
        assert_eq!(
            text(&Selection::new(first.start(), last.end(), false)),
            b"hello world"
        );
        // Written whitespace is itself a word under Ghostty's boundary rules.
        let space = terminal.select_word_between(
            terminal.grid_ref(Point::active(5, 0))?,
            reference,
            &[],
        )?;
        assert!(space.contains(Point::active(5, 0))?);
        assert!(!space.contains(Point::active(6, 0))?);
        let mut formatter = Formatter::new(&terminal)?;
        formatter.set_selection(Some(&space))?;
        formatter.set_trim(false)?;
        assert_eq!(formatter.format_to_vec()?, b" ");
        Ok(())
    }
    #[test]
    fn semantic_line_and_output() -> Result<()> {
        let mut terminal = Terminal::new(80, 8)?;
        terminal.vt_write(b"\x1b]133;A\x07$ \x1b]133;B\x07git status\x1b]133;C\x07\r\nOn branch main\r\nnothing to commit, working tree clean");
        let input = terminal.grid_ref(Point::active(2, 0))?;
        assert_eq!(
            text(&terminal.select_line(input, &[], true)?),
            b"git status"
        );
        let output = terminal.select_output(terminal.grid_ref(Point::active(0, 1))?)?;
        assert_eq!(
            text(&output),
            b"On branch main\nnothing to commit, working tree clean"
        );
        let rectangle = Selection::new(
            terminal.grid_ref(Point::active(0, 1))?,
            terminal.grid_ref(Point::active(1, 2))?,
            true,
        );
        assert_eq!(text(&rectangle), b"On\nno");
        Ok(())
    }
    #[test]
    fn gesture_drag_release_and_deep_press() -> Result<()> {
        let mut terminal = Terminal::new(20, 4)?;
        terminal.vt_write(b"hello world\r\nsecond line");
        let mut gesture = SelectionGesture::new(&mut terminal)?;
        let mut press = SelectionGestureEvent::new(SelectionGestureEventType::Press)?;
        press.set_point(Some(Point::active(0, 0)));
        press.set_position(Some((2.0, 8.0)))?;
        assert!(gesture.event(&mut press)?.is_none());
        let mut drag = SelectionGestureEvent::new(SelectionGestureEventType::Drag)?;
        drag.set_point(Some(Point::active(4, 0)));
        drag.set_position(Some((46.0, 8.0)))?;
        drag.set_geometry(Some(SelectionGestureGeometry {
            columns: 20,
            cell_width: 10,
            padding_left: 0,
            screen_height: 40,
        }))?;
        assert_eq!(text(&gesture.event(&mut drag)?.unwrap()), b"hello");
        let mut release = SelectionGestureEvent::new(SelectionGestureEventType::Release)?;
        release.set_point(Some(Point::active(4, 0)));
        assert!(gesture.event(&mut release)?.is_none());
        assert!(gesture.dragged()?);
        gesture.reset();
        press.set_point(Some(Point::active(6, 0)));
        assert!(gesture.event(&mut press)?.is_none());
        let mut deep = SelectionGestureEvent::new(SelectionGestureEventType::DeepPress)?;
        assert_eq!(text(&gesture.event(&mut deep)?.unwrap()), b"world");
        Ok(())
    }
}
