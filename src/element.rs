//! Custom GPUI element that lays out and paints the terminal cell grid.
//!
//! Ordinary text in each row is shaped as one line (so font fallback and
//! emoji work), then glyphs are placed at their cells' x positions. Unicode
//! box drawing and block elements instead fill device-aligned cell geometry,
//! independent of font advances and the extra spacing between text rows.
//! Laid-out rows are kept between frames and redone only for terminal lines
//! that changed.

use std::mem;
use std::sync::Arc;

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Corners, CursorStyle, Element, RenderImage, ElementId, Entity, Focusable,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    MouseExitEvent, MouseMoveEvent, PaintQuad, Pixels, Point, Rgba, ShapedLine, SharedString, Style, TextRun,
    UnderlineStyle, Window, DispatchPhase, fill, outline, point, px, relative, size,
};

use crate::box_drawing;
use crate::links::Link;
use crate::grid::{GridLayout, PADDING, SCROLLBAR_WIDTH};
use crate::view::{TerminalInputHandler, TerminalView};
use crate::vt::{CursorShape, Frame, ImageLayer, ImagePlacement, Line, Rgb, Underline};

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

/// Font metrics of the grid's rows, snapped to device pixels.
#[derive(Clone, Copy, PartialEq)]
struct Metrics {
    font_size: Pixels,
    ascent: Pixels,
    cell_width: Pixels,
    line_height: Pixels,
    /// Offset of the baseline within a row.
    baseline: Pixels,
    /// Thickness of underlines and other strokes.
    thickness: Pixels,
    /// Offset of underlines within a row.
    underline_y: Pixels,
}

impl Metrics {
    fn new(font: &gpui::Font, font_size: Pixels, window: &Window) -> Self {
        let text_system = window.text_system();
        let scale = window.scale_factor();
        let font_id = text_system.resolve_font(font);
        let advance = text_system
            .advance(font_id, font_size, 'M')
            .map(|advance| advance.width)
            .unwrap_or(font_size * 0.6);
        let ascent = text_system.ascent(font_id, font_size);
        // The sign of the reported descent differs by platform (negative on
        // macOS and Windows, positive on Linux); only its size counts.
        let descent = text_system.descent(font_id, font_size).abs();
        let line_height = snap((ascent + descent).max(font_size * 1.2), scale);
        // Center the font's ascent + descent box in the cell. GPUI's
        // `baseline_offset` assumes a positive descent, so it is not used.
        let baseline = snap((line_height - ascent - descent) / 2. + ascent, scale);
        let thickness = px((f32::from(font_size) / 14.).round().max(1.));
        Self {
            font_size,
            ascent,
            cell_width: snap(advance, scale),
            line_height,
            baseline,
            thickness,
            underline_y: (baseline + descent * 0.5).min(line_height - thickness),
        }
    }
}

/// One row's cells, laid out in window coordinates.
struct Row {
    key: RowKey,
    glyphs: RowGlyphs,
    backgrounds: Vec<PaintQuad>,
    decorations: Vec<PaintQuad>,
    /// Column and scalar of cell-filling procedural symbols.
    symbols: Vec<(u16, char)>,
}

/// Everything a row is laid out from, besides the font family.
#[derive(Clone, Copy, PartialEq)]
struct RowKey {
    /// The terminal line's version, which changes with its contents.
    version: u64,
    origin: Point<Pixels>,
    metrics: Metrics,
}

/// Rows laid out in the last frame, kept as element state so rows that
/// did not change are not laid out again.
#[derive(Default)]
struct RowCache {
    font: Option<gpui::Font>,
    rows: Vec<Row>,
}

/// The cursor, or the IME composition shown in its place.
#[derive(Default)]
struct CursorDraw {
    /// Filled block cursor, painted beneath the text.
    block: Option<PaintQuad>,
    /// Bar, underline or hollow cursor, painted above the text.
    overlay: Option<PaintQuad>,
    /// Cell whose glyph is drawn in a contrasting color under a block cursor.
    text: Option<(usize, u16, Hsla)>,
    marked: Option<(Point<Pixels>, ShapedLine, PaintQuad)>,
}

pub struct Prepared {
    layout: GridLayout,
    metrics: Metrics,
    background: Hsla,
    font: gpui::Font,
    rows: Vec<Row>,
    cursor: CursorDraw,
    /// Scrollbar thumb, and the track's hitbox for the arrow cursor.
    scrollbar: Option<(PaintQuad, Hitbox)>,
    /// Kitty graphics, sorted by layer and then z-index.
    images: Vec<ImageDraw>,
    link: Option<LinkDraw>,
}

