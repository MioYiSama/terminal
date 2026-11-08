//! Kitty graphics image storage and reusable placement inspection.
use crate::{Error, Result, Terminal, ffi, selection::Selection};
use ghostty_sys as sys;
use std::ptr::NonNull;

ffi::c_enum! {
    /// Drawing layer of an image placement.
    pub enum ImageLayer {
        /// Below cell backgrounds.
        BelowBackground = sys::GHOSTTY_IMAGE_LAYER_BELOW_BACKGROUND,
        /// Above backgrounds and below text.
        BelowText = sys::GHOSTTY_IMAGE_LAYER_BELOW_TEXT,
        /// Above text.
        AboveText = sys::GHOSTTY_IMAGE_LAYER_ABOVE_TEXT,
    }
}
bitflags::bitflags! {
    /// Layers visited by a storage placement iterator; empty disables filtering.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LayerMask: u32 {
        /// Below cell backgrounds.
        const BELOW_BACKGROUND = 1 << sys::GHOSTTY_IMAGE_LAYER_BELOW_BACKGROUND;
        /// Below text.
        const BELOW_TEXT = 1 << sys::GHOSTTY_IMAGE_LAYER_BELOW_TEXT;
        /// Above text.
        const ABOVE_TEXT = 1 << sys::GHOSTTY_IMAGE_LAYER_ABOVE_TEXT;
    }
}
ffi::c_enum! {
    /// Decoded image pixel format.
    pub enum ImageFormat {
        /// Three bytes per pixel.
        Rgb = sys::GHOSTTY_KITTY_IMAGE_FORMAT_RGB,
        /// Four bytes per pixel.
        Rgba = sys::GHOSTTY_KITTY_IMAGE_FORMAT_RGBA,
        /// Protocol-only PNG format; never returned for stored images.
        Png = sys::GHOSTTY_KITTY_IMAGE_FORMAT_PNG,
        /// Gray and alpha channels.
        GrayAlpha = sys::GHOSTTY_KITTY_IMAGE_FORMAT_GRAY_ALPHA,
        /// Gray channel.
        Gray = sys::GHOSTTY_KITTY_IMAGE_FORMAT_GRAY,
    }
}
ffi::c_enum! {
    /// Image compression (stored pixels are always uncompressed).
    pub enum ImageCompression {
        /// Uncompressed pixels.
        None = sys::GHOSTTY_KITTY_IMAGE_COMPRESSION_NONE,
        /// Protocol-only compressed format.
        ZlibDeflate = sys::GHOSTTY_KITTY_IMAGE_COMPRESSION_ZLIB_DEFLATE,
    }
}

