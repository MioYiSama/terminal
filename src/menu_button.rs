//! In-window application menu. GPUI turns `cx.set_menus` into a native menu
//! bar only on macOS; on Windows and Linux it just stores the menus, so
//! terminal windows show them in a dropdown behind a button at the left end
//! of the tab bar.

use gpui::{
    Context, MouseButton, OwnedMenuItem, Render, Window, deferred, div, prelude::*, px,
};

use crate::settings::Settings;
use crate::theme::UiColors;

pub const ENABLED: bool = !cfg!(target_os = "macos");

#[derive(Default)]
pub struct MenuButton {
    open: bool,
}

impl MenuButton {
    fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open != open {
            self.open = open;
            cx.notify();
        }
    }

    /// All menus' items in one list, a separator between menus.
    fn items(cx: &Context<Self>) -> Vec<OwnedMenuItem> {
        let mut items = Vec::new();
        for menu in cx.get_menus().unwrap_or_default() {
            if !items.is_empty() {
                items.push(OwnedMenuItem::Separator);
            }
            items.extend(menu.items);
        }
        items
    }

    fn render_items(
        &self,
        colors: UiColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("menu-items")
            .occlude()
            .absolute()
            .top_full()
            .left_0()
            .min_w(px(220.))
            .p_1()
            .flex()
            .flex_col()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(colors.panel)
            .shadow_md()
            .text_color(colors.text)
            // Capture phase: runs before the button's own handler, which
            // then sees the menu as it was rendered and keeps it closed.
            .on_mouse_down_out(cx.listener(|button, _, _, cx| button.set_open(false, cx)))
            .children(Self::items(cx).into_iter().enumerate().filter_map(|(ix, item)| {
                match item {
                    OwnedMenuItem::Separator => Some(
                        div()
                            .id(ix)
                            .my_1()
                            .h(px(1.))
                            .bg(colors.border)
                            .into_any_element(),
                    ),
                    OwnedMenuItem::Action { name, action, .. } => {
                        let shortcut = window
                            .highest_precedence_binding_for_action(action.as_ref())
                            .map(|binding| {
                                binding
                                    .keystrokes()
                                    .iter()
                                    .map(ToString::to_string)
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            });
                        Some(
                            div()
                                .id(ix)
                                .flex()
                                .justify_between()
                                .gap_6()
                                .px_2()
                                .py(px(3.))
                                .rounded_sm()
                                .hover(|style| style.bg(colors.hover))
                                .on_click(cx.listener(move |button, _, window, cx| {
                                    button.set_open(false, cx);
                                    window.dispatch_action(action.boxed_clone(), cx);
                                }))
                                .child(name)
                                .children(
                                    shortcut.map(|keys| div().text_color(colors.muted).child(keys)),
                                )
                                .into_any_element(),
                        )
                    }
                    // Submenus and OS-provided menus are unused here.
                    OwnedMenuItem::Submenu(_) | OwnedMenuItem::SystemMenu(_) => None,
                }
            }))
    }
}

impl Render for MenuButton {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = UiColors::new(Settings::get(cx).flavor(window.appearance()));
        let open = self.open;
        let bar = || div().w(px(12.)).h(px(1.5)).rounded_full().bg(colors.muted);
        let button = div()
            .id("menu-button")
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(3.))
            .size(px(24.))
            .rounded_sm()
            .map(|button| {
                if open {
                    button.bg(colors.hover)
                } else {
                    button.hover(|style| style.bg(colors.border))
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |button, _, _, cx| {
                    cx.stop_propagation();
                    button.set_open(!open, cx);
                }),
            )
            .child(bar())
            .child(bar())
            .child(bar());
        let items = open.then(|| deferred(self.render_items(colors, window, cx)));
        div()
            .relative()
            .flex_none()
            .mx_1()
            .text_sm()
            .child(button)
            .children(items)
    }
}
