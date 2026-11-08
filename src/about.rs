//! The "About Terminal" window.

use gpui::{Context, FontWeight, Subscription, Window, div, prelude::*, px};

use crate::settings::Settings;
use crate::theme::UiColors;

const REPOSITORY: &str = "https://github.com/MioYiSama/terminal";

pub struct AboutView {
    _subscriptions: Vec<Subscription>,
}

impl AboutView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: vec![
                cx.observe_global_in::<Settings>(window, |_, _, cx| cx.notify()),
                cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            ],
        }
    }
}

impl Render for AboutView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = UiColors::new(Settings::get(cx).flavor(window.appearance()));
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .p_6()
            .bg(colors.background)
            .text_color(colors.text)
            .text_sm()
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .child("Terminal"),
            )
            .child(
                div()
                    .text_color(colors.muted)
                    .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
            )
            .child(div().mt_2().child("A terminal emulator built on GPUI and libghostty-vt."))
            .child(
                div()
                    .text_color(colors.muted)
                    .child("Colors by Catppuccin."),
            )
            .child(
                div()
                    .id("repository")
                    .mt_2()
                    .px_3()
                    .py(px(4.))
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(colors.accent)
                    .hover(|style| style.bg(colors.border))
                    .on_click(|_, _, cx| cx.open_url(REPOSITORY))
                    .child(REPOSITORY),
            )
    }
}
