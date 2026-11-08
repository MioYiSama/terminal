//! Cell-filling Unicode box drawing and block elements, independent of font metrics.
//!
//! Rectangles use integer device coordinates. Junction rectangles are unioned
//! before painting so faint text never gets darker where strokes meet.

use gpui::{
    Bounds, FillOptions, FillRule, Hsla, PathBuilder, PathStyle, Pixels, Window, fill, point, px, size,
};

// Left, right, up, down: absent, light, heavy, double.
const STROKES: [[u8; 4]; 0x6d] = [
    [1, 1, 0, 0], // 2500 ─
    [2, 2, 0, 0], // 2501 ━
    [0, 0, 1, 1], // 2502 │
    [0, 0, 2, 2], // 2503 ┃
    [1, 1, 0, 0], // 2504 ┄
    [2, 2, 0, 0], // 2505 ┅
    [0, 0, 1, 1], // 2506 ┆
    [0, 0, 2, 2], // 2507 ┇
    [1, 1, 0, 0], // 2508 ┈
    [2, 2, 0, 0], // 2509 ┉
    [0, 0, 1, 1], // 250A ┊
    [0, 0, 2, 2], // 250B ┋
    [0, 1, 0, 1], // 250C ┌
    [0, 2, 0, 1], // 250D ┍
    [0, 1, 0, 2], // 250E ┎
    [0, 2, 0, 2], // 250F ┏
    [1, 0, 0, 1], // 2510 ┐
    [2, 0, 0, 1], // 2511 ┑
    [1, 0, 0, 2], // 2512 ┒
    [2, 0, 0, 2], // 2513 ┓
    [0, 1, 1, 0], // 2514 └
    [0, 2, 1, 0], // 2515 ┕
    [0, 1, 2, 0], // 2516 ┖
    [0, 2, 2, 0], // 2517 ┗
    [1, 0, 1, 0], // 2518 ┘
    [2, 0, 1, 0], // 2519 ┙
    [1, 0, 2, 0], // 251A ┚
    [2, 0, 2, 0], // 251B ┛
    [0, 1, 1, 1], // 251C ├
    [0, 2, 1, 1], // 251D ┝
    [0, 1, 2, 1], // 251E ┞
    [0, 1, 1, 2], // 251F ┟
    [0, 1, 2, 2], // 2520 ┠
    [0, 2, 2, 1], // 2521 ┡
    [0, 2, 1, 2], // 2522 ┢
    [0, 2, 2, 2], // 2523 ┣
    [1, 0, 1, 1], // 2524 ┤
    [2, 0, 1, 1], // 2525 ┥
    [1, 0, 2, 1], // 2526 ┦
    [1, 0, 1, 2], // 2527 ┧
    [1, 0, 2, 2], // 2528 ┨
    [2, 0, 2, 1], // 2529 ┩
    [2, 0, 1, 2], // 252A ┪
    [2, 0, 2, 2], // 252B ┫
    [1, 1, 0, 1], // 252C ┬
    [2, 1, 0, 1], // 252D ┭
    [1, 2, 0, 1], // 252E ┮
    [2, 2, 0, 1], // 252F ┯
    [1, 1, 0, 2], // 2530 ┰
    [2, 1, 0, 2], // 2531 ┱
    [1, 2, 0, 2], // 2532 ┲
    [2, 2, 0, 2], // 2533 ┳
    [1, 1, 1, 0], // 2534 ┴
    [2, 1, 1, 0], // 2535 ┵
    [1, 2, 1, 0], // 2536 ┶
    [2, 2, 1, 0], // 2537 ┷
    [1, 1, 2, 0], // 2538 ┸
    [2, 1, 2, 0], // 2539 ┹
    [1, 2, 2, 0], // 253A ┺
    [2, 2, 2, 0], // 253B ┻
    [1, 1, 1, 1], // 253C ┼
    [2, 1, 1, 1], // 253D ┽
    [1, 2, 1, 1], // 253E ┾
    [2, 2, 1, 1], // 253F ┿
    [1, 1, 2, 1], // 2540 ╀
    [1, 1, 1, 2], // 2541 ╁
    [1, 1, 2, 2], // 2542 ╂
    [2, 1, 2, 1], // 2543 ╃
    [1, 2, 2, 1], // 2544 ╄
    [2, 1, 1, 2], // 2545 ╅
    [1, 2, 1, 2], // 2546 ╆
    [2, 2, 2, 1], // 2547 ╇
    [2, 2, 1, 2], // 2548 ╈
    [2, 1, 2, 2], // 2549 ╉
    [1, 2, 2, 2], // 254A ╊
    [2, 2, 2, 2], // 254B ╋
    [1, 1, 0, 0], // 254C ╌
    [2, 2, 0, 0], // 254D ╍
    [0, 0, 1, 1], // 254E ╎
    [0, 0, 2, 2], // 254F ╏
    [3, 3, 0, 0], // 2550 ═
    [0, 0, 3, 3], // 2551 ║
    [0, 3, 0, 1], // 2552 ╒
    [0, 1, 0, 3], // 2553 ╓
    [0, 3, 0, 3], // 2554 ╔
    [3, 0, 0, 1], // 2555 ╕
    [1, 0, 0, 3], // 2556 ╖
    [3, 0, 0, 3], // 2557 ╗
    [0, 3, 1, 0], // 2558 ╘
    [0, 1, 3, 0], // 2559 ╙
    [0, 3, 3, 0], // 255A ╚
    [3, 0, 1, 0], // 255B ╛
    [1, 0, 3, 0], // 255C ╜
    [3, 0, 3, 0], // 255D ╝
    [0, 3, 1, 1], // 255E ╞
    [0, 1, 3, 3], // 255F ╟
    [0, 3, 3, 3], // 2560 ╠
    [3, 0, 1, 1], // 2561 ╡
    [1, 0, 3, 3], // 2562 ╢
    [3, 0, 3, 3], // 2563 ╣
    [3, 3, 0, 1], // 2564 ╤
    [1, 1, 0, 3], // 2565 ╥
    [3, 3, 0, 3], // 2566 ╦
    [3, 3, 1, 0], // 2567 ╧
    [1, 1, 3, 0], // 2568 ╨
    [3, 3, 3, 0], // 2569 ╩
    [3, 3, 1, 1], // 256A ╪
    [1, 1, 3, 3], // 256B ╫
    [3, 3, 3, 3], // 256C ╬
];
const HALF_STROKES: [[u8; 4]; 12] = [
    [1, 0, 0, 0], [0, 0, 1, 0], [0, 1, 0, 0], [0, 0, 0, 1],
    [2, 0, 0, 0], [0, 0, 2, 0], [0, 2, 0, 0], [0, 0, 0, 2],
    [1, 2, 0, 0], [0, 0, 1, 2], [2, 1, 0, 0], [0, 0, 2, 1],
];

