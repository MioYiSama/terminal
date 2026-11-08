//! Geometry of the terminal's cell grid inside its element: padding, cell
//! positions, the scrollbar gutter and conversions to device pixels. Shared
//! by the view (pointer input) and the element (painting).

use gpui::{Bounds, Pixels, Point, point, px, size};

use crate::vt::{GesturePoint, Scrollbar, SurfaceSize};

pub const PADDING: Pixels = px(6.);
/// Width reserved for the scrollbar right of the grid's padding.
pub const SCROLLBAR_WIDTH: Pixels = px(10.);
const MIN_THUMB_HEIGHT: Pixels = px(24.);

/// Cell grid geometry as laid out in the last frame, in window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLayout {
    pub bounds: Bounds<Pixels>,
    pub cell_width: Pixels,
    pub line_height: Pixels,
    pub cols: u16,
    pub rows: u16,
    /// Device pixels per logical pixel. The terminal and the program see
    /// device pixels, like in Ghostty, so images are sized for the screen.
    pub scale: f32,
}

impl GridLayout {
    /// The largest grid of `cell_width` x `line_height` cells (at least one)
    /// that fits `bounds` inside the padding and beside the scrollbar.
    pub fn fit(
        bounds: Bounds<Pixels>,
        cell_width: Pixels,
        line_height: Pixels,
        scale: f32,
    ) -> Self {
        let inner = bounds.size - size(PADDING * 2. + SCROLLBAR_WIDTH, PADDING * 2.);
        let cols = (f32::from(inner.width) / f32::from(cell_width))
            .floor()
            .max(1.) as u16;
        let rows = (f32::from(inner.height) / f32::from(line_height))
            .floor()
            .max(1.) as u16;
        Self {
            bounds,
            cell_width,
            line_height,
            cols,
            rows,
            scale,
        }
    }

    /// A logical length in whole device pixels. Cell sizes are snapped to
    /// device pixels, so these are exact.
    pub fn device(&self, value: Pixels) -> u32 {
        (f32::from(value) * self.scale).round() as u32
    }

    /// A device-pixel length as a logical one.
    pub fn logical(&self, value: f32) -> Pixels {
        px(value / self.scale)
    }

    /// Top-left corner of the first cell.
    pub fn origin(&self) -> Point<Pixels> {
        self.bounds.origin + point(PADDING, PADDING)
    }

    /// Top-left corner of cell `(col, row)`; either may be negative or past
    /// the grid for things that start outside it.
    pub fn cell_origin(&self, col: f32, row: f32) -> Point<Pixels> {
        self.origin() + point(self.cell_width * col, self.line_height * row)
    }

    /// The cells' area, without padding.
    pub fn grid_bounds(&self) -> Bounds<Pixels> {
        Bounds::new(
            self.origin(),
            size(
                self.cell_width * self.cols as f32,
                self.line_height * self.rows as f32,
            ),
        )
    }

    /// Whether the cell grid differs from `other`'s in size, in cells or in
    /// device pixels per cell.
    pub fn grid_changed(&self, other: &Self) -> bool {
        (
            self.cols,
            self.rows,
            self.device(self.cell_width),
            self.device(self.line_height),
        ) != (
            other.cols,
            other.rows,
            other.device(other.cell_width),
            other.device(other.line_height),
        )
    }

    /// Cell under a window-space position, or `None` outside the grid.
    pub fn cell_containing(&self, position: Point<Pixels>) -> Option<(u16, u16)> {
        let local = position - self.origin();
        let col = (f32::from(local.x) / f32::from(self.cell_width)).floor();
        let row = (f32::from(local.y) / f32::from(self.line_height)).floor();
        (col >= 0. && row >= 0. && col < self.cols as f32 && row < self.rows as f32)
            .then_some((col as u16, row as u16))
    }

    /// Position relative to the element ("surface space") in device pixels.
    pub fn surface_position(&self, position: Point<Pixels>) -> (f32, f32) {
        let local = position - self.bounds.origin;
        (
            f32::from(local.x) * self.scale,
            f32::from(local.y) * self.scale,
        )
    }

