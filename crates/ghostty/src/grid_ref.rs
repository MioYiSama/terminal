//! Untracked cell snapshots and terminal-owned tracked grid references.

use ghostty_sys as sys;

use crate::ffi::encode_vec;
use crate::point::{Coordinate, Point, PointTag};
use crate::screen::{Cell, Row};
use crate::style::Style;
use crate::terminal::Terminal;
use crate::{Error, Result};

/// A reference to one cell of a [`Terminal`].
///
/// The reference is a snapshot that libghostty-vt only keeps valid until the
/// next change to the terminal, so it borrows the terminal: every mutating
/// [`Terminal`] method takes `&mut self` and cannot be called while a
/// `GridRef` is alive.
#[derive(Clone, Copy)]
pub struct GridRef<'t> {
    raw: sys::GhosttyGridRef,
    terminal: &'t Terminal,
}

impl core::fmt::Debug for GridRef<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GridRef")
            .field("node", &self.raw.node)
            .field("x", &self.raw.x)
            .field("y", &self.raw.y)
            .finish_non_exhaustive()
    }
}

impl<'t> GridRef<'t> {
    /// Wraps a raw reference produced by the library.
    ///
    /// # Safety
    ///
    /// `raw` must be zeroed or have been produced by libghostty-vt from
    /// `terminal`, with no change to that terminal since.
    pub(crate) unsafe fn from_raw(terminal: &'t Terminal, raw: sys::GhosttyGridRef) -> Self {
        Self { raw, terminal }
    }

    pub fn as_raw(&self) -> &sys::GhosttyGridRef {
        &self.raw
    }

    /// The terminal this reference points into.
    pub fn terminal(&self) -> &'t Terminal {
        self.terminal
    }

    /// The position of the cell in the given coordinate space.
    ///
    /// Returns [`Error::NoValue`] if the cell is outside that space, e.g. a
    /// scrollback cell in [`PointTag::Active`] coordinates.
    pub fn point(&self, tag: PointTag) -> Result<Coordinate> {
        let mut out = sys::GhosttyPointCoordinate::default();
        Error::check(unsafe {
            sys::ghostty_terminal_point_from_grid_ref(
                self.terminal.as_raw(),
                &self.raw,
                tag.to_raw(),
                &mut out,
            )
        })?;
        Ok(out.into())
    }

    /// The cell at this position.
    pub fn cell(&self) -> Result<Cell> {
        let mut out = 0;
        Error::check(unsafe { sys::ghostty_grid_ref_cell(&self.raw, &mut out) })?;
        Ok(Cell::from_raw(out))
    }

    /// The row containing this cell.
    pub fn row(&self) -> Result<Row> {
        let mut out = 0;
        Error::check(unsafe { sys::ghostty_grid_ref_row(&self.raw, &mut out) })?;
        Ok(Row::from_raw(out))
    }

    /// The cell's style.
    pub fn style(&self) -> Result<Style> {
        let mut out = sys::GhosttyStyle::default();
        Error::check(unsafe { sys::ghostty_grid_ref_style(&self.raw, &mut out) })?;
        Ok(Style::from_raw(&out))
    }

    /// The full grapheme cluster of the cell as codepoints: the primary
    /// codepoint followed by any combining codepoints. Empty for a cell
    /// with no text.
    pub fn graphemes(&self) -> Result<Vec<u32>> {
        encode_vec(|buf, len, written| unsafe {
            sys::ghostty_grid_ref_graphemes(&self.raw, buf, len, written)
        })
    }

    /// The grapheme cluster of the cell as a string. Invalid codepoints are
    /// replaced with U+FFFD.
    pub fn text(&self) -> Result<String> {
        Ok(self
            .graphemes()?
            .into_iter()
            .map(|cp| char::from_u32(cp).unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect())
    }

    /// The cell's hyperlink URI (OSC 8), or `None` if it has none.
    pub fn hyperlink_uri(&self) -> Result<Option<Vec<u8>>> {
        let uri = encode_vec(|buf, len, written| unsafe {
            sys::ghostty_grid_ref_hyperlink_uri(&self.raw, buf, len, written)
        })?;
        Ok((!uri.is_empty()).then_some(uri))
    }
}

