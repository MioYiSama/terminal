//! Raw-FFI port of `ghostty/example/c-vt-hello`: create a terminal, write
//! styled text into it, and print the screen as plain text.

use core::ffi::CStr;
use core::ptr;
use ghostty_sys::*;

fn main() {
    unsafe {
        println!(
            "headers {}, library {}",
            GHOSTTY_VT_VERSION.to_str().unwrap(),
            CStr::from_ptr(ghostty_version()).to_str().unwrap()
        );

        let mut terminal: GhosttyTerminal = ptr::null_mut();
        assert_eq!(
            ghostty_terminal_new(ptr::null(), &mut terminal, 80, 24),
            GHOSTTY_SUCCESS
        );

        let text = b"Hello, \x1b[1;32mworld\x1b[0m!\r\n";
        ghostty_terminal_vt_write(terminal, text.as_ptr(), text.len());

        let mut formatter: GhosttyFormatter = ptr::null_mut();
        assert_eq!(
            ghostty_formatter_terminal_new(ptr::null(), &mut formatter, terminal),
            GHOSTTY_SUCCESS
        );

        let mut buf: *mut u8 = ptr::null_mut();
        let mut len = 0usize;
        assert_eq!(
            ghostty_formatter_format_alloc(formatter, ptr::null(), &mut buf, &mut len),
            GHOSTTY_SUCCESS
        );
        let screen = core::slice::from_raw_parts(buf, len);
        println!("{}", String::from_utf8_lossy(screen));

        ghostty_free(ptr::null(), buf, len);
        ghostty_formatter_free(formatter);
        ghostty_terminal_free(terminal);
    }
}
