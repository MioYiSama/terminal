//! GPUI textures for Kitty graphics images, cached by image ID and
//! generation so pixels are converted and uploaded only when they change.

use std::collections::{HashMap, hash_map};
use std::sync::Arc;

use gpui::{RenderImage, Window};
use image::{Frame, RgbaImage};

use crate::vt::{ImagePlacement, KittyImage, KittyImageFormat, Terminal};

struct Entry {
    generation: u64,
    image: Arc<RenderImage>,
}

#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<u32, Entry>,
}

impl ImageCache {
    /// Textures for `placements`, paired with them. Placements whose pixels
    /// are not available are left out. Textures of images that are no
    /// longer placed, or whose pixels changed, are released.
    pub fn update(
        &mut self,
        terminal: &Terminal,
        placements: &[ImagePlacement],
        window: &mut Window,
    ) -> Vec<(ImagePlacement, Arc<RenderImage>)> {
        let stale: Vec<u32> = self
            .entries
            .iter()
            .filter(|&(&id, entry)| {
                !placements
                    .iter()
                    .any(|p| p.image_id == id && p.generation == entry.generation)
            })
            .map(|(&id, _)| id)
            .collect();
        for id in stale {
            if let Some(entry) = self.entries.remove(&id) {
                let _ = window.drop_image(entry.image);
            }
        }

        placements
            .iter()
            .filter_map(|placement| {
                let entry = match self.entries.entry(placement.image_id) {
                    hash_map::Entry::Occupied(entry) => entry.into_mut(),
                    hash_map::Entry::Vacant(entry) => {
                        let pixels = terminal.kitty_image(placement.image_id)?;
                        entry.insert(Entry {
                            generation: placement.generation,
                            image: Arc::new(RenderImage::new(vec![Frame::new(bgra(&pixels)?)])),
                        })
                    }
                };
                Some((*placement, entry.image.clone()))
            })
            .collect()
    }
}

/// Convert to the BGRA (straight alpha) layout GPUI images use.
fn bgra(image: &KittyImage) -> Option<RgbaImage> {
    let channels = match image.format {
        KittyImageFormat::Rgb => 3,
        KittyImageFormat::Rgba => 4,
        KittyImageFormat::GrayAlpha => 2,
        KittyImageFormat::Gray => 1,
    };
    let count = image.width as usize * image.height as usize;
    if count == 0 || image.pixels.len() != count * channels {
        return None;
    }
    let mut out = Vec::with_capacity(count * 4);
    for px in image.pixels.chunks_exact(channels) {
        let converted: [u8; 4] = match *px {
            [r, g, b] => [b, g, r, 255],
            [r, g, b, a] => [b, g, r, a],
            [v, a] => [v, v, v, a],
            [v] => [v, v, v, 255],
            _ => unreachable!("chunks have `channels` bytes"),
        };
        out.extend_from_slice(&converted);
    }
    RgbaImage::from_raw(image.width, image.height, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_every_format_to_bgra() {
        let cases: [(KittyImageFormat, &[u8], [u8; 4]); 4] = [
            (KittyImageFormat::Rgb, &[10, 20, 30], [30, 20, 10, 255]),
            (KittyImageFormat::Rgba, &[10, 20, 30, 40], [30, 20, 10, 40]),
            (KittyImageFormat::GrayAlpha, &[7, 99], [7, 7, 7, 99]),
            (KittyImageFormat::Gray, &[7], [7, 7, 7, 255]),
        ];
        for (format, pixels, expected) in cases {
            let image = KittyImage {
                width: 1,
                height: 1,
                format,
                pixels,
            };
            assert_eq!(bgra(&image).unwrap().as_raw().as_slice(), expected, "{format:?}");
        }
    }

    #[test]
    fn rejects_truncated_pixels() {
        let image = KittyImage {
            width: 2,
            height: 1,
            format: KittyImageFormat::Rgb,
            pixels: &[1, 2, 3],
        };
        assert!(bgra(&image).is_none());
    }
}
