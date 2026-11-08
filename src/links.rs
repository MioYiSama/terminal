//! Links under the pointer: OSC 8 hyperlinks, and URLs and paths of existing
//! files written as plain text.

use std::fmt::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::vt::{Frame, Terminal};

/// A link on screen and the cells showing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub url: String,
    /// Viewport `(row, columns)` spans, top to bottom.
    pub spans: Vec<(u16, Range<u16>)>,
}

/// The link at viewport cell `(col, row)`: an OSC 8 hyperlink, else, given
/// `text_links`, a URL or the path of an existing file in the text, which may
/// be soft-wrapped over several rows. `text_links` supplies the directory
/// relative paths are resolved against.
pub fn link_at(
    terminal: &Terminal,
    frame: &Frame,
    col: u16,
    row: u16,
    text_links: Option<impl FnOnce() -> Option<PathBuf>>,
) -> Option<Link> {
    let line = LogicalLine::around(frame, row)?;
    // The pointer may be on a wide character's spacer half.
    let hovered = line
        .cells
        .iter()
        .position(|&(r, c, width, _)| r == row && (c..c + width).contains(&col))?;
    let (_, col, _, _) = line.cells[hovered];

    if let Some(url) = terminal.hyperlink_at(col, row) {
        let same = |i: &usize| {
            let (r, c, _, _) = line.cells[*i];
            terminal.hyperlink_at(c, r).as_deref() == Some(url.as_str())
        };
        let start = (0..hovered).rev().take_while(same).last().unwrap_or(hovered);
        let end = (hovered + 1..line.cells.len()).take_while(same).last().unwrap_or(hovered);
        return Some(Link {
            url,
            spans: line.spans(start..end + 1),
        });
    }

    let cwd = text_links?;
    let (text, offsets) = line.text();
    let at = offsets[hovered];
    let (range, url) = match url_at(&text, at) {
        Some(range) => (range.clone(), text[range].to_owned()),
        None => {
            let range = path_at(&text, at)?;
            let path = resolve(strip_location(&text[range.clone()]), cwd)?;
            (range, file_url(&path)?)
        }
    };
    let first = offsets.partition_point(|&offset| offset < range.start);
    let last = offsets.partition_point(|&offset| offset < range.end);
    Some(Link {
        url,
        spans: line.spans(first..last),
    })
}

/// Rows joined by soft wraps, as `(row, col, width, text)` cells;
/// wide-character spacers are left out.
struct LogicalLine<'a> {
    cells: Vec<(u16, u16, u16, &'a str)>,
}

impl<'a> LogicalLine<'a> {
    fn around(frame: &'a Frame, row: u16) -> Option<Self> {
        let lines = &frame.lines;
        let row = row as usize;
        lines.get(row)?;
        let mut first = row;
        while first > 0 && lines[first - 1].wrapped {
            first -= 1;
        }
        let mut last = row;
        while last + 1 < lines.len() && lines[last].wrapped {
            last += 1;
        }
        let mut cells = Vec::new();
        for (r, line) in lines.iter().enumerate().take(last + 1).skip(first) {
            for (c, cell) in line.cells.iter().enumerate() {
                let spacer = c > 0 && line.cells[c - 1].wide;
                if !spacer {
                    let text = &line.text[cell.text.start as usize..cell.text.end as usize];
                    cells.push((r as u16, c as u16, if cell.wide { 2 } else { 1 }, text));
                }
            }
        }
        Some(Self { cells })
    }

    /// The text, blanks as spaces, and each cell's byte offset into it.
    fn text(&self) -> (String, Vec<usize>) {
        let mut text = String::new();
        let mut offsets = Vec::with_capacity(self.cells.len());
        for &(_, _, _, cell) in &self.cells {
            offsets.push(text.len());
            text.push_str(if cell.is_empty() { " " } else { cell });
        }
        (text, offsets)
    }

    /// Per-row column spans covering `cells`.
    fn spans(&self, cells: Range<usize>) -> Vec<(u16, Range<u16>)> {
        let mut spans: Vec<(u16, Range<u16>)> = Vec::new();
        for &(row, col, width, _) in &self.cells[cells] {
            match spans.last_mut() {
                Some((r, cols)) if *r == row => cols.end = col + width,
                _ => spans.push((row, col..col + width)),
            }
        }
        spans
    }
}

const SCHEMES: [&str; 5] = ["https://", "http://", "file://", "ftp://", "mailto:"];

/// The byte range of the URL in `text` covering byte `at`, if any.
fn url_at(text: &str, at: usize) -> Option<Range<usize>> {
    let mut search = 0;
    while let Some((start, scheme)) = SCHEMES
        .iter()
        .filter_map(|scheme| Some((search + text[search..].find(scheme)?, *scheme)))
        .min_by_key(|&(start, _)| start)
    {
        let range = start..url_end(text, start, scheme);
        let word_start = !text[..start].chars().next_back().is_some_and(char::is_alphanumeric);
        if word_start && range.len() > scheme.len() && range.contains(&at) {
            return Some(range);
        }
        if start > at {
            return None;
        }
        search = range.end.max(start + scheme.len());
    }
    None
}

/// Where the URL starting at `start` ends: at whitespace or a character
/// URLs do not contain, minus trailing punctuation and unbalanced closing
/// brackets, which belong to the surrounding prose.
fn url_end(text: &str, start: usize, scheme: &str) -> usize {
    let body = start + scheme.len();
    let mut end = text[body..]
        .find(|c: char| c.is_whitespace() || c.is_control() || "<>\"`{}|\\^".contains(c))
        .map_or(text.len(), |len| body + len);
    loop {
        let url = &text[start..end];
        let Some(last) = url.chars().next_back() else { break };
        let unbalanced = |open, close| {
            last == close && url.matches(close).count() > url.matches(open).count()
        };
        if end > body && (".,:;!?'*".contains(last) || unbalanced('(', ')') || unbalanced('[', ']')) {
            end -= last.len_utf8();
        } else {
            break;
        }
    }
    end
}

