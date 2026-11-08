//! User settings, kept as a GPUI global. Loaded from
//! `<config_dir>/terminal/settings.json` at startup and saved in the
//! background on every change.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use catppuccin::{Flavor, FlavorName, Palette};
use gpui::{App, Global, Pixels, WindowAppearance, px};
use serde::{Deserialize, Serialize};

pub const MIN_FONT_SIZE: f32 = 6.;
pub const MAX_FONT_SIZE: f32 = 72.;
const DEFAULT_FONT_SIZE: f32 = 14.;

/// Monospace families to try when no family is configured, most preferred
/// first.
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

/// A `static` copy of the palette so flavors can be borrowed for `'static`.
static PALETTE: Palette = catppuccin::PALETTE;

/// UI language; `i18n` translates into it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    /// Follow the system's preferred language.
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en-US")]
    English,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
}

impl Language {
    pub const ALL: [Self; 3] = [Self::System, Self::English, Self::SimplifiedChinese];

    /// The language's name in itself, as shown in the language picker.
    /// `None` for [`Language::System`], whose label is translated.
    pub fn native_name(self) -> Option<&'static str> {
        match self {
            Self::System => None,
            Self::English => Some("English"),
            Self::SimplifiedChinese => Some("简体中文"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    /// Follow the system's light/dark appearance.
    #[default]
    System,
    Light,
    Dark,
}

/// Shell started in new tabs on Windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowsShell {
    Cmd,
    PowerShell,
    Pwsh,
    /// The first available of `pwsh`, `powershell` and `cmd`.
    #[default]
    Auto,
}

impl WindowsShell {
    pub const ALL: [Self; 4] = [Self::Cmd, Self::PowerShell, Self::Pwsh, Self::Auto];

    /// Executable name, looked up the way `CreateProcess` does; `None`
    /// for automatic detection.
    pub fn program(self) -> Option<&'static str> {
        match self {
            Self::Cmd => Some("cmd.exe"),
            Self::PowerShell => Some("powershell.exe"),
            Self::Pwsh => Some("pwsh.exe"),
            Self::Auto => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: Language,
    pub font_family: String,
    pub font_size: f32,
    pub theme_mode: ThemeMode,
    /// Flavor used when the theme resolves to light.
    pub light_theme: FlavorName,
    /// Flavor used when the theme resolves to dark.
    pub dark_theme: FlavorName,
    pub windows_shell: WindowsShell,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::System,
            font_family: String::new(),
            font_size: DEFAULT_FONT_SIZE,
            theme_mode: ThemeMode::System,
            light_theme: FlavorName::Latte,
            dark_theme: FlavorName::Mocha,
            windows_shell: WindowsShell::Auto,
        }
    }
}

impl Global for Settings {}

impl Settings {
    /// Load the settings file and install the global.
    pub fn init(cx: &mut App) {
        let mut settings = load().unwrap_or_default();
        settings.font_size = settings.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        if settings.font_family.is_empty() {
            settings.font_family = default_font_family(cx);
        }
        cx.set_global(settings);
        // Write a save still queued when the app quits.
        cx.on_app_quit(|_| {
            flush();
            async {}
        })
        .detach();
    }

    pub fn get(cx: &App) -> &Self {
        cx.global()
    }

    /// Change the settings, then save them and notify observers if anything
    /// changed.
    pub fn update(cx: &mut App, change: impl FnOnce(&mut Self)) {
        let mut settings = Self::get(cx).clone();
        change(&mut settings);
        settings.font_size = settings.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        if settings == *Self::get(cx) {
            return;
        }
        queue_save(settings.clone(), cx);
        cx.set_global(settings);
    }

    /// Program to run in new terminals; `None` for the platform default.
    pub fn shell(&self) -> Option<&'static str> {
        if cfg!(windows) {
            self.windows_shell.program()
        } else {
            None
        }
    }

    pub fn font_size(&self) -> Pixels {
        px(self.font_size)
    }

    /// Whether the theme resolves to dark for a window with `appearance`.
    pub fn is_dark(&self, appearance: WindowAppearance) -> bool {
        match self.theme_mode {
            ThemeMode::System => matches!(
                appearance,
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
        }
    }

    /// The Catppuccin flavor in effect for a window with `appearance`.
    pub fn flavor(&self, appearance: WindowAppearance) -> &'static Flavor {
        let name = if self.is_dark(appearance) {
            self.dark_theme
        } else {
            self.light_theme
        };
        &PALETTE[name]
    }
}

fn default_font_family(cx: &App) -> String {
    let available = cx.text_system().all_font_names();
    FONT_CANDIDATES
        .iter()
        .find(|candidate| available.iter().any(|name| name == *candidate))
        .unwrap_or(&FONT_CANDIDATES[0])
        .to_string()
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("terminal").join("settings.json"))
}

fn load() -> Option<Settings> {
    let path = path()?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            eprintln!("failed to read {}: {err}", path.display());
            return None;
        }
    };
    serde_json::from_str(&text)
        .inspect_err(|err| eprintln!("invalid settings in {}: {err}", path.display()))
        .ok()
}

/// Settings waiting to be written. Only the latest matters.
static PENDING_SAVE: Mutex<Option<Settings>> = Mutex::new(None);
/// Held while writing, so writes never interleave.
static WRITING: Mutex<()> = Mutex::new(());

/// Save off the UI thread. Every queued save is followed by a flush, so
/// the last settings queued are the last written.
fn queue_save(settings: Settings, cx: &App) {
    *lock(&PENDING_SAVE) = Some(settings);
    cx.background_executor().spawn(async { flush() }).detach();
}

/// Write the latest queued settings, if a flush has not already.
fn flush() {
    let _writing = lock(&WRITING);
    let pending = lock(&PENDING_SAVE).take();
    if let Some(settings) = pending {
        save(&settings);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn save(settings: &Settings) {
    let Some(path) = path() else {
        eprintln!("no config directory; settings not saved");
        return;
    };
    // Write a sibling file and rename it over the old one, so a failed
    // write never leaves a truncated settings file behind.
    let temp = path.with_extension("json.tmp");
    let result = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| {
            let json = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
            fs::write(&temp, json)?;
            fs::rename(&temp, &path)
        });
    if let Err(err) = result {
        eprintln!("failed to save {}: {err}", path.display());
    }
}
