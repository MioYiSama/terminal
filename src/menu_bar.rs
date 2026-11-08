//! In-window menu bar. GPUI turns `cx.set_menus` into a native menu bar only
//! on macOS; on Windows and Linux it just stores the menus, so terminal
//! windows draw them here.

use gpui::{
    Context, MouseButton, OwnedMenuItem, Pixels, Render, Window, deferred, div,
    prelude::*, px,
};

use crate::settings::Settings;
use crate::theme::UiColors;

pub const ENABLED: bool = !cfg!(target_os = "macos");
/// The bar sits at the top of the window, so clicks above this line hit it.
const HEIGHT: Pixels = px(28.);

#[derive(Default)]
pub struct MenuBar {
    /// Index of the open menu.
    open: Option<usize>,
}

impl MenuBar {
    fn set_open(&mut self, open: Option<usize>, cx: &mut Context<Self>) {
        if self.open != open {
            self.open = open;
            cx.notify();
        }
    }

    fn render_items(
        &self,
        items: Vec<OwnedMenuItem>,
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
            // Clicks on the bar are left to its titles, which switch or
            // close menus themselves.
            .on_mouse_down_out(cx.listener(|bar, event: &gpui::MouseDownEvent, _, cx| {
                if event.position.y >= HEIGHT {
                    bar.set_open(None, cx);
                }
            }))
            .children(items.into_iter().enumerate().filter_map(|(ix, item)| match item {
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
                            .on_click(cx.listener(move |bar, _, window, cx| {
                                bar.set_open(None, cx);
                                window.dispatch_action(action.boxed_clone(), cx);
                            }))
                            .child(name)
                            .children(shortcut.map(|keys| div().text_color(colors.muted).child(keys)))
                            .into_any_element(),
                    )
                }
                // Submenus and OS-provided menus are unused here.
                OwnedMenuItem::Submenu(_) | OwnedMenuItem::SystemMenu(_) => None,
            }))
    }
}

impl Render for MenuBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = UiColors::new(Settings::get(cx).flavor(window.appearance()));
        let menus = cx.get_menus().unwrap_or_default();
        let mut titles = Vec::with_capacity(menus.len());
        for (ix, menu) in menus.into_iter().enumerate() {
            let open = self.open == Some(ix);
            let title = div()
                .id(ix)
                .px_2()
                .py(px(2.))
                .rounded_sm()
                .map(|title| {
                    if open {
                        title.bg(colors.hover)
                    } else {
                        title.hover(|style| style.bg(colors.border))
                    }
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |bar, _, _, cx| {
                        cx.stop_propagation();
                        bar.set_open((bar.open != Some(ix)).then_some(ix), cx);
                    }),
                )
                // Like native menu bars, hovering another title while a menu
                // is open switches to it.
                .on_hover(cx.listener(move |bar, hovered: &bool, _, cx| {
                    if *hovered && bar.open.is_some() {
                        bar.set_open(Some(ix), cx);
                    }
                }))
                .child(menu.name);
            let items = open.then(|| deferred(self.render_items(menu.items, colors, window, cx)));
            titles.push(div().relative().child(title).children(items));
        }

        div()
            .id("menu-bar")
            .flex()
            .flex_none()
            .items_center()
            .h(HEIGHT)
            .px_1()
            .bg(colors.panel)
            .border_b_1()
            .border_color(colors.border)
            .text_sm()
            .text_color(colors.text)
            .on_mouse_down(MouseButton::Left, cx.listener(|bar, _, _, cx| bar.set_open(None, cx)))
            .children(titles)
    }
}