/// The hovered link: underlined, with its URL shown in a corner and, when a
/// click would open it, a pointing-hand cursor.
struct LinkDraw {
    underlines: Vec<PaintQuad>,
    preview: (Point<Pixels>, ShapedLine, PaintQuad),
    hitbox: Option<Hitbox>,
}

fn link_draw(
    link: &Link,
    clickable: bool,
    prepared: &Prepared,
    fonts: &[gpui::Font; 4],
    foreground: Hsla,
    window: &mut Window,
) -> LinkDraw {
    let layout = prepared.layout;
    let (cw, lh) = (layout.cell_width, layout.line_height);
    let underlines = link
        .spans
        .iter()
        .map(|(row, cols)| {
            let start = layout.cell_origin(cols.start as f32, *row as f32);
            fill(
                Bounds::new(
                    start + point(px(0.), prepared.metrics.underline_y),
                    size(cw * cols.len() as f32, prepared.metrics.thickness),
                ),
                foreground,
            )
        })
        .collect();

    // Long URLs are cut to the grid width.
    let max_chars = (layout.cols as usize).saturating_sub(2).max(1);
    let text: SharedString = if link.url.chars().count() > max_chars {
        let mut cut: String = link.url.chars().take(max_chars - 1).collect();
        cut.push('…');
        cut.into()
    } else {
        link.url.clone().into()
    };
    let run = TextRun {
        len: text.len(),
        font: fonts[0].clone(),
        color: prepared.background,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text, prepared.metrics.font_size, &[run], None);
    let pad = cw / 2.;
    let chip = size(line.width + pad * 2., lh);
    // Bottom-left, or bottom-right when that would cover the link itself.
    let bottom = layout.bounds.bottom() - PADDING - chip.height;
    let left = layout.bounds.left() + PADDING;
    let covers_link = link.spans.iter().any(|(row, cols)| {
        *row + 1 >= layout.rows && layout.cell_origin(cols.start as f32, 0.).x <= left + chip.width
    });
    let x = if covers_link {
        layout.bounds.right() - PADDING - SCROLLBAR_WIDTH - chip.width
    } else {
        left
    };
    let chip_bounds = Bounds::new(point(x, bottom), chip);
    let background = fill(chip_bounds, foreground.opacity(0.9)).corner_radii(px(4.));
    let hitbox = clickable.then(|| window.insert_hitbox(layout.bounds, HitboxBehavior::Normal));
    LinkDraw {
        underlines,
        preview: (point(x + pad, bottom), line, background),
        hitbox,
    }
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
    let grid = layout.grid_bounds();
    placements
        .into_iter()
        .filter_map(|(p, image)| {
            if p.source_width == 0 || p.source_height == 0 {
                return None;
            }
            let dest = Bounds::new(
                layout.cell_origin(p.col as f32, p.row as f32)
                    + point(layout.logical(p.offset_x as f32), layout.logical(p.offset_y as f32)),
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
        Some("terminal-grid".into())
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
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.view.update(cx, |view, cx| {
            let font = view.font();
            let metrics = Metrics::new(&font, view.font_size(), window);
            let layout = GridLayout::fit(
                bounds,
                metrics.cell_width,
                metrics.line_height,
                window.scale_factor(),
            );
            view.set_layout(layout);

            let focused = view.is_focused(window);
            let marked_text = SharedString::from(view.marked_text().to_owned());
            let fonts = [
                font.clone(),
                font.clone().bold(),
                font.clone().italic(),
                font.clone().bold().italic(),
            ];
            // Paint hands the rows back for the next frame.
            let cache = window.with_element_state(id.expect("element has an id"), |cache, _| {
                (cache.unwrap_or_default(), RowCache::default())
            });
            let frame = view.snapshot(cx);
            let foreground = hsla(frame.foreground);
            let rows = build_rows(frame, layout, &fonts, &metrics, cache, window);
            let mut prepared = Prepared {
                layout,
                metrics,
                background: hsla(frame.background),
                font,
                rows,
                cursor: build_cursor(frame, layout, &fonts, &metrics, focused, marked_text, window),
                scrollbar: None,
                images: Vec::new(),
                link: None,
            };
            prepared.images = image_draws(layout, view.images(window));
            let clickable = view.link_clickable();
            prepared.link = view
                .hovered_link()
                .map(|link| link_draw(link, clickable, &prepared, &fonts, foreground, window));
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
        id: Option<&GlobalElementId>,
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
        let view = self.view.clone();
        window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                view.update(cx, |view, cx| view.mouse_exit(cx));
            }
        });

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, prepared.background));
            paint_images(&prepared.images, ImageLayer::BelowBackground, window);
            for quad in prepared.rows.iter().flat_map(|row| &row.backgrounds) {
                window.paint_quad(quad.clone());
            }
            paint_images(&prepared.images, ImageLayer::BelowText, window);
            if let Some(quad) = prepared.cursor.block.take() {
                window.paint_quad(quad);
            }
            paint_text(prepared, window);
            for quad in prepared.rows.iter().flat_map(|row| &row.decorations) {
                window.paint_quad(quad.clone());
            }
            paint_images(&prepared.images, ImageLayer::AboveText, window);
            if let Some(quad) = prepared.cursor.overlay.take() {
                window.paint_quad(quad);
            }
            let line_height = prepared.layout.line_height;
            if let Some((position, line, background)) = prepared.cursor.marked.take() {
                window.paint_quad(background);
                let _ = line.paint(position, line_height, window, cx);
            }
            if let Some(link) = prepared.link.take() {
                for quad in link.underlines {
                    window.paint_quad(quad);
                }
                let (position, line, background) = link.preview;
                window.paint_quad(background);
                let _ = line.paint(position, line_height, window, cx);
                if let Some(hitbox) = &link.hitbox {
                    window.set_cursor_style(CursorStyle::PointingHand, hitbox);
                }
            }
            if let Some((thumb, hitbox)) = prepared.scrollbar.take() {
                window.paint_quad(thumb);
                window.set_cursor_style(CursorStyle::Arrow, &hitbox);
            }
        });

        let cache = RowCache {
            font: Some(prepared.font.clone()),
            rows: mem::take(&mut prepared.rows),
        };
        window.with_element_state(id.expect("element has an id"), |_: Option<RowCache>, _| {
            ((), cache)
        });
    }
}