pub fn character(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    (('\u{2500}'..='\u{259f}').contains(&ch) && chars.next().is_none()).then_some(ch)
}

#[derive(Clone, Copy, Default)]
struct Rect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Rect {
    fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }
}

// At most eight arm rectangles, eight dashes, or four block quadrants.
#[derive(Default)]
struct Rects {
    items: [Rect; 8],
    len: usize,
}

impl Rects {
    fn push(&mut self, rect: Rect) {
        if rect.x0 < rect.x1 && rect.y0 < rect.y1 {
            self.items[self.len] = rect;
            self.len += 1;
        }
    }

    /// Sweep disjoint horizontal slabs, merging their covered x intervals.
    /// No intermediate heap allocation, and no pixel is alpha-blended twice.
    fn union(&self, mut emit: impl FnMut(Rect)) {
        let mut edges = [0; 16];
        for (i, rect) in self.items[..self.len].iter().enumerate() {
            edges[2 * i] = rect.y0;
            edges[2 * i + 1] = rect.y1;
        }
        let edges = &mut edges[..2 * self.len];
        edges.sort_unstable();
        for pair in edges.windows(2) {
            let [y0, y1] = [pair[0], pair[1]];
            if y0 == y1 {
                continue;
            }
            let mut spans = [(0, 0); 8];
            let mut count = 0;
            for rect in &self.items[..self.len] {
                if rect.y0 <= y0 && rect.y1 >= y1 {
                    spans[count] = (rect.x0, rect.x1);
                    count += 1;
                }
            }
            let spans = &mut spans[..count];
            spans.sort_unstable();
            let Some(&(mut x0, mut x1)) = spans.first() else { continue };
            for &(left, right) in &spans[1..] {
                if left <= x1 {
                    x1 = x1.max(right);
                } else {
                    emit(Rect::new(x0, y0, x1, y1));
                    (x0, x1) = (left, right);
                }
            }
            emit(Rect::new(x0, y0, x1, y1));
        }
    }
}

/// One or two parallel lanes, centered independently of the adjoining cell.
fn lanes(length: i32, weight: u8, light: i32) -> ([(i32, i32); 2], usize) {
    let width = if weight == 2 { light * 2 } else { light }.min(length);
    if weight == 3 {
        let width = light.min((length / 3).max(1));
        let start = (length - width * 3) / 2;
        ([(start, start + width), (start + width * 2, start + width * 3)], 2)
    } else {
        let start = (length - width) / 2;
        ([(start, start + width), (0, 0)], 1)
    }
}

