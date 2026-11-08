//! Custom GPUI element that lays out and paints the terminal cell grid.
//!
//! Ordinary text in each row is shaped as one line (so font fallback and
//! emoji work), then glyphs are placed at their cells' x positions. Unicode
//! box drawing and block elements instead fill device-aligned cell geometry,
//! independent of font advances and the extra spacing between text rows.

use std::sync::Arc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Corners, CursorStyle, Element, RenderImage, ElementId, Entity, Focusable,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    MouseMoveEvent, PaintQuad, Pixels, Point, Rgba, ShapedLine, SharedString, Style, TextRun,
    UnderlineStyle, Window, DispatchPhase, fill, outline, point, px, relative, size,
};

use crate::box_drawing;
use crate::view::{GridLayout, PADDING, SCROLLBAR_WIDTH, TerminalInputHandler, TerminalView};
use crate::vt::{CursorShape, Frame, ImageLayer, ImagePlacement, Rgb, Underline};

pub struct TerminalElement {
    view: Entity<TerminalView>,
}

impl TerminalElement {
    pub fn new(view: Entity<TerminalView>) -> Self {
        Self { view }
    }
}

fn hsla(rgb: Rgb) -> Hsla {
    Rgba {
        r: rgb.r as f32 / 255.,
        g: rgb.g as f32 / 255.,
        b: rgb.b as f32 / 255.,
        a: 1.,
    }
    .into()
}

/// Snap a length to whole device pixels.
fn snap(value: Pixels, scale: f32) -> Pixels {
    px((f32::from(value) * scale).round().max(1.) / scale)
}

const NO_COLUMN: u16 = u16::MAX;
const THUMB_WIDTH: Pixels = px(6.);
/// Gap between the thumb and the ends of its slot.
const THUMB_INSET: Pixels = px(2.);

struct RowGlyphs {
    line: ShapedLine,
    /// Column of the cell each byte of `line.text` belongs to; `NO_COLUMN`
    /// for separators that must not be drawn.
    byte_columns: Vec<u16>,
    /// Text color per column.
    colors: Vec<Hsla>,
}

pub struct Prepared {
    layout: GridLayout,
    font_size: Pixels,
    baseline: Pixels,
    background: Hsla,
    backgrounds: Vec<PaintQuad>,
    rows: Vec<RowGlyphs>,
    /// Row, column and scalar of cell-filling procedural symbols.
    symbols: Vec<(usize, u16, char)>,
    decorations: Vec<PaintQuad>,
    /// Filled block cursor, painted beneath the text.
    cursor_block: Option<PaintQuad>,
    /// Bar, underline or hollow cursor, painted above the text.
    cursor_overlay: Option<PaintQuad>,
    /// Cell whose glyph is drawn in a contrasting color under a block cursor.
    cursor_text: Option<(usize, u16, Hsla)>,
    marked: Option<(Point<Pixels>, ShapedLine, PaintQuad)>,
    /// Scrollbar thumb, and the track's hitbox for the arrow cursor.
    scrollbar: Option<(PaintQuad, Hitbox)>,
    /// Kitty graphics, sorted by layer and then z-index.
    images: Vec<ImageDraw>,
}

struct ImageDraw {
    layer: ImageLayer,
    image: Arc<RenderImage>,
    /// Where the whole image would land, so the source rectangle ends up
    /// in `clip`.
    bounds: Bounds<Pixels>,
    /// The placement's destination, limited to the grid.
    clip: Bounds<Pixels>,
}

