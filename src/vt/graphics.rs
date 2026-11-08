//! Kitty graphics: the PNG decoder libghostty needs, and image access.

use core::ffi::c_void;
use core::{ptr, slice};

use ghostty_sys::*;

use super::Terminal;

/// Decode Kitty graphics PNG payloads with the `image` crate. libghostty
/// ships no decoder and rejects PNG images until one is installed.
/// Process-global: call once at startup, before any terminal exists.
pub fn install_png_decoder() {
    let decode: unsafe extern "C" fn(
        *mut c_void,
        *const GhosttyAllocator,
        *const u8,
        usize,
        *mut GhosttySysImage,
    ) -> bool = decode_png;
    unsafe { ghostty_sys_set(GHOSTTY_SYS_OPT_DECODE_PNG, decode as *const c_void) };
}

unsafe extern "C" fn decode_png(
    _userdata: *mut c_void,
    allocator: *const GhosttyAllocator,
    data: *const u8,
    len: usize,
    out: *mut GhosttySysImage,
) -> bool {
    let bytes = unsafe { slice::from_raw_parts(data, len) };
    let Ok(image) = image::load_from_memory_with_format(bytes, image::ImageFormat::Png) else {
        return false;
    };
    let image = image.into_rgba8();
    let pixels = image.as_raw();
    // The allocator caps one image's memory: NULL means too large.
    let buffer = unsafe { ghostty_alloc(allocator, pixels.len()) };
    if buffer.is_null() {
        return false;
    }
    unsafe {
        ptr::copy_nonoverlapping(pixels.as_ptr(), buffer, pixels.len());
        *out = GhosttySysImage {
            width: image.width(),
            height: image.height(),
            data: buffer,
            data_len: pixels.len(),
        };
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KittyImageFormat {
    Rgb,
    Rgba,
    GrayAlpha,
    Gray,
}

/// Decoded pixels of a Kitty graphics image (its current animation frame),
/// rows top to bottom without padding.
pub struct KittyImage<'a> {
    pub width: u32,
    pub height: u32,
    pub format: KittyImageFormat,
    pub pixels: &'a [u8],
}

impl Terminal {
    /// The active screen's Kitty graphics storage. Borrowed: valid until the
    /// terminal is next modified.
    fn kitty_graphics(&self) -> Option<GhosttyKittyGraphics> {
        let mut graphics: GhosttyKittyGraphics = ptr::null_mut();
        let result = unsafe {
            ghostty_terminal_get(
                self.raw,
                GHOSTTY_TERMINAL_DATA_KITTY_GRAPHICS,
                &mut graphics as *mut GhosttyKittyGraphics as *mut c_void,
            )
        };
        (result == GHOSTTY_SUCCESS && !graphics.is_null()).then_some(graphics)
    }

    /// The Kitty graphics image `id` on the active screen, if it exists and
    /// its pixels have arrived.
    pub fn kitty_image(&self, id: u32) -> Option<KittyImage<'_>> {
        let graphics = self.kitty_graphics()?;
        let image = unsafe { ghostty_kitty_graphics_image(graphics, id) };
        if image.is_null() {
            return None;
        }
        let mut width = 0u32;
        let mut height = 0u32;
        let mut format: GhosttyKittyImageFormat = GHOSTTY_KITTY_IMAGE_FORMAT_RGBA;
        let mut pixels = GhosttyString {
            ptr: ptr::null(),
            len: 0,
        };
        let keys = [
            GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_WIDTH,
            GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_HEIGHT,
            GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_FORMAT,
            GHOSTTY_KITTY_GRAPHICS_IMAGE_DATA_PIXELS,
        ];
        let mut values = [
            &mut width as *mut u32 as *mut c_void,
            &mut height as *mut u32 as *mut c_void,
            &mut format as *mut GhosttyKittyImageFormat as *mut c_void,
            &mut pixels as *mut GhosttyString as *mut c_void,
        ];
        let result = unsafe {
            ghostty_kitty_graphics_image_get_multi(
                image,
                keys.len(),
                keys.as_ptr(),
                values.as_mut_ptr(),
                ptr::null_mut(),
            )
        };
        if result != GHOSTTY_SUCCESS || pixels.ptr.is_null() {
            return None;
        }
        let format = match format {
            GHOSTTY_KITTY_IMAGE_FORMAT_RGB => KittyImageFormat::Rgb,
            GHOSTTY_KITTY_IMAGE_FORMAT_RGBA => KittyImageFormat::Rgba,
            GHOSTTY_KITTY_IMAGE_FORMAT_GRAY_ALPHA => KittyImageFormat::GrayAlpha,
            GHOSTTY_KITTY_IMAGE_FORMAT_GRAY => KittyImageFormat::Gray,
            _ => return None,
        };
        Some(KittyImage {
            width,
            height,
            format,
            // SAFETY: owned by the terminal and valid until it is next
            // modified, which needs `&mut self`.
            pixels: unsafe { slice::from_raw_parts(pixels.ptr, pixels.len) },
        })
    }

    /// Advance playing Kitty graphics animations to `now_ms` (any clock
    /// with a fixed start). Returns when the next frame is due on that
    /// clock, or `None` if nothing is playing.
    pub fn tick_animations(&mut self, now_ms: u64) -> Option<u64> {
        let graphics = self.kitty_graphics()?;
        let mut next_ms = 0u64;
        let result =
            unsafe { ghostty_kitty_graphics_animation_tick(graphics, now_ms, &mut next_ms) };
        (result == GHOSTTY_SUCCESS).then_some(next_ms)
    }
}
