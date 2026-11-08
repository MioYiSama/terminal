//! Quoting file paths for the shell a terminal runs, so pasted paths reach
//! the program as one literal argument each.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The quoting rules of a shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
    /// sh, bash, zsh, fish, …
    Posix,
    /// Windows PowerShell and PowerShell 7.
    PowerShell,
    Cmd,
}

impl ShellKind {
    /// The kind of shell `program` (a name or path) is, by its file name.
    pub fn of(program: &OsStr) -> Self {
        let name = Path::new(program)
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_ascii_lowercase());
        match name.as_deref() {
            Some("pwsh" | "powershell") => Self::PowerShell,
            Some("cmd") => Self::Cmd,
            _ => Self::Posix,
        }
    }

    /// `paths` quoted for this shell, separated by spaces.
    pub fn quote_paths(self, paths: &[PathBuf]) -> String {
        let quoted: Vec<String> = paths.iter().map(|path| self.quote(path)).collect();
        quoted.join(" ")
    }

    fn quote(self, path: &Path) -> String {
        let path = path.to_string_lossy();
        match self {
            Self::Posix => posix(&path),
            Self::PowerShell => powershell(&path),
            Self::Cmd => cmd(&path),
        }
    }
}

/// Backslash-escape what POSIX shells, zsh and fish treat specially, as
/// macOS terminals do for dropped files. Newlines stay as they are: an
/// escaped newline would be a line continuation.
fn posix(path: &str) -> String {
    const SPECIAL: &str = " \t\\'\"`$*?|&;()<>[]{}!#~";
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if SPECIAL.contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Single-quote paths PowerShell would otherwise split or expand. Inside
/// single quotes nothing expands (`$`, backticks, `$(…)` stay literal); a
/// quote character, which includes the typographic single quotes, is
/// escaped by doubling it.
fn powershell(path: &str) -> String {
    const SPECIAL: &str = " \t&()[]{}^=;!'+,`~$@#%|<>\"\u{2018}\u{2019}\u{201A}\u{201B}";
    if !path.contains(|c| SPECIAL.contains(c)) {
        return path.to_owned();
    }
    let mut out = String::with_capacity(path.len() + 2);
    out.push('\'');
    for c in path.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// Double-quote paths with spaces or cmd.exe punctuation; Windows file
/// names cannot contain `"`. cmd.exe has no quoting that stops `%NAME%`
/// expansion on its command line, so such a name expands if the variable
/// exists.
fn cmd(path: &str) -> String {
    const SPECIAL: &str = " \t&()[]{}^=;!'+,`~$@#%|<>";
    if path.contains(|c| SPECIAL.contains(c)) {
        format!("\"{path}\"")
    } else {
        path.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_shell_by_file_name() {
        assert_eq!(ShellKind::of(OsStr::new("pwsh.exe")), ShellKind::PowerShell);
        assert_eq!(
            ShellKind::of(OsStr::new("PowerShell.EXE")),
            ShellKind::PowerShell
        );
        assert_eq!(ShellKind::of(OsStr::new("cmd.exe")), ShellKind::Cmd);
        assert_eq!(ShellKind::of(OsStr::new("/bin/zsh")), ShellKind::Posix);
    }

    #[test]
    fn powershell_keeps_expansions_literal() {
        let quote = |path: &str| ShellKind::PowerShell.quote(Path::new(path));
        assert_eq!(quote(r"C:\tmp\a.png"), r"C:\tmp\a.png");
        assert_eq!(
            quote(r"C:\tmp\$(Get-Date).png"),
            r"'C:\tmp\$(Get-Date).png'"
        );
        assert_eq!(quote(r"C:\it's here"), r"'C:\it''s here'");
        assert_eq!(quote("C:\\a\u{2019}b c"), "'C:\\a\u{2019}\u{2019}b c'");
    }

    #[test]
    fn posix_escapes_metacharacters_but_not_newlines() {
        let quote = |path: &str| ShellKind::Posix.quote(Path::new(path));
        assert_eq!(
            quote("/tmp/My Shot (1)/it's $HOME*.png"),
            r"/tmp/My\ Shot\ \(1\)/it\'s\ \$HOME\*.png"
        );
        assert_eq!(quote("/a\nb"), "/a\nb");
    }
}
