//! 管理窗口壳（§2 窗口模型 / §4.3）：44px 顶栏（产品名 + 三页签 + 工具钮）
//! + 键盘模型（1/2/3、←/→、F、Esc、Cmd/Ctrl+,）。

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::gpui::prelude::FluentBuilder as _;
use gpui_kit::gpui::{
    div, px, AnyElement, App, AppContext as _, Context, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window,
};

use super::app_state::state;
use super::library_view::LibraryView;
use super::monitors_view::MonitorsView;
use super::settings_view::SettingsView;
use super::strings::*;
use super::theme::tokens;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Library,
    Monitors,
    Settings,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Tab::Library => TAB_LIBRARY,
            Tab::Monitors => TAB_MONITORS,
            Tab::Settings => TAB_SETTINGS,
        }
    }

    fn next(self) -> Self {
        match self {
            Tab::Library => Tab::Monitors,
            Tab::Monitors => Tab::Settings,
            Tab::Settings => Tab::Library,
        }
    }

    fn prev(self) -> Self {
        match self {
            Tab::Library => Tab::Settings,
            Tab::Monitors => Tab::Library,
            Tab::Settings => Tab::Monitors,
        }
    }

    pub fn from_index(i: usize) -> Option<Self> {
        [Tab::Library, Tab::Monitors, Tab::Settings].get(i).copied()
    }
}

/// 页签切换的唯一入口（键盘 / 点击 / 跨页动作共用）。
pub fn switch_tab(window: &mut Window, cx: &mut App, tab: Tab) {
    super::app_state::update(window, cx, |g| g.active_tab = tab);
}

pub struct Shell {
    focus: FocusHandle,
    library: gpui_kit::gpui::Entity<LibraryView>,
    monitors: gpui_kit::gpui::Entity<MonitorsView>,
    settings: gpui_kit::gpui::Entity<SettingsView>,
}

impl Focusable for Shell {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library = cx.new(|cx| LibraryView::new(window, cx));
        let monitors = cx.new(|cx| MonitorsView::new(window, cx));
        let settings = cx.new(|cx| SettingsView::new(window, cx));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Shell {
            focus,
            library,
            monitors,
            settings,
        }
    }

    fn tab_button(&self, tab: Tab, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let selected = state(cx).active_tab == tab;
        div()
            .id(SharedString::from(format!("tab-{}", tab.label())))
            .px(px(14.))
            .py(px(6.))
            .rounded(px(6.))
            .text_size(px(13.))
            .text_color(if selected { t.accent } else { t.text2 })
            .when(selected, |d| {
                d.bg(t.accent_soft).font_weight(FontWeight::MEDIUM)
            })
            .hover(|s| {
                if selected {
                    s
                } else {
                    s.bg(t.panel).text_color(t.text1)
                }
            })
            .on_click(move |_, window, cx| switch_tab(window, cx, tab))
            // 页签与键位提示（§3 键盘模型：1/2/3 切页签）
            .child(
                h_flex().gap_1().child(tab.label()).child(
                    div()
                        .text_size(px(10.))
                        .text_color(t.text2)
                        .opacity(0.7)
                ),
            )
            .into_any_element()
    }

    fn topbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let left_pad = if cfg!(target_os = "macos") {
            px(76.)
        } else {
            px(14.)
        };

        // 原型的「重放首启向导」钮：页面验收用（正式版 = 首启一次性弹出）
        let wizard = Button::new("btn-wizard")
            .ghost()
            .icon(Icon::new(IconName::RotateCcw))
            .tooltip("重放首启向导")
            .on_click(|_, _, cx| {
                let _ = super::first_run::FirstRun::open(cx);
            });

        let theme_toggle = Button::new("btn-theme")
            .ghost()
            .icon(Icon::new(IconName::Moon))
            .tooltip("切换亮 / 暗主题")
            .on_click(|_, _, cx| super::theme::toggle(cx));

        h_flex()
            .flex_none()
            .h(px(44.))
            .pl(left_pad)
            .pr(px(14.))
            .gap(px(14.))
            .items_center()
            .border_b_1()
            .border_color(t.hairline)
            .bg(t.surface)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_color(t.text1)
                            .child(Icon::new(IconName::Wallpaper).size_4()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text1)
                            .child(APP_NAME),
                    ),
            )
            .child(h_flex().ml_2().gap(px(2.)).children([
                self.tab_button(Tab::Library, cx),
                self.tab_button(Tab::Monitors, cx),
                self.tab_button(Tab::Settings, cx),
            ]))
            .child(div().flex_1())
            .child(wizard)
            .child(theme_toggle)
            .into_any_element()
    }

    /// 键盘模型（§3）：Tab 走焦点环；1/2/3 与 ←/→ 切页签；F 聚焦搜索；
    /// Esc 清搜索；Cmd/Ctrl+, 进设置。输入框聚焦时按键由输入框消化，不会误触。
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let cmd = event.keystroke.modifiers.platform;
        let active = state(cx).active_tab;
        match key {
            "1" | "2" | "3" => {
                if let Some(tab) = Tab::from_index(key.to_string().parse::<usize>().unwrap() - 1) {
                    switch_tab(window, cx, tab);
                }
            }
            "left" => switch_tab(window, cx, active.prev()),
            "right" => switch_tab(window, cx, active.next()),
            "f" => {
                switch_tab(window, cx, Tab::Library);
                self.library.update(cx, |v, cx| v.focus_search(window, cx));
            }
            "escape" => {
                self.library.update(cx, |v, cx| v.clear_search(window, cx));
            }
            "," if cmd => switch_tab(window, cx, Tab::Settings),
            _ => {}
        }
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let active = state(cx).active_tab;
        v_flex()
            .key_context("GessoShell")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .bg(t.surface)
            .text_size(px(13.))
            .text_color(t.text1)
            .child(self.topbar(cx))
            .when(active == Tab::Library, |d| d.child(self.library.clone()))
            .when(active == Tab::Monitors, |d| d.child(self.monitors.clone()))
            .when(active == Tab::Settings, |d| d.child(self.settings.clone()))
    }
}
