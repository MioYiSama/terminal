//! A terminal window: a tab bar over the active tab's terminal, plus the
//! in-window menu bar on Windows and Linux.

use gpui::{
    Context, Entity, MouseButton, Pixels, SharedString, Subscription, Window, actions, div,
    prelude::*, px,
};

use crate::menu_bar::{self, MenuBar};
use crate::settings::Settings;
use crate::theme::UiColors;
use crate::view::{TerminalEvent, TerminalView};

actions!(workspace, [NewTab, CloseTab, NextTab, PreviousTab]);

const TAB_BAR_HEIGHT: Pixels = px(32.);
const TAB_MAX_WIDTH: Pixels = px(220.);
/// Label for tabs whose program has not set a title.
const DEFAULT_TITLE: &str = "Terminal";

struct Tab {
    view: Entity<TerminalView>,
    _subscription: Subscription,
}

pub struct Workspace {
    /// Never empty: closing the last tab closes the window.
    tabs: Vec<Tab>,
    active: usize,
    menu_bar: Option<Entity<MenuBar>>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut workspace = Self {
            tabs: Vec::new(),
            active: 0,
            menu_bar: menu_bar::ENABLED.then(|| cx.new(|_| MenuBar::default())),
            _subscriptions: vec![
                cx.observe_global_in::<Settings>(window, |_, _, cx| cx.notify()),
                cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            ],
        };
        workspace.new_tab(window, cx);
        workspace
    }

    fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.new(|cx| TerminalView::new(window, cx));
        let subscription = cx.subscribe_in(&view, window, Self::terminal_event);
        self.tabs.push(Tab {
            view,
            _subscription: subscription,
        });
        self.activate(self.tabs.len() - 1, window, cx);
    }

    fn terminal_event(
        &mut self,
        view: &Entity<TerminalView>,
        event: &TerminalEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.tabs.iter().position(|tab| tab.view == *view) else {
            return;
        };
        match event {
            TerminalEvent::TitleChanged => {
                if ix == self.active {
                    self.update_window_title(window, cx);
                }
                cx.notify();
            }
            TerminalEvent::Exited => self.close_tab(ix, window, cx),
        }
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.active = ix;
        self.tabs[ix].view.read(cx).focus(window);
        self.update_window_title(window, cx);
        cx.notify();
    }

    /// Close tab `ix`, activating its right neighbor (or the new last tab)
    /// if it was active. Closing the last tab closes the window.
    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.remove(ix);
        if self.tabs.is_empty() {
            window.remove_window();
            return;
        }
        let active = if ix < self.active {
            self.active - 1
        } else {
            self.active.min(self.tabs.len() - 1)
        };
        self.activate(active, window, cx);
    }

    fn cycle(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.tabs.len();
        let ix = if forward {
            (self.active + 1) % len
        } else {
            (self.active + len - 1) % len
        };
        self.activate(ix, window, cx);
    }

    fn title(&self, ix: usize, cx: &Context<Self>) -> SharedString {
        let title = self.tabs[ix].view.read(cx).title();
        if title.is_empty() {
            DEFAULT_TITLE.into()
        } else {
            title.clone()
        }
    }

    fn update_window_title(&self, window: &mut Window, cx: &Context<Self>) {
        window.set_window_title(&self.title(self.active, cx));
    }

    fn render_tab_bar(&self, colors: UiColors, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = (0..self.tabs.len()).map(|ix| {
            let active = ix == self.active;
            let close = div()
                .id("close")
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .size(px(18.))
                .rounded_sm()
                .hover(|style| style.bg(colors.hover))
                .on_click(cx.listener(move |workspace, _, window, cx| {
                    // Keep the click from activating the tab being closed.
                    cx.stop_propagation();
                    workspace.close_tab(ix, window, cx)
                }))
                .child("×");
            div()
                .id(ix)
                .flex()
                .flex_1()
                .min_w_0()
                .max_w(TAB_MAX_WIDTH)
                .h_full()
                .items_center()
                .gap_1()
                .pl_3()
                .pr_1()
                .border_r_1()
                .border_color(colors.border)
                .map(|tab| {
                    if active {
                        tab.bg(colors.background).text_color(colors.text)
                    } else {
                        tab.text_color(colors.muted)
                            .hover(|style| style.bg(colors.border))
                    }
                })
                .on_click(cx.listener(move |workspace, _, window, cx| {
                    workspace.activate(ix, window, cx)
                }))
                .on_mouse_up(
                    MouseButton::Middle,
                    cx.listener(move |workspace, _, window, cx| workspace.close_tab(ix, window, cx)),
                )
                .child(div().flex_1().min_w_0().truncate().child(self.title(ix, cx)))
                .child(close)
        });
        let new_tab = div()
            .id("new-tab")
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .size(px(24.))
            .mx_1()
            .rounded_sm()
            .text_color(colors.muted)
            .hover(|style| style.bg(colors.border).text_color(colors.text))
            .on_click(cx.listener(|workspace, _, window, cx| workspace.new_tab(window, cx)))
            .child("+");
        div()
            .id("tab-bar")
            .flex()
            .flex_none()
            .items_center()
            .h(TAB_BAR_HEIGHT)
            .bg(colors.panel)
            .border_b_1()
            .border_color(colors.border)
            .text_sm()
            .children(tabs)
            .child(new_tab)
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = UiColors::new(Settings::get(cx).flavor(window.appearance()));
        div()
            .size_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(|workspace, _: &NewTab, window, cx| workspace.new_tab(window, cx)))
            .on_action(cx.listener(|workspace, _: &CloseTab, window, cx| {
                workspace.close_tab(workspace.active, window, cx)
            }))
            .on_action(cx.listener(|workspace, _: &NextTab, window, cx| {
                workspace.cycle(true, window, cx)
            }))
            .on_action(cx.listener(|workspace, _: &PreviousTab, window, cx| {
                workspace.cycle(false, window, cx)
            }))
            .children(self.menu_bar.clone())
            .child(self.render_tab_bar(colors, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.tabs[self.active].view.clone()),
            )
    }
}
