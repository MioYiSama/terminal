//! The settings window: font and Catppuccin theme preferences. Changes apply
//! to every terminal immediately and are saved to disk.

use catppuccin::FlavorName;
use gpui::{
    App, Context, Div, ElementId, Entity, FontWeight, KeyBinding, MouseButton, Point, ScrollStrategy,
    SharedString, Stateful, Subscription, UniformListScrollHandle, Window, actions, div,
    prelude::*, px, uniform_list,
};

use crate::i18n::{Language, tr};
use crate::settings::{MAX_FONT_SIZE, MIN_FONT_SIZE, Settings, ThemeMode};
use crate::text_input::{Changed, TextInput};
use crate::theme::{UiColors, hsla};

actions!(font_picker, [SelectPrevious, SelectNext, Confirm]);

const FONT_PICKER_CONTEXT: &str = "FontPicker";

pub fn bind_keys(cx: &mut App) {
    let ctx = Some(FONT_PICKER_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevious, ctx),
        KeyBinding::new("down", SelectNext, ctx),
        KeyBinding::new("enter", Confirm, ctx),
    ]);
}

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
    font_query: Entity<TextInput>,
    /// Installed font families, sorted and deduplicated.
    fonts: Vec<SharedString>,
    /// Indices into `fonts` matching the query: prefix matches first, then
    /// other substring matches.
    matches: Vec<usize>,
    /// Row of `matches` that Enter applies.
    highlighted: Option<usize>,
    font_scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut fonts: Vec<SharedString> = window
            .text_system()
            .all_font_names()
            .into_iter()
            // Dot-prefixed families are private system UI fonts on macOS.
            .filter(|name| !name.starts_with('.'))
            .map(SharedString::from)
            .collect();
        fonts.sort_unstable_by_key(|name| name.to_lowercase());
        fonts.dedup();

        let font_query = cx.new(|cx| TextInput::new(tr(cx, "settings-search-fonts"), cx));
        let subscriptions = vec![
            cx.observe_global_in::<Settings>(window, |view: &mut Self, window, cx| {
                window.set_window_title(&tr(cx, "settings-title"));
                let placeholder = tr(cx, "settings-search-fonts");
                view.font_query
                    .update(cx, |input, cx| input.set_placeholder(placeholder, cx));
                cx.notify();
            }),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            cx.subscribe(&font_query, |view, _, _: &Changed, cx| view.filter_fonts(cx)),
        ];

        let mut view = Self {
            font_query,
            matches: Vec::new(),
            fonts,
            highlighted: None,
            font_scroll: UniformListScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        view.filter_fonts(cx);
        view
    }

    fn filter_fonts(&mut self, cx: &mut Context<Self>) {
        let query = self.font_query.read(cx).text().trim().to_lowercase();
        let mut prefixed = Vec::new();
        let mut containing = Vec::new();
        for (index, name) in self.fonts.iter().enumerate() {
            let name = name.to_lowercase();
            if name.starts_with(&query) {
                prefixed.push(index);
            } else if name.contains(&query) {
                containing.push(index);
            }
        }
        prefixed.append(&mut containing);
        self.matches = prefixed;

        // Without a query, start at the current font; otherwise at the best
        // match.
        self.highlighted = if query.is_empty() {
            let current = &Settings::get(cx).font_family;
            self.matches
                .iter()
                .position(|&index| self.fonts[index] == current.as_str())
        } else {
            None
        }
        .or((!self.matches.is_empty()).then_some(0));
        if !query.is_empty() {
            self.font_scroll
                .0
                .borrow()
                .base_handle
                .set_offset(Point::default());
        } else if let Some(row) = self.highlighted {
            self.font_scroll
                .scroll_to_item_strict(row, ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// Highlight `row`, scrolling it into view at the edge given by
    /// `strategy` if it is off-screen.
    fn highlight(&mut self, row: usize, strategy: ScrollStrategy, cx: &mut Context<Self>) {
        self.highlighted = Some(row);
        self.font_scroll.scroll_to_item(row, strategy);
        cx.notify();
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.highlighted.and_then(|row| row.checked_sub(1)) {
            self.highlight(row, ScrollStrategy::Top, cx);
        }
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let row = self.highlighted.map_or(0, |row| row + 1);
        if row < self.matches.len() {
            self.highlight(row, ScrollStrategy::Bottom, cx);
        }
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.highlighted {
            let family = self.fonts[self.matches[row]].to_string();
            Settings::update(cx, |settings| settings.font_family = family);
        }
    }

    fn render_font_picker(&self, colors: UiColors, cx: &mut Context<Self>) -> Div {
        let list = uniform_list(
            "font-families",
            self.matches.len(),
            cx.processor(move |view, range: std::ops::Range<usize>, _window, cx| {
                let current = Settings::get(cx).font_family.clone();
                range
                    .map(|row| {
                        let name = view.fonts[view.matches[row]].clone();
                        let selected = name == current.as_str();
                        let highlighted = view.highlighted == Some(row);
                        let family = name.to_string();
                        div()
                            .id(row)
                            .px_2()
                            .py(px(3.))
                            .rounded_sm()
                            .cursor_pointer()
                            .map(|item| {
                                if selected {
                                    item.bg(colors.accent).text_color(colors.on_accent)
                                } else if highlighted {
                                    item.bg(colors.hover)
                                } else {
                                    item.hover(|style| style.bg(colors.border))
                                }
                            })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.highlighted = Some(row);
                                let family = family.clone();
                                Settings::update(cx, |settings| settings.font_family = family);
                            }))
                            .child(name)
                    })
                    .collect()
            }),
        )
        .track_scroll(self.font_scroll.clone())
        .size_full();

        let results = div()
            .h(px(200.))
            .p_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(colors.background)
            .map(|results| {
                if self.matches.is_empty() {
                    results.child(
                        div()
                            .px_2()
                            .py(px(3.))
                            .text_color(colors.muted)
                            .child(tr(cx, "settings-no-matching-fonts")),
                    )
                } else {
                    results.child(list)
                }
            });

        div()
            .key_context(FONT_PICKER_CONTEXT)
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .flex()
            .flex_col()
            .gap_2()
            .child(self.font_query.clone())
            .child(results)
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
                    .child(row(tr(cx, "settings-language"), languages)),
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
                        self.render_font_picker(colors, cx).flex_1(),
                    ))
                    .child(preview),
            )
    }
}
