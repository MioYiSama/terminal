//! Catppuccin colors for the terminal and the app's own windows.

use catppuccin::Flavor;
use gpui::{Hsla, rgb};

use crate::vt::{Rgb, TerminalColors};

fn vt_rgb(color: catppuccin::Rgb) -> Rgb {
    Rgb {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

/// The xterm 256-color palette entry for indices 16..=255: a 6×6×6 color
/// cube followed by a 24-step grayscale ramp.
fn xterm_color(index: u8) -> Rgb {
    if index >= 232 {
        let level = 8 + (index - 232) * 10;
        return Rgb {
            r: level,
            g: level,
            b: level,
        };
    }
    let cube = index - 16;
    let level = |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
    Rgb {
        r: level(cube / 36),
        g: level(cube / 6 % 6),
        b: level(cube % 6),
    }
}

/// Default terminal colors for a flavor, matching Catppuccin's official
/// terminal ports: text on base, a rosewater cursor and the flavor's 16 ANSI
/// colors.
pub fn terminal_colors(flavor: &Flavor) -> TerminalColors {
    let mut palette = std::array::from_fn(|index| xterm_color(index.max(16) as u8));
    for color in flavor.ansi_colors.all_ansi_colors() {
        palette[color.code as usize] = vt_rgb(color.rgb);
    }
    TerminalColors {
        foreground: vt_rgb(flavor.colors.text.rgb),
        background: vt_rgb(flavor.colors.base.rgb),
        cursor: vt_rgb(flavor.colors.rosewater.rgb),
        palette,
    }
}

/// Colors for the app's own windows (settings, about).
#[derive(Clone, Copy)]
pub struct UiColors {
    pub background: Hsla,
    pub panel: Hsla,
    pub border: Hsla,
    pub hover: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
}

impl UiColors {
    pub fn new(flavor: &Flavor) -> Self {
        let colors = &flavor.colors;
        Self {
            background: hsla(colors.base.rgb),
            panel: hsla(colors.mantle.rgb),
            border: hsla(colors.surface0.rgb),
            hover: hsla(colors.surface1.rgb),
            text: hsla(colors.text.rgb),
            muted: hsla(colors.subtext0.rgb),
            accent: hsla(colors.mauve.rgb),
            on_accent: hsla(colors.base.rgb),
        }
    }
}

pub fn hsla(color: catppuccin::Rgb) -> Hsla {
    let catppuccin::Rgb { r, g, b } = color;
    rgb(u32::from_be_bytes([0, r, g, b])).into()
}
