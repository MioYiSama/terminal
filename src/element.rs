//! Custom GPUI element that lays out and paints the terminal cell grid.
//!
//! Each row is shaped as one line (so font fallback and emoji work), then
//! every glyph is placed at the x position of the cell it came from, which
//! keeps the grid exact regardless of font advances.

use gpui::{
    App, BorderStyle, Bounds, ContentMask, Element, ElementId, Entity, Focusable,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, MouseMoveEvent, PaintQuad,
    Pixels, Point, Rgba, ShapedLine, SharedString, Style, TextRun, UnderlineStyle, Window,
    DispatchPhase, fill, outline, point, px, relative, size,
};

use crate::view::{GridLayout, PADDING, TerminalInputHandler, TerminalView};
use crate::vt::{CursorShape, Frame, Rgb, Underline};

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
    decorations: Vec<PaintQuad>,
    /// Filled block cursor, painted beneath the text.
    cursor_block: Option<PaintQuad>,
    /// Bar, underline or hollow cursor, painted above the text.
    cursor_overlay: Option<PaintQuad>,
    /// Cell whose glyph is drawn in a contrasting color under a block cursor.
    cursor_text: Option<(usize, u16, Hsla)>,
    marked: Option<(Point<Pixels>, ShapedLine, PaintQuad)>,
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
        self.view.update(cx, |view, _cx| {
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
            let descent = text_system.descent(font_id, font_size);
            let cell_width = snap(advance, scale);
            let line_height = snap((ascent + descent).max(font_size * 1.2), scale);
            let baseline = snap(text_system.baseline_offset(font_id, font_size, line_height), scale);

            let inner = bounds.size - size(PADDING * 2., PADDING * 2.);
            let cols = (f32::from(inner.width) / f32::from(cell_width)).floor().max(1.) as u16;
            let rows = (f32::from(inner.height) / f32::from(line_height)).floor().max(1.) as u16;
            let layout = GridLayout {
                bounds,
                cell_width,
                line_height,
                cols,
                rows,
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
            let frame = view.snapshot();
            build(
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
            )
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
            for quad in prepared.backgrounds.drain(..) {
                window.paint_quad(quad);
            }
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

            for quad in prepared.decorations.drain(..) {
                window.paint_quad(quad);
            }
            if let Some(quad) = prepared.cursor_overlay.take() {
                window.paint_quad(quad);
            }
            if let Some((position, line, background)) = prepared.marked.take() {
                window.paint_quad(background);
                let _ = line.paint(position, layout.line_height, window, cx);
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
        decorations,
        cursor_block,
        cursor_overlay,
        cursor_text,
        marked,
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
