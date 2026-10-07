//! 管理窗口壳（§2 窗口模型 / §4.3）：44px 顶栏（产品名 + 三页签 + 工具钮）
//! + 键盘模型（1/2/3、←/→、F、Esc、Cmd/Ctrl+,）。

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::gpui::prelude::FluentBuilder as _;
use gpui_kit::gpui::{
    div, px, AnyElement, App, AppContext as _, Context, FocusHandle, Focusable, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window, WindowControlArea,
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
            Tab::Library => TAB_LIBRARY(),
            Tab::Monitors => TAB_MONITORS(),
            Tab::Settings => TAB_SETTINGS(),
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
                h_flex()
                    .gap_1()
                    .child(tab.label())
                    .child(div().text_size(px(10.)).text_color(t.text2).opacity(0.7)),
            )
            .into_any_element()
    }

    /// Windows 标题栏按钮（– □ ✕）：ghost 变体给悬停/按压反馈，行为走窗口方法
    /// 而非 WindowControlArea——控制区交给系统会丢失 gpui 的悬停视觉，且双击
    /// 最大化 / 吸附已由 Drag 区覆盖，这里要的是可控的视觉态。
    #[cfg(target_os = "windows")]
    fn caption_button(
        id: &'static str,
        icon: IconName,
        tooltip: &'static str,
        action: impl Fn(&gpui_kit::gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Button {
        Button::new(id)
            .ghost()
            .icon(Icon::new(icon))
            .tooltip(tooltip)
            .on_click(action)
    }

    fn topbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        // 88 = 红绿灯簇右缘（position 14,15 → 簇占 14..66）+ 22px 呼吸
        // （对齐 Apple 自家 44px 工具条的留白节奏；76 曾只隔 10px，局促）
        let left_pad = if cfg!(target_os = "macos") {
            px(88.)
        } else {
            px(14.)
        };

        let theme_toggle = Button::new("btn-theme")
            .ghost()
            .icon(Icon::new(IconName::Moon))
            .tooltip(TIP_TOGGLE_THEME())
            .on_click(|_, _, cx| super::theme::toggle(cx));

        // Windows 标题栏按钮（– □ ✕，规格 §2/§4.3 顶栏「窗口控制」的实现欠账）：
        // 点击回调走窗口方法（悬停态由 gpui 正常渲染）；窗口拖拽/双击最大化/边缘
        // 吸附由下方 Drag 控制区交给系统（HTCAPTION），两者职责分离。macOS 的
        // 红绿灯是系统在内容层之上绘制的，不受影响——无需此组按钮。
        // 用编译期 cfg（不是运行时 cfg!）：caption_button 本身只在 Windows 存在，
        // 调用点也必须只在 Windows 编译，否则 macOS 类型检查会找不到该关联函数。
        #[cfg(target_os = "windows")]
        let caption = Some(
            h_flex()
                .items_center()
                .child(Self::caption_button(
                    "btn-win-min",
                    IconName::Minus,
                    super::strings::t().win_minimize,
                    |_, window, _| window.minimize_window(),
                ))
                .child(Self::caption_button(
                    "btn-win-max",
                    IconName::Square,
                    super::strings::t().win_maximize,
                    |_, window, _| window.zoom_window(),
                ))
                .child(Self::caption_button(
                    "btn-win-close",
                    IconName::X,
                    super::strings::t().win_close,
                    |_, window, _| window.remove_window(),
                )),
        );
        #[cfg(not(target_os = "windows"))]
        let caption: Option<gpui_kit::Div> = None;

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
                // 品牌区 = 窗口拖拽区（无交互子元素；系统经 WM_NCHITTEST →
                // HTCAPTION 提供拖拽 / 双击最大化 / Win+方向键吸附）
                h_flex()
                    .gap_2()
                    .items_center()
                    .window_control_area(WindowControlArea::Drag)
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
                            .child(APP_NAME()),
                    ),
            )
            .child(h_flex().ml_2().gap(px(2.)).children([
                self.tab_button(Tab::Library, cx),
                self.tab_button(Tab::Monitors, cx),
                self.tab_button(Tab::Settings, cx),
            ]))
            .child(
                // 弹性空档 = 第二拖拽区（覆盖页签右侧到工具钮之间的全部空区）
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(theme_toggle)
            .children(caption)
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
