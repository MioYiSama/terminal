use std::env;
use std::ffi::{OsStr, OsString, c_void};
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::Arc;

use windows::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0};
use windows::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

use crate::{Command, PtySize};

fn coord(size: PtySize) -> COORD {
    let clamp = |n: u16| n.min(i16::MAX as u16) as i16;
    COORD {
        X: clamp(size.cols),
        Y: clamp(size.rows),
    }
}

fn borrowed(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

fn pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    unsafe {
        CreatePipe(&mut read, &mut write, None, 0)?;
        Ok((
            OwnedHandle::from_raw_handle(read.0),
            OwnedHandle::from_raw_handle(write.0),
        ))
    }
}

pub struct Master {
    console: HPCON,
    /// Write end of the console's input pipe.
    input: File,
    /// Read end of the console's output pipe.
    output: File,
}

impl Master {
    pub fn reader(&self) -> io::Result<File> {
        self.output.try_clone()
    }

    pub fn writer(&self) -> io::Result<File> {
        self.input.try_clone()
    }

    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        unsafe { ResizePseudoConsole(self.console, coord(size))? };
        Ok(())
    }

    /// ConPTY does not say which process is in the foreground.
    pub fn foreground_cwd(&self) -> Option<PathBuf> {
        None
    }
}

impl Drop for Master {
    fn drop(&mut self) {
        // Ends the attached processes' console session and closes the
        // console's ends of the pipes, so readers see EOF.
        unsafe { ClosePseudoConsole(self.console) };
    }
}

pub fn spawn(cmd: &Command, size: PtySize) -> io::Result<(Master, Child)> {
    let (input_read, input_write) = pipe()?;
    let (output_read, output_write) = pipe()?;
    let console = unsafe {
        CreatePseudoConsole(
            coord(size),
            borrowed(&input_read),
            borrowed(&output_write),
            0,
        )?
    };
    // The console duplicated its ends of the pipes; only it may hold them.
    drop(input_read);
    drop(output_write);
    let master = Master {
        console,
        input: File::from(input_write),
        output: File::from(output_read),
    };
    let process = spawn_child(cmd, console)?;
    Ok((
        master,
        Child {
            process: Arc::new(process),
        },
    ))
}

/// Owns an initialised attribute list and its backing storage.
struct AttributeList {
    _storage: Vec<usize>,
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl AttributeList {
    fn with_pseudo_console(console: HPCON) -> io::Result<Self> {
        let mut size = 0;
        // Sizing call: fails with ERROR_INSUFFICIENT_BUFFER by design.
        let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut size) };
        let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
        unsafe { InitializeProcThreadAttributeList(Some(list), 1, None, &mut size)? };
        let this = Self {
            _storage: storage,
            list,
        };
        // The attribute value is the HPCON itself, not a pointer to it.
        unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                Some(console.0 as *const c_void),
                size_of::<HPCON>(),
                None,
                None,
            )?
        };
        Ok(this)
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.list) };
    }
}

fn spawn_child(cmd: &Command, console: HPCON) -> io::Result<OwnedHandle> {
    let attributes = AttributeList::with_pseudo_console(console)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    // Explicitly invalid std handles; otherwise the child can inherit this
    // process's redirected handles instead of attaching to the console.
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
    startup.lpAttributeList = attributes.list;

    let mut command_line = command_line(cmd);
    let environment = environment_block(cmd);
    let cwd = cmd.cwd.as_ref().map(|dir| wide(dir.as_os_str()));
    let mut info = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            Some(environment.as_ptr().cast()),
            cwd.as_ref()
                .map_or(PCWSTR::null(), |dir| PCWSTR(dir.as_ptr())),
            &startup.StartupInfo,
            &mut info,
        )?;
        drop(OwnedHandle::from_raw_handle(info.hThread.0));
        Ok(OwnedHandle::from_raw_handle(info.hProcess.0))
    }
}