/// An identifier for a terminal-owned tracked cell reference.
///
/// It can be retained across terminal mutations. IDs are never reused, and
/// another terminal rejects them. Use [`Terminal::untrack`] to release one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrackedGridRefId {
    terminal: u64,
    index: usize,
}

impl TrackedGridRefId {
    pub(crate) fn new(terminal: u64, index: usize) -> Self {
        Self { terminal, index }
    }

    pub(crate) fn index(self, terminal: u64) -> Option<usize> {
        (self.terminal == terminal).then_some(self.index)
    }
}

/// An owned tracked grid-reference handle, managed by its terminal.
///
/// The C handle may outlive a terminal, but freeing or querying it while that
/// terminal is accessed on another thread is not safe. This wrapper therefore
/// keeps ownership inside [`Terminal`] and exposes [`TrackedGridRefId`] instead
/// of handing independently movable handles to callers. All tracked operations
/// remain serialized by the terminal's borrows.
pub struct TrackedGridRef {
    raw: core::ptr::NonNull<sys::GhosttyTrackedGridRefImpl>,
}

impl TrackedGridRef {
    /// Takes ownership of a fresh tracked handle.
    ///
    /// # Safety
    /// The handle must be live, uniquely owned, and managed by its terminal.
    pub(crate) unsafe fn from_raw(raw: core::ptr::NonNull<sys::GhosttyTrackedGridRefImpl>) -> Self {
        Self { raw }
    }

    pub(crate) fn as_raw(&self) -> sys::GhosttyTrackedGridRef {
        self.raw.as_ptr()
    }
}

impl Drop for TrackedGridRef {
    fn drop(&mut self) {
        // SAFETY: this handle is uniquely owned and its terminal serializes
        // every access, including dropping tracked handles before itself.
        unsafe { sys::ghostty_tracked_grid_ref_free(self.as_raw()) };
    }
}

impl Terminal {
    /// Resolves a point to a reference to its cell.
    pub fn grid_ref(&self, point: Point) -> Result<GridRef<'_>> {
        let mut out = sys::GhosttyGridRef::default();
        Error::check(unsafe {
            sys::ghostty_terminal_grid_ref(self.as_raw(), point.to_raw(), &mut out)
        })?;
        // SAFETY: produced from `self`, borrowed for the returned lifetime.
        Ok(unsafe { GridRef::from_raw(self, out) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_ref_tracked_retargets_and_rejects_foreign_ids() {
        let mut first = Terminal::new(12, 3).unwrap();
        let mut second = Terminal::new(12, 3).unwrap();
        first.vt_write(b"hello");
        let id = first.grid_ref_track(Point::active(0, 0)).unwrap();
        assert_eq!(first.tracked_snapshot(id).unwrap().text().unwrap(), "h");
        first.retrack(id, Point::active(4, 0)).unwrap();
        assert_eq!(first.tracked_snapshot(id).unwrap().text().unwrap(), "o");
        assert!(!second.tracked_has_value(id));
        assert_eq!(
            second.tracked_point(id, PointTag::Active),
            Err(Error::InvalidValue)
        );
        assert_eq!(second.untrack(id), Err(Error::InvalidValue));
        first.untrack(id).unwrap();
        let replacement = first.track(Point::active(0, 0)).unwrap();
        assert_ne!(id, replacement);
        assert!(!first.tracked_has_value(id));
    }

    #[test]
    fn grid_ref_tracked_follows_reflow_and_keeps_original_screen() {
        let mut terminal = Terminal::new(12, 3).unwrap();
        terminal.vt_write(b"hello world");
        let id = terminal.track(Point::active(6, 0)).unwrap();
        terminal.resize(6, 3, 0, 0).unwrap();
        assert_eq!(terminal.tracked_snapshot(id).unwrap().text().unwrap(), "w");
        // Switching screens preserves the cursor position; explicitly home
        // before placing the alternate-screen text that we will track.
        terminal.vt_write(b"\x1b[?1049h\x1b[Hother");
        assert_eq!(
            terminal.active_screen().unwrap(),
            crate::terminal::ActiveScreen::Alternate
        );
        assert_eq!(terminal.tracked_snapshot(id).unwrap().text().unwrap(), "w");
        terminal.retrack(id, Point::active(0, 0)).unwrap();
        assert_eq!(terminal.tracked_snapshot(id).unwrap().text().unwrap(), "o");
    }
}