/// End a half-stroke at its adjoining perpendicular lane. For double lines,
/// the quadrant's two strokes turn into each other rather than forming a
/// filled square (or drawing a spurious line through the center of ╬).
fn join(
    length: i32,
    light: i32,
    perpendicular: [u8; 2],
    side: usize,
    lane: usize,
    double: bool,
) -> i32 {
    let weight = perpendicular[0].max(perpendicular[1]);
    if weight == 0 {
        return length / 2;
    }
    let (lanes, count) = lanes(length, weight, light);
    let target = if count == 1 {
        0
    } else if double {
        if perpendicular[lane] != 0 { side } else { 1 - side }
    } else if perpendicular[0] != 0 && perpendicular[1] != 0 {
        side
    } else {
        1 - side
    };
    if side == 0 { lanes[target].1 } else { lanes[target].0 }
}

fn strokes(rects: &mut Rects, weights: [u8; 4], w: i32, h: i32, light: i32) {
    for (side, &weight) in weights.iter().enumerate() {
        if weight == 0 {
            continue;
        }
        let horizontal = side < 2;
        let (length, cross_length, perpendicular) = if horizontal {
            (w, h, [weights[2], weights[3]])
        } else {
            (h, w, [weights[0], weights[1]])
        };
        let (lanes, count) = lanes(cross_length, weight, light);
        for (lane, &(a, b)) in lanes[..count].iter().enumerate() {
            let end = join(length, light, perpendicular, side % 2, lane, weight == 3);
            let (from, to) = if side % 2 == 0 { (0, end) } else { (end, length) };
            rects.push(if horizontal {
                Rect::new(from, a, to, b)
            } else {
                Rect::new(a, from, b, to)
            });
        }
    }
}

fn dashes(rects: &mut Rects, ch: u32, w: i32, h: i32, light: i32) {
    let count = match ch {
        0x2504..=0x2507 => 3,
        0x2508..=0x250b => 4,
        _ => 2,
    };
    let horizontal = ch & 2 == 0;
    let weight = if ch & 1 == 0 { 1 } else { 2 };
    let (length, cross_length) = if horizontal { (w, h) } else { (h, w) };
    let (bands, _) = lanes(cross_length, weight, light);
    let (a, b) = bands[0];
    for dash in 0..count {
        let from = (length * (dash * 4 + 1) + count * 2) / (count * 4);
        let to = (length * (dash * 4 + 4) + count * 2) / (count * 4);
        rects.push(if horizontal {
            Rect::new(from, a, to, b)
        } else {
            Rect::new(a, from, b, to)
        });
    }
}

fn blocks(rects: &mut Rects, ch: u32, w: i32, h: i32) {
    // Shared rounded boundaries keep complementary fractions and quadrants
    // disjoint even when a cell's device-pixel dimensions are odd.
    let fraction = |length: i32, eighths: i32| (length * eighths + 4) / 8;
    match ch {
        0x2580 => rects.push(Rect::new(0, 0, w, fraction(h, 4))),
        0x2581..=0x2588 => rects.push(Rect::new(0, fraction(h, 0x2588 - ch as i32), w, h)),
        0x2589..=0x258f => rects.push(Rect::new(0, 0, fraction(w, 0x2590 - ch as i32), h)),
        0x2590 => rects.push(Rect::new(fraction(w, 4), 0, w, h)),
        0x2591..=0x2593 => rects.push(Rect::new(0, 0, w, h)),
        0x2594 => rects.push(Rect::new(0, 0, w, fraction(h, 1))),
        0x2595 => rects.push(Rect::new(fraction(w, 7), 0, w, h)),
        _ => {
            // Bits: upper left, upper right, lower left, lower right.
            const QUADRANTS: [u8; 10] = [4, 8, 1, 13, 9, 7, 11, 2, 6, 14];
            let mask = QUADRANTS[(ch - 0x2596) as usize];
            let xs = [0, fraction(w, 4), w];
            let ys = [0, fraction(h, 4), h];
            for i in 0..4 {
                if mask & (1 << i) != 0 {
                    let (x, y) = (i % 2, i / 2);
                    rects.push(Rect::new(xs[x], ys[y], xs[x + 1], ys[y + 1]));
                }
            }
        }
    }
}

