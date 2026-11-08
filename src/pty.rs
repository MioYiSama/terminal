//! Cross-platform pseudo terminal (Unix pty / Windows ConPTY) running the
//! user's shell. I/O happens on background threads so the UI never blocks on
//! the child process.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use pty::{Command, Killer, Master, PtySize};

pub enum PtyEvent {
    Output(Vec<u8>),
    Exited,
}

pub struct Pty {
    master: Master,
    input: mpsc::Sender<Vec<u8>>,
    killer: Killer,
}

fn pty_size(cols: u16, rows: u16, cell_width: u16, cell_height: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: cols.saturating_mul(cell_width),
        pixel_height: rows.saturating_mul(cell_height),
    }
}

fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
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
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", env!("CARGO_PKG_NAME"));
        // PowerShell 7.6.6/7.6.7 crash on startup (in Windows Terminal too)
        // while printing the "new version available" banner under non-English
        // UI cultures such as zh-Hans and ja-JP: the localized message yields
        // a negative `PadRight` width (PowerShell/PowerShell#28131). Skip the
        // update check unless the user configured it.
        if cfg!(windows) && std::env::var_os("POWERSHELL_UPDATECHECK").is_none() {
            cmd.env("POWERSHELL_UPDATECHECK", "Off");
        }
        if let Some(home) = home_dir() {
            cmd.cwd(home);
        }

        let (master, mut child) = pty::spawn(&cmd, pty_size(cols, rows, cell_width, cell_height))
            .map_err(|e| format!("spawn shell: {e}"))?;

        let killer = child.killer();
        let mut reader = master.reader().map_err(|e| format!("pty reader: {e}"))?;
        let mut writer = master.writer().map_err(|e| format!("pty writer: {e}"))?;

        let (events_tx, events_rx) = unbounded::<PtyEvent>();

        let output_tx: UnboundedSender<PtyEvent> = events_tx.clone();
        thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if output_tx
                                .unbounded_send(PtyEvent::Output(buf[..n].to_vec()))
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
                let _ = output_tx.unbounded_send(PtyEvent::Exited);
            })
            .map_err(|e| format!("spawn reader thread: {e}"))?;

        // ConPTY keeps the output pipe open after the shell exits, so exit is
        // detected by waiting on the child rather than by reader EOF alone.
        thread::Builder::new()
            .name("pty-waiter".into())
            .spawn(move || {
                let _ = child.wait();
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

        Ok((
            Self {
                master,
                input: input_tx,
                killer,
            },
            events_rx,
        ))
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
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}
