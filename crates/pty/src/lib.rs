//! Minimal pseudo terminal: a Unix pty from `openpty(3)` or a Windows
//! ConPTY, with a child process attached to it as its controlling terminal.

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

#[cfg_attr(unix, path = "unix.rs")]
#[cfg_attr(windows, path = "windows.rs")]
mod sys;

/// Terminal dimensions in cells and pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

/// What to run inside the pty. The child inherits the current environment
/// minus `env_remove` names, with `env` entries layered on top.
#[derive(Clone, Debug)]
pub struct Command {
    /// `None` runs the user's default shell (a login shell on Unix).
    program: Option<OsString>,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    env_remove: Vec<OsString>,
    cwd: Option<PathBuf>,
}

impl Command {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self::with_program(Some(program.into()))
    }

    /// The user's default shell: `$SHELL`, the passwd entry, or `/bin/sh` on
    /// Unix; on Windows the first of `pwsh.exe` and `powershell.exe` on
    /// `PATH`, else `%ComSpec%` or `cmd.exe`.
    pub fn default_shell() -> Self {
        Self::with_program(None)
    }

    fn with_program(program: Option<OsString>) -> Self {
        Self {
            program,
            args: Vec::new(),
            env: Vec::new(),
            env_remove: Vec::new(),
            cwd: None,
        }
    }

    pub fn arg(&mut self, arg: impl Into<OsString>) -> &mut Self {
        self.args.push(arg.into());
        self
    }

    pub fn env(&mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> &mut Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Do not pass the inherited variable `key` on.
    pub fn env_remove(&mut self, key: impl Into<OsString>) -> &mut Self {
        self.env_remove.push(key.into());
        self
    }

    pub fn cwd(&mut self, dir: impl Into<PathBuf>) -> &mut Self {
        self.cwd = Some(dir.into());
        self
    }
}

/// Open a pty of the given size and start `cmd` attached to it.
pub fn spawn(cmd: &Command, size: PtySize) -> io::Result<(Master, Child)> {
    let (master, child) = sys::spawn(cmd, size)?;
    Ok((Master(master), Child(child)))
}

/// The controlling side of the pty. Dropping it hangs up the terminal.
pub struct Master(sys::Master);

impl Master {
    /// A handle that reads the child's output.
    pub fn reader(&self) -> io::Result<File> {
        self.0.reader()
    }

    /// A handle that writes the child's input.
    pub fn writer(&self) -> io::Result<File> {
        self.0.writer()
    }

    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        self.0.resize(size)
    }

    /// The working directory of the process in the foreground of the
    /// terminal (the shell, or the program it is running), where known.
    /// Always `None` on Windows.
    pub fn foreground_cwd(&self) -> Option<PathBuf> {
        self.0.foreground_cwd()
    }
}

/// The process running inside the pty.
pub struct Child(sys::Child);

impl Child {
    /// Block until the child exits.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.0.wait()
    }

    /// A handle that can terminate the child while another thread waits.
    pub fn killer(&self) -> Killer {
        Killer(self.0.killer())
    }
}

#[derive(Clone)]
pub struct Killer(sys::Killer);

impl Killer {
    /// Terminate the child (SIGHUP on Unix). A no-op once it has exited.
    pub fn kill(&self) -> io::Result<()> {
        self.0.kill()
    }
}