pub fn paint(
    ch: char,
    bounds: Bounds<Pixels>,
    font_size: Pixels,
    mut color: Hsla,
    window: &mut Window,
) {
    let scale = window.scale_factor();
    let device = |value: Pixels| (f32::from(value) * scale).round() as i32;
    let x = device(bounds.left());
    let y = device(bounds.top());
    let w = device(bounds.right()) - x;
    let h = device(bounds.bottom()) - y;
    if w <= 0 || h <= 0 {
        return;
    }
    let light = ((f32::from(font_size) / 14. * scale).round() as i32).max(1);
    let position = |a: f32, b: f32| point(px((x as f32 + a) / scale), px((y as f32 + b) / scale));
    let code = ch as u32;
    if (0x256d..=0x2573).contains(&code) {
        let (xs, _) = lanes(w, 1, light);
        let (ys, _) = lanes(h, 1, light);
        let cx = (xs[0].0 + xs[0].1) as f32 / 2.;
        let cy = (ys[0].0 + ys[0].1) as f32 / 2.;
        let mut path;
        if code <= 0x2570 {
            path = PathBuilder::stroke(px(light as f32 / scale));
            let right = matches!(code, 0x256d | 0x2570);
            let up = matches!(code, 0x256f | 0x2570);
            let dx = if right { 1. } else { -1. };
            let dy = if up { -1. } else { 1. };
            let radius = cx.min(w as f32 - cx).min(cy).min(h as f32 - cy);
            path.move_to(position(if right { w as f32 } else { 0. }, cy));
            path.line_to(position(cx + dx * radius, cy));
            path.arc_to(
                point(px(radius / scale), px(radius / scale)),
                px(0.), false, right == up,
                position(cx, cy + dy * radius),
            );
            path.line_to(position(cx, if up { 0. } else { h as f32 }));
        } else {
            // Filled strips clipped to the cell: a single tessellated union
            // also prevents the two diagonals in ╳ from double-blending.
            path = PathBuilder::fill().with_style(PathStyle::Fill(
                FillOptions::default().with_fill_rule(FillRule::NonZero),
            ));
            let wf = w as f32;
            let hf = h as f32;
            let offset = light as f32 * wf.hypot(hf) / (2. * hf);
            let rise = offset * hf / wf;
            for ascending in [true, false] {
                if (ascending && code == 0x2572) || (!ascending && code == 0x2571) {
                    continue;
                }
                let points = [(0., 0.), (offset, 0.), (wf, hf - rise),
                    (wf, hf), (wf - offset, hf), (0., rise)];
                for index in 0..points.len() {
                    // Reflection changes winding: reverse its vertices so
                    // NonZero fills the intersection rather than cutting it out.
                    let (a, b) = points[if ascending { points.len() - 1 - index } else { index }];
                    let p = position(a, if ascending { hf - b } else { b });
                    if index == 0 { path.move_to(p); } else { path.line_to(p); }
                }
                path.close();
            }
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
        return;
    }

    let mut rects = Rects::default();
    match code {
        0x2504..=0x250b | 0x254c..=0x254f => dashes(&mut rects, code, w, h, light),
        0x2500..=0x256c => strokes(&mut rects, STROKES[(code - 0x2500) as usize], w, h, light),
        0x2574..=0x257f => strokes(&mut rects, HALF_STROKES[(code - 0x2574) as usize], w, h, light),
        0x2580..=0x259f => {
            blocks(&mut rects, code, w, h);
            if (0x2591..=0x2593).contains(&code) {
                color.a *= (code - 0x2590) as f32 / 4.;
            }
        }
        _ => return,
    }
    rects.union(|r| {
        window.paint_quad(fill(
            Bounds::new(position(r.x0 as f32, r.y0 as f32),
                size(px((r.x1 - r.x0) as f32 / scale), px((r.y1 - r.y0) as f32 / scale))),
            color,
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junction_union_preserves_coverage_without_double_blending() {
        let mut rects = Rects::default();
        strokes(&mut rects, [1, 2, 2, 1], 13, 27, 2);
        let mut coverage = [[0u8; 13]; 27];
        rects.union(|rect| {
            for row in &mut coverage[rect.y0 as usize..rect.y1 as usize] {
                for pixel in &mut row[rect.x0 as usize..rect.x1 as usize] {
                    *pixel += 1;
                }
            }
        });
        for (y, row) in coverage.iter().enumerate() {
            for (x, &paint_count) in row.iter().enumerate() {
                let covered = rects.items[..rects.len].iter().any(|r| {
                    (r.x0..r.x1).contains(&(x as i32)) && (r.y0..r.y1).contains(&(y as i32))
                });
                assert_eq!(paint_count, u8::from(covered));
            }
        }
    }
}
