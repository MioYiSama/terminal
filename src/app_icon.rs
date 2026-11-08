/// Sets the Dock icon for a bare macOS executable; call on the main thread.
#[cfg(target_os = "macos")]
pub fn set_app_icon() {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let mtm = MainThreadMarker::new().expect("the app icon must be set on the main thread");
    let data = NSData::with_bytes(include_bytes!("../assets/terminal.png"));
    let image = NSImage::initWithData(NSImage::alloc(), &data)
        .expect("the embedded application icon must be a valid PNG");

    // SAFETY: The image is present, as required by AppKit, and this is the main thread.
    unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
}

/// Windows uses the embedded icon resource; GPUI exposes no Linux window-icon API.
#[cfg(not(target_os = "macos"))]
pub fn set_app_icon() {}
