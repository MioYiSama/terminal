// Release builds on Windows are GUI apps: no console window next to the terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod about;
mod element;
mod keys;
mod pty;
mod settings;
mod settings_view;
mod text_input;
mod theme;
mod view;
mod vt;

use gpui::{
    App, Application, Bounds, KeyBinding, Menu, MenuItem, Pixels, Render, Size, TitlebarOptions,
    Window, WindowBounds, WindowOptions, actions, prelude::*, px, size,
};

use about::AboutView;
use settings::Settings;
use settings_view::SettingsView;

use view::{
    Copy, DecreaseFontSize, IncreaseFontSize, KEY_CONTEXT, Paste, ResetFontSize, SelectAll,
    TerminalView,
};

actions!(app, [About, OpenSettings, NewWindow, Quit]);

fn bind_keys(cx: &mut App) {
    text_input::bind_keys(cx);
    settings_view::bind_keys(cx);
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
            KeyBinding::new("cmd-,", OpenSettings, None),
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
            KeyBinding::new("ctrl-,", OpenSettings, None),
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
        |window, cx| cx.new(|cx| TerminalView::new(window, cx)),
    );
    if let Err(err) = result {
        eprintln!("failed to open window: {err}");
    }
}

/// Focus the open window showing a `V`, or open one.
fn open_or_activate<V: Render>(
    cx: &mut App,
    title: &'static str,
    window_size: Size<Pixels>,
    new: fn(&mut Window, &mut gpui::Context<V>) -> V,
) {
    if let Some(handle) = cx.windows().iter().find_map(|window| window.downcast::<V>()) {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
        return;
    }
    let bounds = Bounds::centered(None, window_size, cx);
    let result = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(title.into()),
                ..Default::default()
            }),
            is_minimizable: false,
            ..Default::default()
        },
        |window, cx| cx.new(|cx| new(window, cx)),
    );
    if let Err(err) = result {
        eprintln!("failed to open {title} window: {err}");
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

        Settings::init(cx);
        bind_keys(cx);
        cx.on_action(|_: &About, cx| {
            open_or_activate(cx, "About Terminal", size(px(420.), px(260.)), AboutView::new)
        });
        cx.on_action(|_: &OpenSettings, cx| {
            open_or_activate(cx, "Settings", size(px(720.), px(680.)), SettingsView::new)
        });
        cx.on_action(|_: &NewWindow, cx| open_terminal_window(cx));
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![Menu {
            name: "Terminal".into(),
            items: vec![
                MenuItem::action("About Terminal", About),
                MenuItem::separator(),
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("New Window", NewWindow),
                MenuItem::separator(),
                MenuItem::action("Quit", Quit),
            ],
        }]);

        open_terminal_window(cx);
        cx.activate(true);
    });
}