    /// A window-space position for selection gestures: the cell under it,
    /// clamped to the grid, and its surface position.
    pub fn gesture_point(&self, position: Point<Pixels>) -> GesturePoint {
        let local = position - self.origin();
        let col = (f32::from(local.x) / f32::from(self.cell_width)).floor();
        let row = (f32::from(local.y) / f32::from(self.line_height)).floor();
        let (x, y) = self.surface_position(position);
        GesturePoint {
            col: col.clamp(0., self.cols.saturating_sub(1) as f32) as u16,
            row: row.clamp(0., self.rows.saturating_sub(1) as f32) as u16,
            x,
            y,
        }
    }

    /// The scrollbar's gutter along the element's right edge.
    pub fn scrollbar_track(&self) -> Bounds<Pixels> {
        Bounds::new(
            point(self.bounds.right() - SCROLLBAR_WIDTH, self.bounds.top()),
            size(SCROLLBAR_WIDTH, self.bounds.size.height),
        )
    }

    /// The thumb's slot in the track (full gutter width), or `None` when
    /// there is nothing to scroll.
    pub fn scrollbar_thumb(&self, bar: Scrollbar) -> Option<Bounds<Pixels>> {
        if !bar.scrollable() {
            return None;
        }
        let track = self.scrollbar_track();
        let height = track.size.height;
        let thumb = (height * (bar.len as f32 / bar.total as f32))
            .max(MIN_THUMB_HEIGHT)
            .min(height);
        let top = (height - thumb) * (bar.offset as f32 / bar.max_offset() as f32);
        Some(Bounds::new(
            point(track.left(), track.top() + top),
            size(track.size.width, thumb),
        ))
    }

    /// The scroll offset that puts the thumb's top edge at window-space `y`.
    pub fn scrollbar_offset_at(&self, bar: Scrollbar, y: Pixels) -> u64 {
        let Some(thumb) = self.scrollbar_thumb(bar) else {
            return 0;
        };
        let track = self.scrollbar_track();
        let range = f32::from(track.size.height - thumb.size.height);
        if range <= 0. {
            return 0;
        }
        let fraction = (f32::from(y - track.top()) / range).clamp(0., 1.);
        (fraction * bar.max_offset() as f32).round() as u64
    }

    /// The surface in device pixels, as the terminal's encoders see it.
    pub fn surface_size(&self) -> SurfaceSize {
        SurfaceSize {
            width: self.device(self.bounds.size.width - SCROLLBAR_WIDTH),
            height: self.device(self.bounds.size.height),
            cell_width: self.device(self.cell_width),
            cell_height: self.device(self.line_height),
            padding: self.device(PADDING),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> GridLayout {
        GridLayout {
            bounds: Bounds::new(point(px(0.), px(40.)), size(px(800.), px(500.))),
            cell_width: px(8.),
            line_height: px(16.),
            cols: 97,
            rows: 30,
            scale: 2.,
        }
    }

    fn bar(offset: u64) -> Scrollbar {
        Scrollbar {
            total: 1000,
            offset,
            len: 30,
        }
    }

    #[test]
    fn thumb_spans_track_ends() {
        let layout = layout();
        let track = layout.scrollbar_track();
        let top = layout.scrollbar_thumb(bar(0)).unwrap();
        let bottom = layout.scrollbar_thumb(bar(970)).unwrap();
        assert_eq!(top.top(), track.top());
        assert_eq!(bottom.bottom(), track.bottom());
        // 30 of 1000 rows would be 15px: clamped to the minimum.
        assert_eq!(top.size.height, MIN_THUMB_HEIGHT);
    }

    #[test]
    fn dragging_thumb_round_trips_offset() {
        let layout = layout();
        for offset in [0, 1, 485, 969, 970] {
            let thumb = layout.scrollbar_thumb(bar(offset)).unwrap();
            assert_eq!(layout.scrollbar_offset_at(bar(offset), thumb.top()), offset);
        }
        // Dragging past either end clamps.
        assert_eq!(layout.scrollbar_offset_at(bar(500), px(-100.)), 0);
        assert_eq!(layout.scrollbar_offset_at(bar(500), px(10_000.)), 970);
    }

    #[test]
    fn no_thumb_without_scrollback() {
        let screen = Scrollbar {
            total: 30,
            offset: 0,
            len: 30,
        };
        assert!(layout().scrollbar_thumb(screen).is_none());
    }
}
