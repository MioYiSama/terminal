//! A searchable list of the installed font families. Picking one (click or
//! Enter) makes it the terminal font.

use gpui::{
    App, Context, Entity, KeyBinding, Point, ScrollStrategy, SharedString, Subscription,
    UniformListScrollHandle, Window, actions, div, prelude::*, px, uniform_list,
};

use crate::i18n::tr;
use crate::settings::Settings;
use crate::text_input::{Changed, TextInput};
use crate::theme::UiColors;

actions!(font_picker, [SelectPrevious, SelectNext, Confirm]);

const KEY_CONTEXT: &str = "FontPicker";

pub fn bind_keys(cx: &mut App) {
    let ctx = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevious, ctx),
        KeyBinding::new("down", SelectNext, ctx),
        KeyBinding::new("enter", Confirm, ctx),
    ]);
}

struct FontFamily {
    name: SharedString,
    /// Lowercased `name`, which queries are matched against.
    key: String,
}

pub struct FontPicker {
    query: Entity<TextInput>,
    /// Installed families, sorted case-insensitively and deduplicated.
    fonts: Vec<FontFamily>,
    /// Indices into `fonts` matching the query: prefix matches first, then
    /// other substring matches.
    matches: Vec<usize>,
    /// Row of `matches` that Enter applies.
    highlighted: Option<usize>,
    scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl FontPicker {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut fonts: Vec<FontFamily> = window
            .text_system()
            .all_font_names()
            .into_iter()
            // Dot-prefixed families are private system UI fonts on macOS.
            .filter(|name| !name.starts_with('.'))
            .map(|name| FontFamily {
                key: name.to_lowercase(),
                name: name.into(),
            })
            .collect();
        fonts.sort_unstable_by(|a, b| a.key.cmp(&b.key).then_with(|| a.name.cmp(&b.name)));
        fonts.dedup_by(|a, b| a.name == b.name);

        let query = cx.new(|cx| TextInput::new(tr(cx, "settings-search-fonts"), cx));
        let subscriptions = vec![
            cx.observe_global_in::<Settings>(window, |picker: &mut Self, _, cx| {
                let placeholder = tr(cx, "settings-search-fonts");
                picker
                    .query
                    .update(cx, |input, cx| input.set_placeholder(placeholder, cx));
                cx.notify();
            }),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            cx.subscribe(&query, |picker, _, _: &Changed, cx| picker.filter(cx)),
        ];

        let mut picker = Self {
            query,
            fonts,
            matches: Vec::new(),
            highlighted: None,
            scroll: UniformListScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        picker.filter(cx);
        picker
    }

    fn filter(&mut self, cx: &mut Context<Self>) {
        let query = self.query.read(cx).text().trim().to_lowercase();
        let mut prefixed = Vec::new();
        let mut containing = Vec::new();
        for (index, font) in self.fonts.iter().enumerate() {
            if font.key.starts_with(&query) {
                prefixed.push(index);
            } else if font.key.contains(&query) {
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
                .position(|&index| self.fonts[index].name == current.as_str())
        } else {
            None
        }
        .or((!self.matches.is_empty()).then_some(0));
        if !query.is_empty() {
            self.scroll
                .0
                .borrow()
                .base_handle
                .set_offset(Point::default());
        } else if let Some(row) = self.highlighted {
            self.scroll
                .scroll_to_item_strict(row, ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// Highlight `row`, scrolling it into view at the edge given by
    /// `strategy` if it is off-screen.
    fn highlight(&mut self, row: usize, strategy: ScrollStrategy, cx: &mut Context<Self>) {
        self.highlighted = Some(row);
        self.scroll.scroll_to_item(row, strategy);
        cx.notify();
    }

    /// Make the family in `row` the terminal font.
    fn pick(&mut self, row: usize, cx: &mut Context<Self>) {
        self.highlighted = Some(row);
        let family = self.fonts[self.matches[row]].name.to_string();
        Settings::update(cx, |settings| settings.font_family = family);
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
            self.pick(row, cx);
        }
    }
}

impl Render for FontPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = UiColors::new(Settings::get(cx).flavor(window.appearance()));
        let list = uniform_list(
            "font-families",
            self.matches.len(),
            cx.processor(move |picker, range: std::ops::Range<usize>, _window, cx| {
                let current = &Settings::get(cx).font_family;
                range
                    .map(|row| {
                        let name = picker.fonts[picker.matches[row]].name.clone();
                        let selected = name == current.as_str();
                        let highlighted = picker.highlighted == Some(row);
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
                            .on_click(cx.listener(move |picker, _, _, cx| picker.pick(row, cx)))
                            .child(name)
                    })
                    .collect()
            }),
        )
        .track_scroll(self.scroll.clone())
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
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .flex()
            .flex_col()
            .gap_2()
            .child(self.query.clone())
            .child(results)
    }
}