/// Lay out the placements' images over the grid in window coordinates.
fn image_draws(
    layout: GridLayout,
    mut placements: Vec<(ImagePlacement, Arc<RenderImage>)>,
) -> Vec<ImageDraw> {
    placements.sort_by_key(|(p, _)| (p.layer, p.z));
    let origin = layout.bounds.origin + point(PADDING, PADDING);
    let grid = Bounds::new(
        origin,
        size(
            layout.cell_width * layout.cols as f32,
            layout.line_height * layout.rows as f32,
        ),
    );
    placements
        .into_iter()
        .filter_map(|(p, image)| {
            if p.source_width == 0 || p.source_height == 0 {
                return None;
            }
            let dest = Bounds::new(
                origin
                    + point(
                        layout.cell_width * p.col as f32 + layout.logical(p.offset_x as f32),
                        layout.line_height * p.row as f32 + layout.logical(p.offset_y as f32),
                    ),
                size(layout.logical(p.width as f32), layout.logical(p.height as f32)),
            );
            let clip = dest.intersect(&grid);
            if clip.is_empty() {
                return None;
            }
            // Logical pixels per source pixel on each axis.
            let scale_x = f32::from(dest.size.width) / p.source_width as f32;
            let scale_y = f32::from(dest.size.height) / p.source_height as f32;
            let full = image.size(0);
            let bounds = Bounds::new(
                dest.origin - point(px(p.source_x as f32 * scale_x), px(p.source_y as f32 * scale_y)),
                size(px(full.width.0 as f32 * scale_x), px(full.height.0 as f32 * scale_y)),
            );
            Some(ImageDraw {
                layer: p.layer,
                image,
                bounds,
                clip,
            })
        })
        .collect()
}

/// Paint the images in `layer`, which `images` holds as one sorted run.
fn paint_images(images: &[ImageDraw], layer: ImageLayer, window: &mut Window) {
    for draw in images.iter().filter(|draw| draw.layer == layer) {
        window.with_content_mask(Some(ContentMask { bounds: draw.clip }), |window| {
            let _ = window.paint_image(draw.bounds, Corners::default(), draw.image.clone(), 0, false);
        });
    }
}