/// NUL-terminated UTF-16.
fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain([0]).collect()
}

fn command_line(cmd: &Command) -> Vec<u16> {
    let program = cmd.program.clone().unwrap_or_else(default_shell);
    let mut line = Vec::new();
    for (i, arg) in [&program].into_iter().chain(&cmd.args).enumerate() {
        if i > 0 {
            line.push(b' ' as u16);
        }
        push_quoted(&mut line, arg);
    }
    line.push(0);
    line
}

/// PowerShell 7 (`pwsh.exe`), then Windows PowerShell (`powershell.exe`), as
/// found on `PATH`; otherwise `%ComSpec%` or `cmd.exe`.
fn default_shell() -> OsString {
    ["pwsh.exe", "powershell.exe"]
        .into_iter()
        .find_map(find_on_path)
        .map(PathBuf::into_os_string)
        .or_else(|| env::var_os("ComSpec").filter(|shell| !shell.is_empty()))
        .unwrap_or_else(|| "cmd.exe".into())
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Quote `arg` so `CommandLineToArgvW` and the MSVC runtime parse it back
/// unchanged.
fn push_quoted(line: &mut Vec<u16>, arg: &OsStr) {
    const QUOTE: u16 = b'"' as u16;
    const BACKSLASH: u16 = b'\\' as u16;
    let needs_quotes = arg.is_empty()
        || arg
            .encode_wide()
            .any(|c| c == b' ' as u16 || c == b'\t' as u16 || c == QUOTE);
    if !needs_quotes {
        line.extend(arg.encode_wide());
        return;
    }
    line.push(QUOTE);
    let mut backslashes = 0;
    for c in arg.encode_wide() {
        match c {
            BACKSLASH => backslashes += 1,
            QUOTE => {
                // Backslashes before a quote are escaped, then the quote.
                line.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2 + 1));
                backslashes = 0;
            }
            _ => {
                line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
                backslashes = 0;
            }
        }
        if c != BACKSLASH {
            line.push(c);
        }
    }
    // Trailing backslashes precede the closing quote, so escape them too.
    line.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2));
    line.push(QUOTE);
}

/// The current environment with `cmd.env_remove` and `cmd.env` applied, as a
/// sorted UTF-16 block. Variable names are case-insensitive on Windows.
fn environment_block(cmd: &Command) -> Vec<u16> {
    let mut vars: Vec<(OsString, OsString)> = env::vars_os()
        .filter(|(name, _)| !cmd.env_remove.iter().any(|key| name.eq_ignore_ascii_case(key)))
        .collect();
    for (key, value) in &cmd.env {
        match vars
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
        {
            Some(var) => var.1 = value.clone(),
            None => vars.push((key.clone(), value.clone())),
        }
    }
    vars.sort_by_cached_key(|(name, _)| name.to_ascii_uppercase());
    let mut block = Vec::new();
    for (name, value) in &vars {
        block.extend(name.encode_wide());
        block.push(b'=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

pub struct Child {
    process: Arc<OwnedHandle>,
}

impl Child {
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let process = borrowed(&self.process);
        if unsafe { WaitForSingleObject(process, INFINITE) } == WAIT_FAILED {
            return Err(io::Error::last_os_error());
        }
        let mut code = 0;
        unsafe { GetExitCodeProcess(process, &mut code)? };
        Ok(ExitStatus::from_raw(code))
    }

    pub fn killer(&self) -> Killer {
        Killer {
            process: self.process.clone(),
        }
    }
}

/// Shares the process handle, which keeps the process id from being reused.
#[derive(Clone)]
pub struct Killer {
    process: Arc<OwnedHandle>,
}

impl Killer {
    pub fn kill(&self) -> io::Result<()> {
        let process = borrowed(&self.process);
        if unsafe { WaitForSingleObject(process, 0) } == WAIT_OBJECT_0 {
            return Ok(());
        }
        unsafe { TerminateProcess(process, 1)? };
        Ok(())
    }
}
