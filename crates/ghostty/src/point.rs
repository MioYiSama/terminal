//! Grid coordinates in the terminal's coordinate spaces.

use ghostty_sys as sys;

use crate::ffi::c_enum;

c_enum! {
    /// The coordinate space of a [`Point`].
    pub enum PointTag {
        /// The active area: the bottom `rows` rows of the screen, where the
        /// cursor can move.
        Active = sys::GHOSTTY_POINT_TAG_ACTIVE,
        /// The visible viewport, which may be scrolled into history.
        Viewport = sys::GHOSTTY_POINT_TAG_VIEWPORT,
        /// The whole screen, scrollback included; row 0 is the oldest row.
        Screen = sys::GHOSTTY_POINT_TAG_SCREEN,
        /// Scrollback history only.
        History = sys::GHOSTTY_POINT_TAG_HISTORY,
    }
}

/// A column/row position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Coordinate {
    pub x: u16,
    pub y: u32,
}

impl From<sys::GhosttyPointCoordinate> for Coordinate {
    fn from(c: sys::GhosttyPointCoordinate) -> Self {
        Self { x: c.x, y: c.y }
    }
}

impl From<Coordinate> for sys::GhosttyPointCoordinate {
    fn from(c: Coordinate) -> Self {
        Self { x: c.x, y: c.y }
    }
}

/// A position in one of the terminal's coordinate spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point {
    pub tag: PointTag,
    pub coordinate: Coordinate,
}

impl Point {
    pub const fn new(tag: PointTag, x: u16, y: u32) -> Self {
        Self {
            tag,
            coordinate: Coordinate { x, y },
        }
    }

    pub const fn active(x: u16, y: u32) -> Self {
        Self::new(PointTag::Active, x, y)
    }

    pub const fn viewport(x: u16, y: u32) -> Self {
        Self::new(PointTag::Viewport, x, y)
    }

    pub const fn screen(x: u16, y: u32) -> Self {
        Self::new(PointTag::Screen, x, y)
    }

    pub const fn history(x: u16, y: u32) -> Self {
        Self::new(PointTag::History, x, y)
    }

    pub(crate) fn to_raw(self) -> sys::GhosttyPoint {
        let mut value = sys::GhosttyPointValue::default();
        value.coordinate = self.coordinate.into();
        sys::GhosttyPoint {
            tag: self.tag.to_raw(),
            value,
        }
    }
}