impl IntoElement for TerminalElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepared;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.view.update(cx, |view, cx| {
            let text_system = window.text_system().clone();
            let scale = window.scale_factor();
            let font = view.font();
            let font_size = view.font_size();
            let font_id = text_system.resolve_font(&font);

            let advance = text_system
                .advance(font_id, font_size, 'M')
                .map(|advance| advance.width)
                .unwrap_or(font_size * 0.6);
            let ascent = text_system.ascent(font_id, font_size);
            // The sign of the reported descent differs by platform (negative
            // on macOS and Windows, positive on Linux); only its size counts.
            let descent = text_system.descent(font_id, font_size).abs();
            let cell_width = snap(advance, scale);
            let line_height = snap((ascent + descent).max(font_size * 1.2), scale);
            // Center the font's ascent + descent box in the cell. GPUI's
            // `baseline_offset` assumes a positive descent, so it is not used.
            let baseline = snap((line_height - ascent - descent) / 2. + ascent, scale);

            let inner = bounds.size - size(PADDING * 2. + SCROLLBAR_WIDTH, PADDING * 2.);
            let cols = (f32::from(inner.width) / f32::from(cell_width)).floor().max(1.) as u16;
            let rows = (f32::from(inner.height) / f32::from(line_height)).floor().max(1.) as u16;
            let layout = GridLayout {
                bounds,
                cell_width,
                line_height,
                cols,
                rows,
                scale,
            };
            view.set_layout(layout);

            let focused = view.is_focused(window);
            let marked_text = SharedString::from(view.marked_text().to_owned());
            let fonts = [
                font.clone(),
                font.clone().bold(),
                font.clone().italic(),
                font.bold().italic(),
            ];
            let frame = view.snapshot(cx);
            let foreground = hsla(frame.foreground);
            let mut prepared = build(
                frame,
                layout,
                &fonts,
                font_size,
                baseline,
                ascent,
                descent,
                focused,
                marked_text,
                window,
            );
            prepared.images = image_draws(layout, view.images(window));
            prepared.scrollbar = view.scrollbar_thumb().map(|(slot, highlighted)| {
                let thumb = Bounds::new(
                    point(
                        slot.center().x - THUMB_WIDTH / 2.,
                        slot.top() + THUMB_INSET,
                    ),
                    size(THUMB_WIDTH, slot.size.height - THUMB_INSET * 2.),
                );
                let color = foreground.opacity(if highlighted { 0.5 } else { 0.25 });
                let quad = fill(thumb, color).corner_radii(THUMB_WIDTH / 2.);
                let hitbox = window.insert_hitbox(layout.scrollbar_track(), HitboxBehavior::Normal);
                (quad, hitbox)
            });
            prepared
        })
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepared: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.view.read(cx).focus_handle(cx);
        window.handle_input(
            &focus_handle,
            TerminalInputHandler::new(bounds, self.view.clone()),
            cx,
        );

        // Track drags and reported motion even when the pointer leaves the
        // element, which hover-scoped listeners would miss.
        let view = self.view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                view.update(cx, |view, cx| view.mouse_move(event, window, cx));
            }
        });

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, prepared.background));
            paint_images(&prepared.images, ImageLayer::BelowBackground, window);
            for quad in prepared.backgrounds.drain(..) {
                window.paint_quad(quad);
            }
            paint_images(&prepared.images, ImageLayer::BelowText, window);
            if let Some(quad) = prepared.cursor_block.take() {
                window.paint_quad(quad);
            }

            let layout = prepared.layout;
            let origin = bounds.origin + point(PADDING, PADDING);
            for (row_index, row) in prepared.rows.iter().enumerate() {
                let y = origin.y + layout.line_height * row_index as f32 + prepared.baseline;
                let mut cluster_column = NO_COLUMN;
                let mut cluster_x = px(0.);
                for run in row.line.runs.iter() {
                    for glyph in &run.glyphs {
                        let column = row.byte_columns.get(glyph.index).copied().unwrap_or(NO_COLUMN);
                        if column == NO_COLUMN {
                            continue;
                        }
                        // Glyphs of one cell keep their relative offsets
                        // (combining marks, multi-glyph clusters).
                        if column != cluster_column {
                            cluster_column = column;
                            cluster_x = glyph.position.x;
                        }
                        let x = origin.x
                            + layout.cell_width * column as f32
                            + (glyph.position.x - cluster_x);
                        let glyph_origin = point(x, y);
                        if glyph.is_emoji {
                            let _ = window.paint_emoji(
                                glyph_origin,
                                run.font_id,
                                glyph.id,
                                prepared.font_size,
                            );
                        } else {
                            let color = match prepared.cursor_text {
                                Some((r, c, color)) if r == row_index && c == column => color,
                                _ => row.colors[column as usize],
                            };
                            let _ = window.paint_glyph(
                                glyph_origin,
                                run.font_id,
                                glyph.id,
                                prepared.font_size,
                                color,
                            );
                        }
                    }
                }
            }

            for &(row, column, ch) in &prepared.symbols {
                let color = match prepared.cursor_text {
                    Some((r, c, color)) if r == row && c == column => color,
                    _ => prepared.rows[row].colors[column as usize],
                };
                box_drawing::paint(
                    ch,
                    Bounds::new(
                        origin + point(
                            layout.cell_width * column as f32,
                            layout.line_height * row as f32,
                        ),
                        size(layout.cell_width, layout.line_height),
                    ),
                    prepared.font_size,
                    color,
                    window,
                );
            }

            for quad in prepared.decorations.drain(..) {
                window.paint_quad(quad);
            }
            paint_images(&prepared.images, ImageLayer::AboveText, window);
            if let Some(quad) = prepared.cursor_overlay.take() {
                window.paint_quad(quad);
            }
            if let Some((position, line, background)) = prepared.marked.take() {
                window.paint_quad(background);
                let _ = line.paint(position, layout.line_height, window, cx);
            }
            if let Some((thumb, hitbox)) = prepared.scrollbar.take() {
                window.paint_quad(thumb);
                window.set_cursor_style(CursorStyle::Arrow, &hitbox);
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    frame: &Frame,
    layout: GridLayout,
    fonts: &[gpui::Font; 4],
    font_size: Pixels,
    baseline: Pixels,
    ascent: Pixels,
    descent: Pixels,
    focused: bool,
    marked_text: SharedString,
    window: &mut Window,
) -> Prepared {
    let origin = layout.bounds.origin + point(PADDING, PADDING);
    let cw = layout.cell_width;
    let lh = layout.line_height;
    let background = hsla(frame.background);
    let thickness = px((f32::from(font_size) / 14.).round().max(1.));
    let underline_y = (baseline + descent * 0.5).min(lh - thickness);
    let strike_y = baseline - ascent * 0.3;

    let mut backgrounds = Vec::new();
    let mut decorations = Vec::new();
    let mut rows = Vec::with_capacity(frame.lines.len());
    let mut symbols = Vec::new();

    for (row_index, line) in frame.lines.iter().enumerate() {
        let top = origin.y + lh * row_index as f32;

        // Cell backgrounds, merging horizontal runs of the same color.
        let mut run: Option<(usize, usize, Rgb)> = None;
        let flush = |run: Option<(usize, usize, Rgb)>, backgrounds: &mut Vec<PaintQuad>| {
            if let Some((start, end, color)) = run {
                backgrounds.push(fill(
                    Bounds::new(
                        point(origin.x + cw * start as f32, top),
                        size(cw * (end - start) as f32, lh),
                    ),
                    hsla(color),
                ));
            }
        };
        for (col, cell) in line.cells.iter().enumerate() {
            match (run, cell.bg) {
                (Some((start, end, color)), Some(bg)) if end == col && color == bg => {
                    run = Some((start, col + 1, color));
                }
                (_, bg) => {
                    flush(run.take(), &mut backgrounds);
                    run = bg.map(|bg| (col, col + 1, bg));
                }
            }
        }
        flush(run, &mut backgrounds);

        // Text and decorations.
        let mut text = String::with_capacity(line.text.len() + 8);
        let mut byte_columns: Vec<u16> = Vec::with_capacity(line.text.len() + 8);
        let mut colors = Vec::with_capacity(line.cells.len());
        let mut runs: Vec<TextRun> = Vec::new();
        let mut next_col = 0usize;
        for (col, cell) in line.cells.iter().enumerate() {
            let mut fg = hsla(cell.fg);
            if cell.faint {
                fg.a *= 0.5;
            }
            colors.push(fg);

            let width = if cell.wide { 2. } else { 1. };
            let x = origin.x + cw * col as f32;
            let deco_color = cell.underline_color.map(hsla).unwrap_or(fg);
            let mut line_at = |y: Pixels, color: Hsla| {
                decorations.push(fill(
                    Bounds::new(point(x, top + y), size(cw * width, thickness)),
                    color,
                ));
            };
            match cell.underline {
                Underline::None => {}
                Underline::Double => {
                    line_at(underline_y, deco_color);
                    line_at((underline_y - thickness * 2.).max(baseline), deco_color);
                }
                // Curly, dotted and dashed underlines are drawn solid.
                _ => line_at(underline_y, deco_color),
            }
            if cell.strikethrough {
                line_at(strike_y, fg);
            }
            if cell.overline {
                line_at(px(0.), fg);
            }

            let cell_text = &line.text[cell.text.start as usize..cell.text.end as usize];
            if cell.invisible || cell_text.is_empty() || cell_text == " " {
                continue;
            }
            if let Some(ch) = box_drawing::character(cell_text) {
                symbols.push((row_index, col as u16, ch));
                continue;
            }
            // Separate non-adjacent cells so they cannot shape together.
            if col != next_col && !text.is_empty() {
                text.push(' ');
                byte_columns.push(NO_COLUMN);
                push_run(&mut runs, fonts, 0, 1);
            }
            text.push_str(cell_text);
            byte_columns.extend(std::iter::repeat_n(col as u16, cell_text.len()));
            let variant = cell.bold as usize | (cell.italic as usize) << 1;
            push_run(&mut runs, fonts, variant, cell_text.len());
            next_col = col + if cell.wide { 2 } else { 1 };
        }

        let line = window
            .text_system()
            .shape_line(text.into(), font_size, &runs, None);
        rows.push(RowGlyphs {
            line,
            byte_columns,
            colors,
        });
    }

    // Cursor. Hidden while the IME shows composition text in its place.
    let mut cursor_block = None;
    let mut cursor_overlay = None;
    let mut cursor_text = None;
    let mut marked = None;
    if let Some(cursor) = frame.cursor {
        let cell_origin = point(origin.x + cw * cursor.x as f32, origin.y + lh * cursor.y as f32);
        let width = cw * if cursor.wide { 2. } else { 1. };
        let color = hsla(cursor.color);
        if !marked_text.is_empty() {
            let run = TextRun {
                len: marked_text.len(),
                font: fonts[0].clone(),
                color: hsla(frame.foreground),
                background_color: None,
                underline: Some(UnderlineStyle {
                    color: Some(hsla(frame.foreground)),
                    thickness: px(1.),
                    wavy: false,
                }),
                strikethrough: None,
            };
            let line = window
                .text_system()
                .shape_line(marked_text, font_size, &[run], None);
            let background_quad = fill(Bounds::new(cell_origin, size(line.width, lh)), background);
            marked = Some((cell_origin, line, background_quad));
        } else {
            let shape = if focused {
                cursor.shape
            } else {
                CursorShape::HollowBlock
            };
            match shape {
                CursorShape::Block => {
                    cursor_block = Some(fill(Bounds::new(cell_origin, size(width, lh)), color));
                    let under = frame
                        .lines
                        .get(cursor.y as usize)
                        .and_then(|line| line.cells.get(cursor.x as usize))
                        .and_then(|cell| cell.bg)
                        .unwrap_or(frame.background);
                    cursor_text = Some((cursor.y as usize, cursor.x, hsla(under)));
                }
                CursorShape::HollowBlock => {
                    cursor_overlay = Some(outline(
                        Bounds::new(cell_origin, size(width, lh)),
                        color,
                        BorderStyle::Solid,
                    ));
                }
                CursorShape::Bar => {
                    let bar = px(f32::from(thickness).max(2.));
                    cursor_overlay = Some(fill(Bounds::new(cell_origin, size(bar, lh)), color));
                }
                CursorShape::Underline => {
                    let bar = px(f32::from(thickness).max(2.));
                    cursor_overlay = Some(fill(
                        Bounds::new(cell_origin + point(px(0.), lh - bar), size(width, bar)),
                        color,
                    ));
                }
            }
        }
    }

    Prepared {
        layout,
        font_size,
        baseline,
        background,
        backgrounds,
        rows,
        symbols,
        decorations,
        cursor_block,
        cursor_overlay,
        cursor_text,
        marked,
        scrollbar: None,
        images: Vec::new(),
    }
}

fn push_run(runs: &mut Vec<TextRun>, fonts: &[gpui::Font; 4], variant: usize, len: usize) {
    let font = &fonts[variant];
    if let Some(last) = runs.last_mut()
        && last.font == *font
    {
        last.len += len;
        return;
    }
    runs.push(TextRun {
        len,
        font: font.clone(),
        // Glyph colors are applied at paint time. The run color only has to
        // differ between font variants: GPUI merges adjacent runs whose
        // decorations match even when their fonts differ.
        color: Hsla {
            h: 0.,
            s: 0.,
            l: 0.,
            a: variant as f32 / 4.,
        },
        background_color: None,
        underline: None,
        strikethrough: None,
    });
}
