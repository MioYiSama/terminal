//! Cross-platform pseudo terminal (Unix pty / Windows ConPTY) running the
//! user's shell. I/O happens on background threads so the UI never blocks on
//! the child process.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub enum PtyEvent {
    Output(Vec<u8>),
    Exited,
}

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    input: mpsc::Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
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
    /// Spawn the user's default shell. Output and exit notifications arrive
    /// on the returned channel.
    pub fn spawn(
        cols: u16,
        rows: u16,
        cell_width: u16,
        cell_height: u16,
    ) -> Result<(Self, UnboundedReceiver<PtyEvent>), String> {
        let pair = native_pty_system()
            .openpty(pty_size(cols, rows, cell_width, cell_height))
            .map_err(|e| format!("openpty: {e}"))?;

        let mut cmd = CommandBuilder::new_default_prog();
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", env!("CARGO_PKG_NAME"));
        if let Some(home) = home_dir() {
            cmd.cwd(home);
        }

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn shell: {e}"))?;
        // The child holds its own handle; keeping ours open would stop the
        // reader from seeing EOF when the shell exits.
        drop(pair.slave);

        let killer = child.clone_killer();
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("pty reader: {e}"))?;
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("pty writer: {e}"))?;

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
                master: pair.master,
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
