use std::ffi::{CStr, OsStr, OsString};
use std::fs::File;
use std::io;
use std::mem::MaybeUninit;
use std::path::PathBuf;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::process::CommandExt;
use std::process::{self, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::{env, ptr};

use crate::{Command, PtySize};

fn cvt(ret: libc::c_int) -> io::Result<libc::c_int> {
    if ret == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

fn winsize(size: PtySize) -> libc::winsize {
    libc::winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: size.pixel_width,
        ws_ypixel: size.pixel_height,
    }
}

fn set_cloexec(fd: RawFd) -> io::Result<()> {
    let flags = cvt(unsafe { libc::fcntl(fd, libc::F_GETFD) })?;
    cvt(unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) })?;
    Ok(())
}

pub struct Master {
    fd: File,
}

impl Master {
    pub fn reader(&self) -> io::Result<File> {
        self.fd.try_clone()
    }

    pub fn writer(&self) -> io::Result<File> {
        self.fd.try_clone()
    }

    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        let ws = winsize(size);
        cvt(unsafe { libc::ioctl(self.fd.as_raw_fd(), libc::TIOCSWINSZ as _, &ws) })?;
        Ok(())
    }

    pub fn foreground_cwd(&self) -> Option<PathBuf> {
        // The foreground process group's id is its leader's pid.
        let leader = unsafe { libc::tcgetpgrp(self.fd.as_raw_fd()) };
        if leader <= 0 {
            return None;
        }
        process_cwd(leader)
    }
}

#[cfg(target_os = "macos")]
fn process_cwd(pid: libc::pid_t) -> Option<PathBuf> {
    let mut info = MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    let size = size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(pid, libc::PROC_PIDVNODEPATHINFO, 0, info.as_mut_ptr().cast(), size)
    };
    if written != size {
        return None;
    }
    let info = unsafe { info.assume_init() };
    // `vip_path` is a NUL-terminated `[c_char; MAXPATHLEN]`.
    let path = info.pvi_cdir.vip_path.as_flattened();
    let path = unsafe { std::slice::from_raw_parts(path.as_ptr().cast::<u8>(), path.len()) };
    let path = CStr::from_bytes_until_nul(path).ok()?;
    Some(PathBuf::from(OsStr::from_bytes(path.to_bytes())))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_cwd(pid: libc::pid_t) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "android")))]
fn process_cwd(_pid: libc::pid_t) -> Option<PathBuf> {
    None
}

pub fn spawn(cmd: &Command, size: PtySize) -> io::Result<(Master, Child)> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let mut ws = winsize(size);
    cvt(unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            ptr::null_mut(),
            ptr::null_mut(),
            // `*mut` on Apple, `*const` elsewhere.
            &raw mut ws,
        )
    })?;
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    // Only the child's stdio copies of the slave may survive exec.
    set_cloexec(master.as_raw_fd())?;
    set_cloexec(slave.as_raw_fd())?;

    let child = spawn_child(cmd, File::from(slave))?;
    let reaped = Arc::new(Mutex::new(false));
    Ok((
        Master {
            fd: File::from(master),
        },
        Child {
            inner: child,
            reaped,
        },
    ))
}

/// Consumes `slave` so every parent-side copy is closed once the child is
/// running; otherwise the master would never see the hangup when it exits.
fn spawn_child(cmd: &Command, slave: File) -> io::Result<process::Child> {
    let mut command = match &cmd.program {
        Some(program) => process::Command::new(program),
        None => {
            let shell = default_shell();
            let mut command = process::Command::new(&shell);
            // A leading '-' in argv[0] asks the shell to run as a login shell.
            let name = shell
                .as_bytes()
                .rsplit(|&b| b == b'/')
                .next()
                .unwrap_or_default();
            let mut arg0 = OsString::from("-");
            arg0.push(OsStr::from_bytes(name));
            command.arg0(arg0);
            command
        }
    };
    for key in &cmd.env_remove {
        command.env_remove(key);
    }
    command
        .args(&cmd.args)
        .envs(cmd.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::from(slave.try_clone()?))
        .stdout(Stdio::from(slave.try_clone()?))
        .stderr(Stdio::from(slave));
    if let Some(cwd) = &cmd.cwd {
        command.current_dir(cwd);
    }
    unsafe {
        command.pre_exec(|| {
            // Exec keeps ignored signals ignored; give the shell a clean slate.
            for signal in [
                libc::SIGCHLD,
                libc::SIGHUP,
                libc::SIGINT,
                libc::SIGQUIT,
                libc::SIGTERM,
                libc::SIGALRM,
            ] {
                libc::signal(signal, libc::SIG_DFL);
            }
            // New session, with the pty (already on fd 0) as its controlling
            // terminal so job control and SIGHUP work.
            cvt(libc::setsid())?;
            cvt(libc::ioctl(0, libc::TIOCSCTTY as _, 0))?;
            Ok(())
        });
    }
    command.spawn()
}

pub(crate) fn default_shell() -> OsString {
    env::var_os("SHELL")
        .filter(|shell| !shell.is_empty())
        .or_else(passwd_shell)
        .unwrap_or_else(|| "/bin/sh".into())
}

fn passwd_shell() -> Option<OsString> {
    let mut buf = vec![0u8; 1024];
    loop {
        let mut pwd = MaybeUninit::<libc::passwd>::uninit();
        let mut result = ptr::null_mut();
        let err = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                pwd.as_mut_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut result,
            )
        };
        if err == libc::ERANGE {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        if err != 0 || result.is_null() {
            return None;
        }
        let shell = unsafe { (*result).pw_shell };
        if shell.is_null() {
            return None;
        }
        let shell = unsafe { CStr::from_ptr(shell) }.to_bytes();
        return (!shell.is_empty()).then(|| OsString::from_vec(shell.to_vec()));
    }
}

/// Whether the child has been reaped. Its pid may be recycled afterwards, so
/// signalling and reaping are serialised through this lock.
type Reaped = Arc<Mutex<bool>>;

fn lock(reaped: &Reaped) -> MutexGuard<'_, bool> {
    reaped.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct Child {
    inner: process::Child,
    reaped: Reaped,
}

impl Child {
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        if !*lock(&self.reaped) {
            // Block without reaping: the zombie keeps the pid reserved, so a
            // concurrent kill cannot hit an unrelated process.
            let mut info = MaybeUninit::<libc::siginfo_t>::zeroed();
            while unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.inner.id() as libc::id_t,
                    info.as_mut_ptr(),
                    libc::WEXITED | libc::WNOWAIT,
                )
            } == -1
            {
                let err = io::Error::last_os_error();
                if err.kind() != io::ErrorKind::Interrupted {
                    return Err(err);
                }
            }
        }
        let mut reaped = lock(&self.reaped);
        let status = self.inner.wait()?;
        *reaped = true;
        Ok(status)
    }

    pub fn killer(&self) -> Killer {
        Killer {
            pid: self.inner.id() as libc::pid_t,
            reaped: self.reaped.clone(),
        }
    }
}

#[derive(Clone)]
pub struct Killer {
    pid: libc::pid_t,
    reaped: Reaped,
}

impl Killer {
    pub fn kill(&self) -> io::Result<()> {
        let reaped = lock(&self.reaped);
        if !*reaped {
            cvt(unsafe { libc::kill(self.pid, libc::SIGHUP) })?;
        }
        Ok(())
    }
}
