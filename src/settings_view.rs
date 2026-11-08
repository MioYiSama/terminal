//! The settings window: font and Catppuccin theme preferences. Changes apply
//! to every terminal immediately and are saved to disk.

use catppuccin::FlavorName;
use gpui::{
    App, Context, Div, ElementId, Entity, FontWeight, MouseButton, SharedString, Stateful,
    Subscription, Window, div, prelude::*, px,
};

use crate::font_picker::FontPicker;
use crate::i18n::tr;
use crate::settings::{Language, MAX_FONT_SIZE, MIN_FONT_SIZE, Settings, ThemeMode, WindowsShell};
use crate::theme::{UiColors, hsla};

const FLAVORS: [FlavorName; 4] = [
    FlavorName::Latte,
    FlavorName::Frappe,
    FlavorName::Macchiato,
    FlavorName::Mocha,
];

/// Theme modes and their message ids.
const THEME_MODES: [(ThemeMode, &str); 3] = [
    (ThemeMode::System, "settings-system"),
    (ThemeMode::Light, "settings-light"),
    (ThemeMode::Dark, "settings-dark"),
];

/// Preview line after the translated sample text.
const PREVIEW_CODE: &str = "0123456789 -> => != == {} [] () ~/.config $ ls -la";

pub struct SettingsView {
    font_picker: Entity<FontPicker>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            font_picker: cx.new(|cx| FontPicker::new(window, cx)),
            _subscriptions: vec![
                cx.observe_global_in::<Settings>(window, |_, window, cx| {
                    window.set_window_title(&tr(cx, "settings-title"));
                    cx.notify();
                }),
                cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            ],
        }
    }
}

fn section(title: SharedString, colors: UiColors) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_lg()
        .bg(colors.panel)
        .border_1()
        .border_color(colors.border)
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.muted)
                .child(title),
        )
}

fn row(label: SharedString, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_start()
        .gap_4()
        // Vertical padding matches the controls' so labels line up with
        // their first line.
        .child(div().w(px(110.)).flex_none().py(px(5.)).child(label))
        .child(control)
}

fn choice(id: impl Into<ElementId>, selected: bool, colors: UiColors) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .border_1()
        .map(|button| {
            if selected {
                button
                    .bg(colors.accent)
                    .border_color(colors.accent)
                    .text_color(colors.on_accent)
            } else {
                button
                    .bg(colors.background)
                    .border_color(colors.border)
                    .hover(|style| style.bg(colors.hover))
            }
        })
}

/// Background and accent dots previewing a flavor.
fn flavor_swatch(name: FlavorName) -> Div {
    let flavor = &catppuccin::PALETTE[name];
    let dot = |color: catppuccin::Rgb| div().size(px(8.)).rounded_full().bg(hsla(color));
    div()
        .flex()
        .gap(px(2.))
        .p(px(2.))
        .rounded_full()
        .bg(hsla(flavor.colors.base.rgb))
        .child(dot(flavor.colors.red.rgb))
        .child(dot(flavor.colors.green.rgb))
        .child(dot(flavor.colors.blue.rgb))
}

fn flavor_choices(
    id: &'static str,
    current: FlavorName,
    colors: UiColors,
    set: fn(&mut Settings, FlavorName),
) -> Div {
    div().flex().gap_2().children(FLAVORS.map(|name| {
        choice((id, name as usize), name == current, colors)
            .on_click(move |_, _, cx| Settings::update(cx, |settings| set(settings, name)))
            .child(flavor_swatch(name))
            .child(name.to_string())
    }))
}

