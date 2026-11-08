//! Cross-platform pseudo terminal (Unix pty / Windows ConPTY) running the
//! user's shell. I/O happens on background threads so the UI never blocks on
//! the child process.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use pty::{Command, Killer, Master, PtySize};

use crate::shell::ShellKind;

/// How long output may still arrive after the shell exits. The reader is
/// usually done at once (Unix hangs up when the last process holding the
/// pty exits); ConPTY keeps its pipe open, and background jobs can keep a
/// Unix pty open, so the wait is bounded.
const EXIT_DRAIN_TIMEOUT: Duration = Duration::from_millis(200);

pub enum PtyEvent {
    Output(Vec<u8>),
    /// The shell exited (or its output ended). Sent once, after all output
    /// that arrived in time.
    Exited,
}

pub struct Pty {
    master: Master,
    input: mpsc::Sender<Vec<u8>>,
    killer: Killer,
    shell: ShellKind,
}

/// Kills the child unless disarmed, so a failed start leaves no process.
struct KillOnDrop(Option<Killer>);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some(killer) = &self.0 {
            let _ = killer.kill();
        }
    }
}

fn pty_size(cols: u16, rows: u16, cell_width: u16, cell_height: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: cols.saturating_mul(cell_width),
        pixel_height: rows.saturating_mul(cell_height),
    }
}

/// Whether the inherited variable `name` describes the terminal or multiplexer
/// this app was launched from, e.g. a terminal pane in an editor. Programs
/// detect their terminal from these before `TERM_PROGRAM`, so passed on they
/// would mistake this terminal for that one.
fn describes_another_terminal(name: &str) -> bool {
    const PREFIXES: [&str; 10] = [
        "ALACRITTY_",
        "GHOSTTY_",
        "ITERM_",
        "KITTY_",
        "KONSOLE_",
        "VSCODE_",
        "WEZTERM_",
        "WT_",
        "ZED_",
        "ZELLIJ",
    ];
    const NAMES: [&str; 11] = [
        "COLORFGBG",
        "LC_TERMINAL",
        "LC_TERMINAL_VERSION",
        "STY",
        "TERMINAL_EMULATOR",
        "TERM_FEATURES",
        "TERM_PROGRAM_VERSION",
        "TERM_SESSION_ID",
        "TMUX",
        "TMUX_PANE",
        "VTE_VERSION",
    ];
    PREFIXES.iter().any(|prefix| name.starts_with(prefix)) || NAMES.contains(&name)
}

impl Pty {
    /// Spawn `program`, or the user's default shell if `None`. Output and
    /// exit notifications arrive on the returned channel.
    pub fn spawn(
        program: Option<&str>,
        cols: u16,
        rows: u16,
        cell_width: u16,
        cell_height: u16,
    ) -> Result<(Self, UnboundedReceiver<PtyEvent>), String> {
        let mut cmd = program.map_or_else(Command::default_shell, Command::new);
        for (name, _) in std::env::vars_os() {
            if name.to_str().is_some_and(describes_another_terminal) {
                cmd.env_remove(name);
            }
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        // The terminal is libghostty, so programs that pick features by
        // terminal (Kitty graphics with Unicode placeholders, OSC 8, …) get
        // them by recognizing Ghostty; without a known name they assume an
        // xterm with none of them.
        cmd.env("TERM_PROGRAM", "ghostty");
        // PowerShell 7.6.6/7.6.7 crash on startup (in Windows Terminal too)
        // while printing the "new version available" banner under non-English
        // UI cultures such as zh-Hans and ja-JP: the localized message yields
        // a negative `PadRight` width (PowerShell/PowerShell#28131). Skip the
        // update check unless the user configured it.
        if cfg!(windows) && std::env::var_os("POWERSHELL_UPDATECHECK").is_none() {
            cmd.env("POWERSHELL_UPDATECHECK", "Off");
        }
        if let Some(home) = dirs::home_dir().filter(|home| home.is_dir()) {
            cmd.cwd(home);
        }

        let shell = ShellKind::of(&cmd.program());
        let (master, mut child) = pty::spawn(&cmd, pty_size(cols, rows, cell_width, cell_height))
            .map_err(|e| format!("spawn shell: {e}"))?;
        let killer = child.killer();
        let mut guard = KillOnDrop(Some(killer.clone()));

        let mut reader = master.reader().map_err(|e| format!("pty reader: {e}"))?;
        let mut writer = master.writer().map_err(|e| format!("pty writer: {e}"))?;
        let (events_tx, events_rx) = unbounded::<PtyEvent>();
        let (reader_done_tx, reader_done_rx) = mpsc::channel::<()>();

        // Output, then `Exited` at EOF. Either this or the waiter reports
        // the exit; the consumer stops at the first.
        let output_tx: UnboundedSender<PtyEvent> = events_tx.clone();
        thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let n = match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    };
                    if output_tx
                        .unbounded_send(PtyEvent::Output(buf[..n].to_vec()))
                        .is_err()
                    {
                        return;
                    }
                }
                let _ = reader_done_tx.send(());
                let _ = output_tx.unbounded_send(PtyEvent::Exited);
            })
            .map_err(|e| format!("spawn reader thread: {e}"))?;

        // ConPTY keeps the output pipe open after the shell exits, so exit is
        // detected by waiting on the child. Output the shell wrote before
        // exiting may still be in flight: let the reader drain it first, so
        // `Exited` comes after it.
        thread::Builder::new()
            .name("pty-waiter".into())
            .spawn(move || {
                let _ = child.wait();
                let _ = reader_done_rx.recv_timeout(EXIT_DRAIN_TIMEOUT);
                let _ = events_tx.unbounded_send(PtyEvent::Exited);
            })
            .map_err(|e| format!("spawn waiter thread: {e}"))?;

        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
        thread::Builder::new()
            .name("pty-writer".into())
            .spawn(move || {
                for bytes in input_rx {
                    if writer.write_all(&bytes).and_then(|()| writer.flush()).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("spawn writer thread: {e}"))?;

        guard.0 = None;
        Ok((
            Self {
                master,
                input: input_tx,
                killer,
                shell,
            },
            events_rx,
        ))
    }

    /// Quoting rules of the shell running here.
    pub fn shell(&self) -> ShellKind {
        self.shell
    }

    pub fn write(&self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            let _ = self.input.send(bytes);
        }
    }

    pub fn resize(&self, cols: u16, rows: u16, cell_width: u16, cell_height: u16) {
        let _ = self
            .master
            .resize(pty_size(cols, rows, cell_width, cell_height));
    }

    /// Where relative paths printed by the foreground program point.
    pub fn foreground_cwd(&self) -> Option<PathBuf> {
        self.master.foreground_cwd()
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}