/// The byte range of the path-like word in `text` covering byte `at`: one
/// with a `/` (or `\` on Windows) in it, without the quotes, brackets, and
/// punctuation of the surrounding prose. It may end in a `:line` or
/// `:line:column` location.
fn path_at(text: &str, at: usize) -> Option<Range<usize>> {
    let delimiter =
        |c: char| c.is_whitespace() || c.is_control() || "\"'`<>()[]{}|,;，。；：（）「」【】、".contains(c);
    if text[at..].starts_with(delimiter) {
        return None;
    }
    let start = text[..at]
        .char_indices()
        .rev()
        .find(|&(_, c)| delimiter(c))
        .map_or(0, |(i, c)| i + c.len_utf8());
    let end = text[at..].find(delimiter).map_or(text.len(), |len| at + len);
    let word = text[start..end].trim_end_matches(['.', ':', '!', '?']);
    let separator = |c: char| c == '/' || (cfg!(windows) && c == '\\');
    (start + word.len() > at && word.contains(separator)).then_some(start..start + word.len())
}

/// `path` without a trailing `:line` or `:line:column`.
fn strip_location(mut path: &str) -> &str {
    for _ in 0..2 {
        match path.rsplit_once(':') {
            Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => {
                path = head
            }
            _ => break,
        }
    }
    path
}

/// The existing file or directory `path` names, with `~/` meaning the home
/// directory and relative paths taken from `cwd`.
fn resolve(path: &str, cwd: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    let path = match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()?.join(rest),
        None => PathBuf::from(path),
    };
    let path = if path.is_absolute() { path } else { cwd()?.join(path) };
    path.exists().then_some(path)
}

/// The `file://` URL of an absolute path.
fn file_url(path: &Path) -> Option<String> {
    let path = path.to_str()?;
    // Windows drive paths become `file:///C:/…`.
    let mut url = String::from(if cfg!(windows) { "file:///" } else { "file://" });
    for byte in path.bytes() {
        match byte {
            b'\\' if cfg!(windows) => url.push('/'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'.' | b'_' | b'~' | b':' => {
                url.push(byte as char)
            }
            _ => {
                let _ = write!(url, "%{byte:02X}");
            }
        }
    }
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str, at: &str) -> Option<String> {
        let at = text.find(at).unwrap();
        url_at(text, at).map(|range| text[range].to_owned())
    }

    fn path<'a>(text: &'a str, at: &str) -> Option<&'a str> {
        let at = text.find(at).unwrap();
        path_at(text, at).map(|range| &text[range])
    }

    #[test]
    fn paths_keep_their_location_but_not_prose_punctuation() {
        assert_eq!(path("see src/view.rs:42:7.", "view"), Some("src/view.rs:42:7"));
        assert_eq!(path("(~/notes/a.md), ok", "notes"), Some("~/notes/a.md"));
        assert_eq!(path("改了 `src/main.rs`。", "main"), Some("src/main.rs"));
        assert_eq!(path("ls /tmp/x", "ls"), None);
        assert_eq!(path("2/3 done.", "done"), None);
        assert_eq!(strip_location("src/a.rs:12:3"), "src/a.rs");
        assert_eq!(strip_location("src/a.rs:12"), "src/a.rs");
        assert_eq!(strip_location("src/a:b.rs"), "src/a:b.rs");
    }

    #[test]
    fn resolves_only_existing_paths() {
        let dir = std::env::temp_dir().join(format!("terminal-links-{}", std::process::id()));
        let file = dir.join("src").join("文件.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
        assert_eq!(resolve("src/文件.rs", || Some(dir.clone())), Some(file.clone()));
        assert_eq!(resolve("src/missing.rs", || Some(dir.clone())), None);
        assert_eq!(resolve("src/文件.rs", || None), None);
        assert_eq!(resolve(file.to_str().unwrap(), || None), Some(file.clone()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn file_urls_are_percent_encoded() {
        assert_eq!(
            file_url(Path::new("/tmp/a b/文.rs")).as_deref(),
            Some("file:///tmp/a%20b/%E6%96%87.rs")
        );
    }

    #[test]
    fn stops_at_prose_punctuation_and_unbalanced_brackets() {
        assert_eq!(url("see https://a.com/x.", "a.com").as_deref(), Some("https://a.com/x"));
        assert_eq!(url("(https://a.com/x)", "a.com").as_deref(), Some("https://a.com/x"));
        assert_eq!(
            url("https://en.wikipedia.org/wiki/Rust_(language), ok", "wiki").as_deref(),
            Some("https://en.wikipedia.org/wiki/Rust_(language)")
        );
        assert_eq!(url("<https://a.com/?q=1&b=2>", "q=").as_deref(), Some("https://a.com/?q=1&b=2"));
    }

    #[test]
    fn finds_the_url_under_the_offset_only() {
        let text = "https://a.com and http://b.org/p";
        assert_eq!(url(text, "b.org").as_deref(), Some("http://b.org/p"));
        assert_eq!(url(text, "and"), None);
        assert_eq!(url("xhttps://a.com", "a.com"), None);
        assert_eq!(url("https:// nothing", "https"), None);
    }
}