fn step_font_size(cx: &mut App, delta: f32) {
    Settings::update(cx, |settings| settings.font_size = (settings.font_size + delta).round());
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = Settings::get(cx).clone();
        let flavor = settings.flavor(window.appearance());
        let colors = UiColors::new(flavor);

        let languages = div().flex().gap_2().children(Language::ALL.map(|language| {
            let label = language
                .native_name()
                .map_or_else(|| tr(cx, "settings-system"), SharedString::from);
            choice(("language", language as usize), settings.language == language, colors)
                .on_click(move |_, _, cx| {
                    Settings::update(cx, |settings| settings.language = language)
                })
                .child(label)
        }));

        let shells = div().flex().flex_col().gap_2().child(
            div().flex().gap_2().children(WindowsShell::ALL.map(|shell| {
                let label = shell
                    .program()
                    .map_or_else(|| tr(cx, "settings-shell-auto"), |program| {
                        program.trim_end_matches(".exe").into()
                    });
                choice(("shell", shell as usize), settings.windows_shell == shell, colors)
                    .on_click(move |_, _, cx| {
                        Settings::update(cx, |settings| settings.windows_shell = shell)
                    })
                    .child(label)
            })),
        )
        .child(div().text_xs().text_color(colors.muted).child(tr(cx, "settings-shell-hint")));

        let theme_modes = div().flex().gap_2().children(THEME_MODES.map(|(mode, label)| {
            choice(("theme-mode", mode as usize), settings.theme_mode == mode, colors)
                .on_click(move |_, _, cx| Settings::update(cx, |settings| settings.theme_mode = mode))
                .child(tr(cx, label))
        }));

        let stepper = |id: &'static str, label: &'static str, delta: f32, enabled: bool| {
            choice(id, false, colors)
                .when(!enabled, |button| button.opacity(0.4))
                .on_click(move |_, _, cx| step_font_size(cx, delta))
                .child(label)
        };
        let font_size = div()
            .flex()
            .items_center()
            .gap_2()
            .child(stepper("font-smaller", "−", -1., settings.font_size > MIN_FONT_SIZE))
            .child(
                div()
                    .w(px(48.))
                    .flex()
                    .justify_center()
                    .child(format!("{} pt", settings.font_size)),
            )
            .child(stepper("font-larger", "+", 1., settings.font_size < MAX_FONT_SIZE));

        let ansi = flavor.ansi_colors.all_ansi_colors();
        let preview = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded_md()
            .bg(hsla(flavor.colors.base.rgb))
            .border_1()
            .border_color(colors.border)
            .text_color(hsla(flavor.colors.text.rgb))
            .font_family(SharedString::from(settings.font_family.clone()))
            .text_size(settings.font_size())
            .child(
                div()
                    .text_xs()
                    .text_color(hsla(flavor.colors.subtext0.rgb))
                    .child(format!("{} · {} pt", settings.font_family, settings.font_size)),
            )
            .children(
                tr(cx, "settings-preview")
                    .lines()
                    .chain([PREVIEW_CODE])
                    .map(|line| div().child(line.to_owned())),
            )
            .child(
                div().flex().flex_wrap().gap_1().children(
                    ansi.iter()
                        .map(|color| div().size(px(16.)).rounded_sm().bg(hsla(color.rgb))),
                ),
            );

        div()
            .id("settings")
            // Clicking anywhere outside a text field takes focus away from it.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.blur())
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .bg(colors.background)
            .text_color(colors.text)
            .text_sm()
            .child(
                section(tr(cx, "settings-general"), colors)
                    .child(row(tr(cx, "settings-language"), languages))
                    .when(cfg!(windows), |section| {
                        section.child(row(tr(cx, "settings-shell"), shells))
                    }),
            )
            .child(
                section(tr(cx, "settings-appearance"), colors)
                    .child(row(tr(cx, "settings-theme"), theme_modes))
                    .child(row(
                        tr(cx, "settings-light-theme"),
                        flavor_choices("light-theme", settings.light_theme, colors, |s, name| {
                            s.light_theme = name
                        }),
                    ))
                    .child(row(
                        tr(cx, "settings-dark-theme"),
                        flavor_choices("dark-theme", settings.dark_theme, colors, |s, name| {
                            s.dark_theme = name
                        }),
                    )),
            )
            .child(
                section(tr(cx, "settings-font"), colors)
                    .child(row(tr(cx, "settings-font-size"), font_size))
                    .child(row(
                        tr(cx, "settings-font-family"),
                        div().flex_1().child(self.font_picker.clone()),
                    ))
                    .child(preview),
            )
    }
}
