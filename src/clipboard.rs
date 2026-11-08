//! Clipboard contents as terminal input. Copied files paste as their paths
//! and copied images as the path of a file holding the image, which is how
//! programs such as coding agents take attachments; anything else pastes as
//! text. Quoting paths for the shell is up to the caller.

use std::borrow::Cow;
use std::io;
use std::path::PathBuf;

use gpui::{App, ClipboardEntry, Image, ImageFormat};

pub enum Content {
    /// Copied files.
    Paths(Vec<PathBuf>),
    /// A copied image; [`save_image`] turns it into a path to paste.
    Image(Image),
    Text(String),
}

pub fn read(cx: &App) -> Option<Content> {
    #[cfg(any(target_os = "macos", windows))]
    {
        let files = native::file_paths();
        if !files.is_empty() {
            return Some(Content::Paths(files));
        }
    }
    let item = cx.read_from_clipboard()?;
    if let Some(text) = item.text() {
        #[cfg(not(any(target_os = "macos", windows)))]
        if let Some(files) = file_uris(&text) {
            return Some(Content::Paths(files));
        }
        return Some(Content::Text(text));
    }
    item.into_entries().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(Content::Image(image)),
        ClipboardEntry::String(_) => None,
    })
}

/// Write the image to the per-user temporary directory, named after its
/// content so pasting the same image again reuses the file, and return its
/// path. TIFF (all that many macOS apps copy) and BMP become
/// PNG: programs taking image paths accept PNG but not those.
pub fn save_image(image: &Image) -> io::Result<PathBuf> {
    let (bytes, extension) = match image.format {
        ImageFormat::Png => (Cow::Borrowed(&image.bytes), "png"),
        ImageFormat::Jpeg => (Cow::Borrowed(&image.bytes), "jpg"),
        ImageFormat::Webp => (Cow::Borrowed(&image.bytes), "webp"),
        ImageFormat::Gif => (Cow::Borrowed(&image.bytes), "gif"),
        ImageFormat::Svg => (Cow::Borrowed(&image.bytes), "svg"),
        ImageFormat::Tiff => (Cow::Owned(to_png(&image.bytes, image::ImageFormat::Tiff)?), "png"),
        ImageFormat::Bmp => (Cow::Owned(to_png(&image.bytes, image::ImageFormat::Bmp)?), "png"),
    };
    let dir = dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("terminal-clipboard");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("image-{:016x}.{extension}", image.id()));
    std::fs::write(&path, bytes.as_slice())?;
    Ok(path)
}

fn to_png(bytes: &[u8], format: image::ImageFormat) -> io::Result<Vec<u8>> {
    let decoded = image::load_from_memory_with_format(bytes, format).map_err(io::Error::other)?;
    let mut png = Vec::new();
    decoded
        .write_to(&mut io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(io::Error::other)?;
    Ok(png)
}

/// Linux file managers offer copied files as a list of `file://` URIs (KDE
/// as plain text too); GPUI only reads the text, so recognize that list.
#[cfg(not(any(target_os = "macos", windows)))]
fn file_uris(text: &str) -> Option<Vec<PathBuf>> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let mut paths = Vec::new();
    for line in text.lines().map(str::trim) {
        // RFC 2483: lines starting with `#` are comments.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let rest = line.strip_prefix("file://")?;
        let path = rest.strip_prefix("localhost").unwrap_or(rest);
        if !path.starts_with('/') {
            return None;
        }
        paths.push(PathBuf::from(OsString::from_vec(percent_decode(path)?)));
    }
    (!paths.is_empty()).then_some(paths)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn percent_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(text.get(i + 1..i + 3)?, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

#[cfg(target_os = "macos")]
mod native {
    use std::path::PathBuf;

    use objc2::ClassType;
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSArray, NSURL};

    /// Files copied in Finder. GPUI would read these as their names only.
    pub fn file_paths() -> Vec<PathBuf> {
        let pasteboard = NSPasteboard::generalPasteboard();
        let classes = NSArray::from_slice(&[NSURL::class()]);
        // SAFETY: NSURL implements NSPasteboardReading; no options.
        let Some(objects) = (unsafe { pasteboard.readObjectsForClasses_options(&classes, None) })
        else {
            return Vec::new();
        };
        objects
            .iter()
            .filter_map(|object| {
                let url = object.downcast::<NSURL>().ok()?;
                // `path` also resolves Finder's file reference URLs.
                url.isFileURL()
                    .then(|| url.path())
                    .flatten()
                    .map(|path| PathBuf::from(path.to_string()))
            })
            .collect()
    }
}

#[cfg(windows)]
mod native {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    /// Files copied in Explorer. GPUI reads these as one string with the
    /// paths run together.
    pub fn file_paths() -> Vec<PathBuf> {
        let format = u32::from(CF_HDROP.0);
        let mut paths = Vec::new();
        unsafe {
            if IsClipboardFormatAvailable(format).is_err() || OpenClipboard(None).is_err() {
                return paths;
            }
            if let Ok(handle) = GetClipboardData(format) {
                let hdrop = HDROP(handle.0);
                let count = DragQueryFileW(hdrop, u32::MAX, None);
                for index in 0..count {
                    let len = DragQueryFileW(hdrop, index, None) as usize;
                    let mut buf = vec![0u16; len + 1];
                    let copied = DragQueryFileW(hdrop, index, Some(&mut buf)) as usize;
                    if copied > 0 {
                        paths.push(PathBuf::from(OsString::from_wide(&buf[..copied])));
                    }
                }
            }
            let _ = CloseClipboard();
        }
        paths
    }
}

#[cfg(all(test, not(any(target_os = "macos", windows))))]
mod tests {
    use super::*;

    #[test]
    fn reads_file_uri_lists_only() {
        assert_eq!(
            file_uris("# comment\r\nfile:///tmp/a%20b.png\r\nfile://localhost/c\r\n"),
            Some(vec![PathBuf::from("/tmp/a b.png"), PathBuf::from("/c")])
        );
        assert_eq!(file_uris("file:///a\nplain text"), None);
        assert_eq!(file_uris("file://host/a"), None);
        assert_eq!(file_uris("file:///a%2"), None);
    }
}
