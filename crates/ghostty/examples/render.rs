//! Print a styled terminal frame using only the safe render API.
use ghostty::{
    Result, Terminal,
    point::Point,
    render::{Dirty, RenderState, RowCells, RowIterator},
    style::{StyleFlags, Underline},
};
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(40, 5)?;
    terminal.vt_write(
        b"Hello, \x1b[1;32mworld\x1b[0m!\r\n\x1b[4munderlined\x1b[0m text\r\n\x1b[38;2;255;128;0morange\x1b[0m\r\n",
    );
    terminal.set_selection(Point::active(0, 1), Point::active(9, 1), false)?;
    let mut state = RenderState::new()?;
    state.update(&terminal)?;
    let mut output = io::stdout().lock();
    writeln!(
        output,
        "{}",
        match state.dirty()? {
            Dirty::Clean => "Frame is clean, nothing to draw.",
            Dirty::Partial => "Partial redraw needed.",
            Dirty::Full => "Full redraw needed.",
        }
    )?;
    let colors = state.colors()?;
    writeln!(
        output,
        "Background: #{:02x}{:02x}{:02x}",
        colors.background.r, colors.background.g, colors.background.b
    )?;
    writeln!(
        output,
        "Foreground: #{:02x}{:02x}{:02x}",
        colors.foreground.r, colors.foreground.g, colors.foreground.b
    )?;
    let cursor = state.cursor()?;
    if cursor.visible
        && let Some(position) = cursor.viewport
    {
        writeln!(
            output,
            "Cursor at ({}, {}), style: {:?}",
            position.x, position.y, cursor.visual_style
        )?;
    }
    let mut row_storage = RowIterator::new()?;
    let mut cell_storage = RowCells::new()?;
    let mut rows = state.row_iterator(&mut row_storage)?;
    while let Some((y, row)) = rows.next_dirty() {
        write!(output, "Row {y:2} [dirty]: ")?;
        if let Some(selection) = row.selection()? {
            write!(
                output,
                "selection={}..{} ",
                selection.start_x, selection.end_x
            )?;
        }
        let mut cells = row.cells(&mut cell_storage)?;
        while let Some(cell) = cells.next() {
            if cell.graphemes_len()? == 0 {
                write!(output, " ")?;
                continue;
            }
            let style = cell.style()?;
            let fg = cell.foreground()?.unwrap_or(colors.foreground);
            write!(output, "\x1b[38;2;{};{};{}m", fg.r, fg.g, fg.b)?;
            if style.flags.contains(StyleFlags::BOLD) {
                write!(output, "\x1b[1m")?;
            }
            if style.underline != Underline::None {
                write!(output, "\x1b[4m")?;
            }
            let mut codepoints = [0; 16];
            let len = cell.graphemes_buf(&mut codepoints)?.min(codepoints.len());
            for codepoint in &codepoints[..len] {
                if let Some(character) = char::from_u32(*codepoint) {
                    write!(output, "{character}")?;
                }
            }
            write!(output, "\x1b[0m")?;
        }
        writeln!(output)?;
    }
    output.flush()?;
    state.clean()?;
    Ok(())
}
