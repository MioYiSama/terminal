// Release builds on Windows are GUI apps: no console window next to the terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod element;
mod keys;
mod pty;
mod view;
mod vt;

use gpui::{
    App, Application, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, prelude::*, px, size,
};

use view::{
    Copy, DecreaseFontSize, IncreaseFontSize, KEY_CONTEXT, Paste, ResetFontSize, SelectAll,
    TerminalView,
};

actions!(app, [NewWindow, Quit]);

/// Monospace families to try, most preferred first.
const FONT_CANDIDATES: &[&str] = if cfg!(target_os = "macos") {
    &["SF Mono", "Menlo", "Monaco"]
} else if cfg!(target_os = "windows") {
    &["Cascadia Mono", "Consolas", "Courier New"]
} else {
    &[
        "DejaVu Sans Mono",
        "Noto Sans Mono",
        "Liberation Mono",
        "Ubuntu Mono",
        "Source Code Pro",
    ]
};

fn pick_font_family(window: &Window) -> String {
    let available = window.text_system().all_font_names();
    FONT_CANDIDATES
        .iter()
        .find(|candidate| available.iter().any(|name| name == *candidate))
        .unwrap_or(&FONT_CANDIDATES[0])
        .to_string()
}

fn bind_keys(cx: &mut App) {
    let ctx = Some(KEY_CONTEXT);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-c", Copy, ctx),
            KeyBinding::new("cmd-v", Paste, ctx),
            KeyBinding::new("cmd-a", SelectAll, ctx),
            KeyBinding::new("cmd-=", IncreaseFontSize, ctx),
            KeyBinding::new("cmd-+", IncreaseFontSize, ctx),
            KeyBinding::new("cmd--", DecreaseFontSize, ctx),
            KeyBinding::new("cmd-0", ResetFontSize, ctx),
            KeyBinding::new("cmd-n", NewWindow, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
    } else {
        // Plain Ctrl+C/V belong to the shell, so clipboard shortcuts use
        // Ctrl+Shift like other Linux/Windows terminals.
        cx.bind_keys([
            KeyBinding::new("ctrl-shift-c", Copy, ctx),
            KeyBinding::new("ctrl-shift-v", Paste, ctx),
            KeyBinding::new("shift-insert", Paste, ctx),
            KeyBinding::new("ctrl-shift-a", SelectAll, ctx),
            KeyBinding::new("ctrl-=", IncreaseFontSize, ctx),
            KeyBinding::new("ctrl-+", IncreaseFontSize, ctx),
            KeyBinding::new("ctrl--", DecreaseFontSize, ctx),
            KeyBinding::new("ctrl-0", ResetFontSize, ctx),
            KeyBinding::new("ctrl-shift-n", NewWindow, None),
        ]);
    }
}

fn open_terminal_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(860.), px(540.)), cx);
    let result = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Terminal".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| {
            let family = pick_font_family(window);
            cx.new(|cx| TerminalView::new(family, window, cx))
        },
    );
    if let Err(err) = result {
        eprintln!("failed to open window: {err}");
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        // Quit when the last window closes (macOS keeps apps alive otherwise).
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        bind_keys(cx);
        cx.on_action(|_: &NewWindow, cx| open_terminal_window(cx));
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![Menu {
            name: "Terminal".into(),
            items: vec![
                MenuItem::action("New Window", NewWindow),
                MenuItem::separator(),
                MenuItem::action("Quit", Quit),
            ],
        }]);

        open_terminal_window(cx);
        cx.activate(true);
    });
}
