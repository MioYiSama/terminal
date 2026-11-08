use std::fs::File;
use std::io::Read;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pty::{Command, PtySize};

const SIZE: PtySize = PtySize {
    rows: 24,
    cols: 80,
    pixel_width: 640,
    pixel_height: 384,
};

/// The pty's output, drained on a background thread so a test can wait for
/// text without blocking forever.
struct Output {
    rx: mpsc::Receiver<Vec<u8>>,
    seen: Vec<u8>,
    matched: usize,
}

impl Output {
    fn new(mut reader: File) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            rx,
            seen: Vec::new(),
            matched: 0,
        }
    }

    /// Wait until `needle` appears after the previous match, returning the
    /// output in between.
    fn until(&mut self, needle: &str) -> String {
        loop {
            let text = String::from_utf8_lossy(&self.seen);
            if let Some(end) = text[self.matched..].find(needle) {
                let start = self.matched;
                self.matched += end + needle.len();
                return text[start..self.matched].to_owned();
            }
            match self.rx.recv_timeout(Duration::from_secs(10)) {
                Ok(bytes) => self.seen.extend(bytes),
                Err(_) => panic!("{needle:?} not in output: {text:?}"),
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn child_sees_tty_env_size_and_resize() {
    use std::io::Write;

    // Non-interactive on purpose: readline re-applies the size it read
    // during prompt setup, which races with a resize.
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg("test -t 0 && echo tty-$PTY_TEST; echo size-$(stty size); read _; echo size-$(stty size); exit 3")
        .env("PTY_TEST", "env-ok");
    let (master, mut child) = pty::spawn(&cmd, SIZE).unwrap();
    let mut output = Output::new(master.reader().unwrap());

    let out = output.until("size-24 80");
    assert!(out.contains("tty-env-ok"), "{out:?}");

    master
        .resize(PtySize {
            rows: 40,
            cols: 100,
            ..SIZE
        })
        .unwrap();
    master.writer().unwrap().write_all(b"\n").unwrap();
    output.until("size-40 100");
    assert_eq!(child.wait().unwrap().code(), Some(3));
}

#[cfg(unix)]
#[test]
fn killer_hangs_up_waiting_child() {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg("echo ready; exec sleep 30");
    let (master, mut child) = pty::spawn(&cmd, SIZE).unwrap();
    Output::new(master.reader().unwrap()).until("ready");

    let killer = child.killer();
    let waiter = thread::spawn(move || child.wait().unwrap());
    killer.kill().unwrap();
    let status = waiter.join().unwrap();
    // SIGHUP is 1 on every Unix.
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&status),
        Some(1)
    );
    // Killing after the child was reaped must not signal anything.
    killer.kill().unwrap();
}

#[cfg(windows)]
#[test]
fn child_echoes_env_and_exit_code() {
    let mut cmd = Command::new("cmd.exe");
    cmd.arg("/c").arg("echo tty-%PTY_TEST% & exit /b 3");
    cmd.env("PTY_TEST", "env-ok");
    let (master, mut child) = pty::spawn(&cmd, SIZE).unwrap();
    Output::new(master.reader().unwrap()).until("tty-env-ok");
    assert_eq!(child.wait().unwrap().code(), Some(3));
}
