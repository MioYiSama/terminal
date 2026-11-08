//! Feed styled VT bytes to a terminal and print its plain-text screen.

use ghostty::{formatter::Formatter, terminal::Terminal};
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(80, 24)?;
    terminal.vt_write(b"Hello, \x1b[1;32mworld\x1b[0m!\r\n");
    let formatter = Formatter::new(&terminal)?;
    let mut stdout = io::stdout().lock();
    formatter.format(&mut stdout)?;
    stdout.write_all(b"\n")?;
    Ok(())
}