/// Image storage borrowed from a terminal's active screen.
pub struct KittyGraphics<'t> {
    raw: NonNull<sys::GhosttyKittyGraphicsImpl>,
    terminal: &'t Terminal,
}
impl Terminal {
    /// Borrow the active screen's Kitty graphics storage.
    pub fn kitty_graphics(&self) -> Result<KittyGraphics<'_>> {
        let mut raw = std::ptr::null_mut();
        Error::check(unsafe {
            sys::ghostty_terminal_get(
                self.as_raw(),
                sys::GHOSTTY_TERMINAL_DATA_KITTY_GRAPHICS,
                (&mut raw as *mut sys::GhosttyKittyGraphics).cast(),
            )
        })?;
        Ok(KittyGraphics {
            raw: NonNull::new(raw).ok_or(Error::NoValue)?,
            terminal: self,
        })
    }
    /// Advance image animations; use one monotonically increasing clock per terminal.
    /// Returns the next frame deadline, or `None` when no animation is playing.
    pub fn kitty_graphics_animation_tick(&mut self, now_ms: u64) -> Result<Option<u64>> {
        let graphics = self.kitty_graphics()?;
        let mut next = 0;
        let code = unsafe {
            sys::ghostty_kitty_graphics_animation_tick(graphics.raw.as_ptr(), now_ms, &mut next)
        };
        if code == sys::GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        Error::check(code)?;
        Ok(Some(next))
    }
}
impl<'t> KittyGraphics<'t> {
    /// Storage generation; unchanged means the same images and placements.
    pub fn generation(&self) -> Result<u64> {
        let mut out = 0;
        Error::check(unsafe {
            sys::ghostty_kitty_graphics_get(
                self.raw.as_ptr(),
                sys::GHOSTTY_KITTY_GRAPHICS_DATA_GENERATION,
                (&mut out as *mut u64).cast(),
            )
        })?;
        Ok(out)
    }
    /// Look up an image without copying its pixels.
    pub fn image(&self, id: u32) -> Option<KittyImage<'t>> {
        let raw = unsafe { sys::ghostty_kitty_graphics_image(self.raw.as_ptr(), id) };
        NonNull::new(raw.cast_mut()).map(|raw| KittyImage {
            raw,
            _terminal: self.terminal,
        })
    }
    /// Populate a reusable iterator, borrowing this terminal until the view is dropped.
    pub fn placements<'a>(
        &self,
        iterator: &'a mut PlacementIterator,
    ) -> Result<Placements<'a, 't>> {
        let mut raw = iterator.raw.as_ptr();
        Error::check(unsafe {
            sys::ghostty_kitty_graphics_get(
                self.raw.as_ptr(),
                sys::GHOSTTY_KITTY_GRAPHICS_DATA_PLACEMENT_ITERATOR,
                (&mut raw as *mut sys::GhosttyKittyGraphicsPlacementIterator).cast(),
            )
        })?;
        Ok(Placements {
            iterator,
            terminal: self.terminal,
        })
    }
}
/// A borrowed image; terminal mutation is prohibited while it is in use.
pub struct KittyImage<'t> {
    raw: NonNull<sys::GhosttyKittyGraphicsImageImpl>,
    _terminal: &'t Terminal,
}
macro_rules! image_getters {
    ($(#[doc = $doc:literal] $name:ident: $ty:ty = $key:ident;)*) => {$(
        #[doc = $doc]
        pub fn $name(&self) -> Result<$ty> { unsafe { self.get(sys::$key) } }
    )*};
}
impl<'t> KittyImage<'t> {
    // SAFETY: callers supply exactly the output type documented for the key.
    unsafe fn get<T: Default>(&self, key: sys::GhosttyKittyGraphicsImageData) -> Result<T> {
        let mut out = T::default();
        Error::check(unsafe {
            sys::ghostty_kitty_graphics_image_get(
                self.raw.as_ptr(),
                key,
                (&mut out as *mut T).cast(),
            )
        })?;
        Ok(out)
    }
    image_getters! {
        /// Image ID.
        id: u32 = GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_ID;
        /// Image number.
        number: u32 = GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_NUMBER;
        /// Width in pixels.
        width: u32 = GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_WIDTH;
        /// Height in pixels.
        height: u32 = GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_HEIGHT;
        /// Expected decoded pixel byte count, even before transmission completes.
        expected_len: usize = GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_EXPECTED_LEN;
        /// Generation of the pixel contents.
        generation: u64 = GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_GENERATION;
    }
    /// Decoded pixel format.
    pub fn format(&self) -> Result<ImageFormat> {
        Ok(ImageFormat::from_lib(unsafe {
            self.get(sys::GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_FORMAT)?
        }))
    }
    /// Compression of stored pixels.
    pub fn compression(&self) -> Result<ImageCompression> {
        Ok(ImageCompression::from_lib(unsafe {
            self.get(sys::GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_COMPRESSION)?
        }))
    }
    /// Borrow the decoded pixels, or `None` while transmission is incomplete.
    pub fn pixels(&self) -> Result<Option<&'t [u8]>> {
        let value: sys::GhosttyString =
            match unsafe { self.get(sys::GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_PIXELS) } {
                Ok(value) => value,
                Err(Error::NoValue) => return Ok(None),
                Err(error) => return Err(error),
            };
        // SAFETY: storage is borrowed from the terminal for 't, preventing invalidating mutations.
        Ok(Some(unsafe { ffi::string_bytes(value) }))
    }
}
/// Reusable owning allocation for inspecting storage placements.
pub struct PlacementIterator {
    raw: NonNull<sys::GhosttyKittyGraphicsPlacementIteratorImpl>,
}
impl PlacementIterator {
    /// Allocate an unpopulated placement iterator.
    pub fn new() -> Result<Self> {
        let mut raw = std::ptr::null_mut();
        Error::check(unsafe {
            sys::ghostty_kitty_graphics_placement_iterator_new(std::ptr::null(), &mut raw)
        })?;
        Ok(Self {
            raw: NonNull::new(raw).ok_or(Error::OutOfMemory)?,
        })
    }
    /// Select drawing layers; an empty mask restores visiting all placements.
    pub fn set_layers(&mut self, layers: LayerMask) -> Result<()> {
        let value = layers.bits();
        Error::check(unsafe {
            sys::ghostty_kitty_graphics_placement_iterator_set(
                self.raw.as_ptr(),
                sys::GHOSTTY_KITTY_GRAPHICS_PLACEMENT_ITERATOR_OPT_LAYER,
                (&value as *const u32).cast(),
            )
        })
    }
}
impl Drop for PlacementIterator {
    fn drop(&mut self) {
        unsafe { sys::ghostty_kitty_graphics_placement_iterator_free(self.raw.as_ptr()) }
    }
}
/// Lending view of populated placement data.
pub struct Placements<'a, 't> {
    iterator: &'a mut PlacementIterator,
    terminal: &'t Terminal,
}
impl<'t> Placements<'_, 't> {
    /// Advance to a placement, borrowing the iterator until that placement is released.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Placement<'_, 't>> {
        unsafe { sys::ghostty_kitty_graphics_placement_next(self.iterator.raw.as_ptr()) }.then_some(
            Placement {
                raw: self.iterator.raw,
                terminal: self.terminal,
                _iterator: std::marker::PhantomData,
            },
        )
    }
}
/// The current storage placement and its terminal-relative geometry.
pub struct Placement<'a, 't> {
    raw: NonNull<sys::GhosttyKittyGraphicsPlacementIteratorImpl>,
    terminal: &'t Terminal,
    _iterator: std::marker::PhantomData<&'a mut PlacementIterator>,
}
macro_rules! placement_getters {
    ($(#[doc = $doc:literal] $name:ident: $ty:ty = $key:ident;)*) => {$(
        #[doc = $doc]
        pub fn $name(&self) -> Result<$ty> { unsafe { self.get(sys::$key) } }
    )*};
}
/// Resolved pixel rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRect {
    /// Left edge in image pixels.
    pub x: u32,
    /// Top edge in image pixels.
    pub y: u32,
    /// Width in image pixels.
    pub width: u32,
    /// Height in image pixels.
    pub height: u32,
}
/// Resolved storage placement geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacementRenderInfo {
    /// Drawn pixel size (width, height).
    pub pixel_size: (u32, u32),
    /// Occupied cells (columns, rows).
    pub grid_size: (u32, u32),
    /// Top-left cell if any part is visible.
    pub viewport_pos: Option<(i32, i32)>,
    /// Source image rectangle.
    pub source_rect: SourceRect,
}
impl<'t> Placement<'_, 't> {
    // SAFETY: each caller uses the output type specified by the header.
    unsafe fn get<T: Default>(&self, key: sys::GhosttyKittyGraphicsPlacementData) -> Result<T> {
        let mut out = T::default();
        Error::check(unsafe {
            sys::ghostty_kitty_graphics_placement_get(
                self.raw.as_ptr(),
                key,
                (&mut out as *mut T).cast(),
            )
        })?;
        Ok(out)
    }
    placement_getters! {
        /// Image ID.
        image_id: u32 = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_IMAGE_ID;
        /// Placement ID.
        placement_id: u32 = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_PLACEMENT_ID;
        /// Whether Unicode placeholders position this placement.
        is_virtual: bool = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_IS_VIRTUAL;
        /// Horizontal offset within the first cell.
        x_offset: u32 = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_X_OFFSET;
        /// Vertical offset within the first cell.
        y_offset: u32 = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_Y_OFFSET;
        /// Requested source x.
        source_x: u32 = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_SOURCE_X;
        /// Requested source y.
        source_y: u32 = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_SOURCE_Y;
        /// Requested source width (zero means full image).
        source_width: u32 = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_SOURCE_WIDTH;
        /// Requested source height (zero means full image).
        source_height: u32 = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_SOURCE_HEIGHT;
        /// Requested columns.
        columns: u32 = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_COLUMNS;
        /// Requested rows.
        rows: u32 = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_ROWS;
        /// Z-index.
        z: i32 = GHOSTTY_KITTY_GRAPHICS_PLACEMENT_DATA_Z;
    }
    fn image(&self) -> Result<KittyImage<'t>> {
        self.terminal
            .kitty_graphics()?
            .image(self.image_id()?)
            .ok_or(Error::NoValue)
    }
    /// Bounding rectangle, unavailable for virtual placements.
    pub fn rect(&self) -> Result<Option<Selection<'t>>> {
        let image = self.image()?;
        let mut out = sys::GhosttySelection::default();
        let code = unsafe {
            sys::ghostty_kitty_graphics_placement_rect(
                self.raw.as_ptr(),
                image.raw.as_ptr(),
                self.terminal.as_raw(),
                &mut out,
            )
        };
        if code == sys::GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        Error::check(code)?;
        // SAFETY: the selection belongs to the terminal borrowed for 't.
        Ok(Some(unsafe { Selection::from_raw(self.terminal, &out) }))
    }
    /// Final drawn pixel width and height.
    pub fn pixel_size(&self) -> Result<(u32, u32)> {
        let image = self.image()?;
        let (mut x, mut y) = (0, 0);
        Error::check(unsafe {
            sys::ghostty_kitty_graphics_placement_pixel_size(
                self.raw.as_ptr(),
                image.raw.as_ptr(),
                self.terminal.as_raw(),
                &mut x,
                &mut y,
            )
        })?;
        Ok((x, y))
    }
    /// Occupied columns and rows.
    pub fn grid_size(&self) -> Result<(u32, u32)> {
        let image = self.image()?;
        let (mut x, mut y) = (0, 0);
        Error::check(unsafe {
            sys::ghostty_kitty_graphics_placement_grid_size(
                self.raw.as_ptr(),
                image.raw.as_ptr(),
                self.terminal.as_raw(),
                &mut x,
                &mut y,
            )
        })?;
        Ok((x, y))
    }
    /// Top-left cell when any part is in the viewport.
    pub fn viewport_pos(&self) -> Result<Option<(i32, i32)>> {
        let image = self.image()?;
        let (mut x, mut y) = (0, 0);
        let code = unsafe {
            sys::ghostty_kitty_graphics_placement_viewport_pos(
                self.raw.as_ptr(),
                image.raw.as_ptr(),
                self.terminal.as_raw(),
                &mut x,
                &mut y,
            )
        };
        if code == sys::GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        Error::check(code)?;
        Ok(Some((x, y)))
    }
    /// Clamped source rectangle, with zero dimensions resolved to the full image.
    pub fn source_rect(&self) -> Result<SourceRect> {
        let image = self.image()?;
        let (mut x, mut y, mut width, mut height) = (0, 0, 0, 0);
        Error::check(unsafe {
            sys::ghostty_kitty_graphics_placement_source_rect(
                self.raw.as_ptr(),
                image.raw.as_ptr(),
                &mut x,
                &mut y,
                &mut width,
                &mut height,
            )
        })?;
        Ok(SourceRect {
            x,
            y,
            width,
            height,
        })
    }
    /// Read all resolved geometry together.
    pub fn render_info(&self) -> Result<PlacementRenderInfo> {
        let image = self.image()?;
        let mut out = unsafe { ffi::sized::<sys::GhosttyKittyGraphicsPlacementRenderInfo>() };
        Error::check(unsafe {
            sys::ghostty_kitty_graphics_placement_render_info(
                self.raw.as_ptr(),
                image.raw.as_ptr(),
                self.terminal.as_raw(),
                &mut out,
            )
        })?;
        Ok(PlacementRenderInfo {
            pixel_size: (out.pixel_width, out.pixel_height),
            grid_size: (out.grid_cols, out.grid_rows),
            viewport_pos: if out.viewport_visible {
                Some((out.viewport_col, out.viewport_row))
            } else {
                None
            },
            source_rect: SourceRect {
                x: out.source_x,
                y: out.source_y,
                width: out.source_width,
                height: out.source_height,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_rgba_image_storage_placement_and_render_state() -> Result<()> {
        let mut terminal = Terminal::new(20, 5)?;
        terminal.resize(20, 5, 8, 16)?;
        terminal.set_kitty_image_storage_limit(Some(64 * 1024 * 1024))?;
        // A single opaque red pixel, transmitted and placed in one action.
        terminal.vt_write(b"\x1b_Ga=T,f=32,s=1,v=1,i=7,p=9,c=2,r=1,q=2;/wAA/w==\x1b\\");
        let graphics = terminal.kitty_graphics()?;
        assert_ne!(graphics.generation()?, 0);
        assert!(graphics.image(999).is_none());
        let image = graphics.image(7).expect("transmitted image");
        assert_eq!(image.pixels()?, Some(&[255, 0, 0, 255][..]));
        assert_eq!(image.format()?, ImageFormat::Rgba);
        assert_eq!(image.expected_len()?, 4);
        let generation = image.generation()?;
        let mut iterator = PlacementIterator::new()?;
        let mut placements = graphics.placements(&mut iterator)?;
        let placement = placements.next().expect("placement");
        assert_eq!(placement.image_id()?, 7);
        assert_eq!(placement.placement_id()?, 9);
        assert_eq!(placement.grid_size()?, (2, 1));
        assert_eq!(placement.viewport_pos()?, Some((0, 0)));
        assert!(placement.rect()?.unwrap().is_rectangle());
        let info = placement.render_info()?;
        assert_eq!(info.source_rect, placement.source_rect()?);
        assert_eq!(info.pixel_size, placement.pixel_size()?);
        assert_eq!(info.grid_size, (2, 1));
        assert!(placements.next().is_none());
        iterator.set_layers(LayerMask::BELOW_TEXT)?;
        assert!(graphics.placements(&mut iterator)?.next().is_none());
        iterator.set_layers(LayerMask::ABOVE_TEXT)?;
        assert!(graphics.placements(&mut iterator)?.next().is_some());
        let mut state = crate::render::RenderState::new()?;
        state.update(&terminal)?;
        let mut images = crate::render::ImageIterator::new()?;
        let mut view = state.images(&mut images)?;
        let entry = view.next().expect("render placement");
        assert_eq!(entry.image_id()?, 7);
        assert_eq!(entry.image_generation()?, generation);
        assert_eq!(entry.kitty_placement_id()?, Some(9));
        assert_eq!(entry.grid_cols()?, 2);
        assert_eq!(entry.grid_rows()?, 1);
        assert!(view.next().is_none());
        assert_eq!(terminal.kitty_graphics_animation_tick(0)?, None);
        Ok(())
    }
    #[test]
    fn animation_tick_advances_pixels_and_generation() -> Result<()> {
        let mut terminal = Terminal::new(20, 5)?;
        terminal.resize(20, 5, 8, 16)?;
        terminal.set_kitty_image_storage_limit(Some(64 * 1024 * 1024))?;
        terminal.vt_write(
            concat!(
                "\x1b_Ga=T,f=32,i=2,s=1,v=1,q=2;/wAA/w==\x1b\\",
                "\x1b_Ga=f,f=32,i=2,s=1,v=1,z=40,q=2;AAD//w==\x1b\\",
                "\x1b_Ga=a,i=2,r=1,z=30,q=2\x1b\\",
                "\x1b_Ga=a,i=2,s=3,q=2\x1b\\",
            )
            .as_bytes(),
        );
        let deadline = terminal
            .kitty_graphics_animation_tick(0)?
            .expect("playing animation");
        let image = terminal.kitty_graphics()?.image(2).unwrap();
        assert_eq!(image.pixels()?, Some(&[255, 0, 0, 255][..]));
        let generation = image.generation()?;
        assert!(deadline > 0);
        assert!(terminal.kitty_graphics_animation_tick(deadline)?.is_some());
        let image = terminal.kitty_graphics()?.image(2).unwrap();
        assert_eq!(image.pixels()?, Some(&[0, 0, 255, 255][..]));
        assert_ne!(image.generation()?, generation);
        terminal.vt_write(b"\x1b_Ga=a,i=2,s=1,q=2\x1b\\");
        assert_eq!(terminal.kitty_graphics_animation_tick(deadline)?, None);
        Ok(())
    }
}