/// Paint the glyphs of the rows and the procedural symbols, each at its
/// cell.
fn paint_text(prepared: &Prepared, window: &mut Window) {
    let layout = prepared.layout;
    let font_size = prepared.metrics.font_size;
    let color_at = |row: usize, column: u16| match prepared.cursor.text {
        Some((r, c, color)) if r == row && c == column => color,
        _ => prepared.rows[row].glyphs.colors[column as usize],
    };
    for (row_index, row) in prepared.rows.iter().enumerate() {
        let y = layout.cell_origin(0., row_index as f32).y + prepared.metrics.baseline;
        let glyphs = &row.glyphs;
        let mut cluster_column = NO_COLUMN;
        let mut cluster_x = px(0.);
        for run in glyphs.line.runs.iter() {
            for glyph in &run.glyphs {
                let column = glyphs.byte_columns.get(glyph.index).copied().unwrap_or(NO_COLUMN);
                if column == NO_COLUMN {
                    continue;
                }
                // Glyphs of one cell keep their relative offsets (combining
                // marks, multi-glyph clusters).
                if column != cluster_column {
                    cluster_column = column;
                    cluster_x = glyph.position.x;
                }
                let x = layout.cell_origin(column as f32, 0.).x + (glyph.position.x - cluster_x);
                let glyph_origin = point(x, y);
                if glyph.is_emoji {
                    let _ = window.paint_emoji(glyph_origin, run.font_id, glyph.id, font_size);
                } else {
                    let color = color_at(row_index, column);
                    let _ = window.paint_glyph(glyph_origin, run.font_id, glyph.id, font_size, color);
                }
            }
        }

        for &(column, ch) in &row.symbols {
            let cell = Bounds::new(
                layout.cell_origin(column as f32, row_index as f32),
                size(layout.cell_width, layout.line_height),
            );
            box_drawing::paint(ch, cell, font_size, color_at(row_index, column), window);
        }
    }
}

/// Lay out the frame's rows, reusing the cached rows whose line, position
/// and font are unchanged.
fn build_rows(
    frame: &Frame,
    layout: GridLayout,
    fonts: &[gpui::Font; 4],
    metrics: &Metrics,
    cache: RowCache,
    window: &mut Window,
) -> Vec<Row> {
    let mut cached = if cache.font.as_ref() == Some(&fonts[0]) {
        cache.rows
    } else {
        Vec::new()
    };
    cached.truncate(frame.lines.len());
    let mut cached = cached.into_iter();
    frame
        .lines
        .iter()
        .enumerate()
        .map(|(row_index, line)| {
            let key = RowKey {
                version: line.version,
                origin: layout.cell_origin(0., row_index as f32),
                metrics: *metrics,
            };
            match cached.next() {
                Some(row) if row.key == key => row,
                _ => build_row(line, key, fonts, window),
            }
        })
        .collect()
}

