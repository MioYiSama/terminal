//! Safe Rust bindings to [libghostty-vt](https://github.com/ghostty-org/ghostty),
//! Ghostty's terminal emulation library.
//!
//! ```
//! use ghostty::{Terminal, formatter::Formatter};
//!
//! let mut terminal = Terminal::new(80, 24).unwrap();
//! terminal.vt_write(b"Hello, \x1b[1;32mworld\x1b[0m!\r\n");
//! let text = Formatter::new(&terminal).unwrap().format_to_vec().unwrap();
//! assert_eq!(String::from_utf8(text).unwrap().trim_end(), "Hello, world!");
//! ```
//!
//! The raw C API is available as [`sys`].

pub use ghostty_sys as sys;

pub(crate) mod ffi;

mod error;

pub mod build_info;
pub mod color;
pub mod color_scheme;
pub mod device;
pub mod focus;
pub mod formatter;
pub mod grid_ref;
pub mod key;
pub mod kitty_graphics;
pub mod modes;
pub mod mouse;
pub mod osc;
pub mod paste;
pub mod point;
pub mod render;
pub mod screen;
pub mod search;
pub mod selection;
pub mod sgr;
pub mod size_report;
pub mod snapshot;
pub mod style;
pub mod system;
pub mod terminal;
pub mod unicode;

pub use color::Rgb;
pub use error::{Error, Result};
pub use formatter::{Formatter, FormatterFormat};
pub use grid_ref::GridRef;
pub use key::{KeyEncoder, KeyEvent};
pub use modes::Mode;
pub use mouse::{MouseEncoder, MouseEvent};
pub use point::{Point, PointTag};
pub use render::RenderState;
pub use selection::Selection;
pub use style::Style;
pub use terminal::Terminal;