/// Shape a row's text and lay out its cell backgrounds and decorations.
fn build_row(line: &Line, key: RowKey, fonts: &[gpui::Font; 4], window: &mut Window) -> Row {
    let metrics = &key.metrics;
    let row_origin = key.origin;
    let cw = metrics.cell_width;
    let lh = metrics.line_height;
    let Metrics {
        thickness,
        underline_y,
        baseline,
        ..
    } = *metrics;
    let strike_y = baseline - metrics.ascent * 0.3;

    let mut backgrounds = Vec::new();
    let mut decorations = Vec::new();
    let mut symbols = Vec::new();

    // Cell backgrounds, merging horizontal runs of the same color.
    let mut run: Option<(usize, usize, Rgb)> = None;
    let flush = |run: Option<(usize, usize, Rgb)>, backgrounds: &mut Vec<PaintQuad>| {
        if let Some((start, end, color)) = run {
            backgrounds.push(fill(
                Bounds::new(
                    row_origin + point(cw * start as f32, px(0.)),
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
        let cell_origin = row_origin + point(cw * col as f32, px(0.));
        let deco_color = cell.underline_color.map(hsla).unwrap_or(fg);
        let mut line_at = |y: Pixels, color: Hsla| {
            decorations.push(fill(
                Bounds::new(cell_origin + point(px(0.), y), size(cw * width, thickness)),
                color,
            ));
        };
        match cell.underline {
            // OSC 8 hyperlinks are underlined unless styled otherwise.
            Underline::None if cell.hyperlink => line_at(underline_y, deco_color),
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
            symbols.push((col as u16, ch));
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
        .shape_line(text.into(), metrics.font_size, &runs, None);
    Row {
        key,
        glyphs: RowGlyphs {
            line,
            byte_columns,
            colors,
        },
        backgrounds,
        decorations,
        symbols,
    }
}

/// The cursor in its shape, hollow when unfocused; hidden while the IME
/// shows composition text in its place.
fn build_cursor(
    frame: &Frame,
    layout: GridLayout,
    fonts: &[gpui::Font; 4],
    metrics: &Metrics,
    focused: bool,
    marked_text: SharedString,
    window: &mut Window,
) -> CursorDraw {
    let Some(cursor) = frame.cursor else {
        return CursorDraw::default();
    };
    let lh = layout.line_height;
    let cell_origin = layout.cell_origin(cursor.x as f32, cursor.y as f32);
    let width = layout.cell_width * if cursor.wide { 2. } else { 1. };
    let color = hsla(cursor.color);
    let mut draw = CursorDraw::default();

    if !marked_text.is_empty() {
        let foreground = hsla(frame.foreground);
        let run = TextRun {
            len: marked_text.len(),
            font: fonts[0].clone(),
            color: foreground,
            background_color: None,
            underline: Some(UnderlineStyle {
                color: Some(foreground),
                thickness: px(1.),
                wavy: false,
            }),
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line(marked_text, metrics.font_size, &[run], None);
        let background = fill(Bounds::new(cell_origin, size(line.width, lh)), hsla(frame.background));
        draw.marked = Some((cell_origin, line, background));
        return draw;
    }

    let shape = if focused {
        cursor.shape
    } else {
        CursorShape::HollowBlock
    };
    let bar = px(f32::from(metrics.thickness).max(2.));
    match shape {
        CursorShape::Block => {
            draw.block = Some(fill(Bounds::new(cell_origin, size(width, lh)), color));
            let under = frame
                .lines
                .get(cursor.y as usize)
                .and_then(|line| line.cells.get(cursor.x as usize))
                .and_then(|cell| cell.bg)
                .unwrap_or(frame.background);
            draw.text = Some((cursor.y as usize, cursor.x, hsla(under)));
        }
        CursorShape::HollowBlock => {
            draw.overlay = Some(outline(
                Bounds::new(cell_origin, size(width, lh)),
                color,
                BorderStyle::Solid,
            ));
        }
        CursorShape::Bar => {
            draw.overlay = Some(fill(Bounds::new(cell_origin, size(bar, lh)), color));
        }
        CursorShape::Underline => {
            draw.overlay = Some(fill(
                Bounds::new(cell_origin + point(px(0.), lh - bar), size(width, bar)),
                color,
            ));
        }
    }
    draw
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
