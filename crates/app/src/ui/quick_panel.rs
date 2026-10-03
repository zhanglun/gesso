//! 托盘快速面板（§4.1，signature #3）：左键托盘 → 360×280 弹出小窗。
//! 高频操作的家：点屏缩略图 = 暂停/恢复；点最近缩略图 = 设为主显示器（两击完成）。

use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::gpui::prelude::FluentBuilder as _;
use gpui_kit::gpui::{
    div, px, ClickEvent, Context, FocusHandle, Focusable, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Window,
};

use super::app_state::{bridge_assign, state, update};
use super::strings::{PANEL_ALL, PANEL_MONITORS, PANEL_RECENT};
use super::theme::tokens;
use super::widgets::{play_state_visual, preview};

pub struct QuickPanel {
    focus: FocusHandle,
}

impl Focusable for QuickPanel {
    fn focus_handle(&self, _cx: &gpui_kit::gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl QuickPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        QuickPanel { focus }
    }
}

impl Render for QuickPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let g = state(cx);

        // 显示器区：每屏一块 16:9 缩略图，点击 = 暂停/恢复
        let monitors: Vec<_> = g
            .monitors
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let (icon, _, icon_color) = play_state_visual(m.state, cx);
                let paused = m.state.paused();
                let (thumb, real_id, name) = {
                    let item = m
                        .wallpaper
                        .as_ref()
                        .and_then(|wid| g.library.iter().find(|it| &it.id == wid));
                    (
                        match item {
                            Some(item) => {
                                preview(item.art, Some(item.kind), item.broken, false, cx)
                            }
                            None => preview(
                                super::data::Art {
                                    from: 0x26262A,
                                    to: 0x1E1E20,
                                },
                                None,
                                false,
                                false,
                                cx,
                            ),
                        },
                        m.real_id.clone(),
                        m.short.to_string(),
                    )
                };
                div()
                    .id(SharedString::from(format!("panel-m{i}")))
                    .w(px(104.))
                    .cursor_pointer()
                    .child(
                        div()
                            .relative()
                            .overflow_hidden()
                            .rounded(px(6.))
                            .child(thumb)
                            .child(
                                div()
                                    .absolute()
                                    .top_1()
                                    .left_1()
                                    .text_color(icon_color)
                                    .child(Icon::new(icon).size_3()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.text2)
                            .mt_1()
                            .child(name),
                    )
                    .on_click({
                        move |_, window, cx| {
                            if !real_id.is_empty() {
                                crate::engine::enqueue(crate::engine::EngineAction::PauseOne {
                                    monitor_id: real_id.clone(),
                                    paused: !paused,
                                });
                            }
                            update(window, cx, |_| {});
                        }
                    })
                    .into_any_element()
            })
            .collect();

        // 最近使用：库前 6 个，点击 = 设为主显示器（两击换壁纸）
        let main_idx = g.main_monitor();
        let recent: Vec<_> = g
            .library
            .iter()
            .filter(|w| !w.broken)
            .take(6)
            .cloned()
            .enumerate()
            .map(|(i, item)| {
                div()
                    .id(SharedString::from(format!("panel-r{i}")))
                    .size(px(56.))
                    .overflow_hidden()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.shadow_sm())
                    .child(preview(item.art, Some(item.kind), false, false, cx))
                    .on_click(move |_: &ClickEvent, window, cx| {
                        let _ = bridge_assign(window, cx, main_idx, item.id.as_ref());
                        // 换完即收（任务完成）
                        window.remove_window();
                    })
                    .into_any_element()
            })
            .collect();

        v_flex()
            .key_context("GessoQuickPanel")
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|_, event: &gpui_kit::gpui::KeyDownEvent, window, _| {
                    // Esc 关闭弹出层（§3 键盘模型）
                    if event.keystroke.key == "escape" {
                        window.remove_window();
                    }
                }),
            )
            .size_full()
            .bg(t.elevated)
            .rounded(px(8.))
            .shadow_lg()
            .p_3()
            .gap_2()
            .text_color(t.text1)
            .when(!monitors.is_empty(), |r| {
                r.child(section_title(PANEL_MONITORS, None, cx))
                    .child(h_flex().gap_2().flex_wrap().children(monitors))
            })
            .child(section_title(PANEL_RECENT, Some((PANEL_ALL, false)), cx))
            .child(h_flex().gap_2().flex_wrap().min_h(px(56.)).children(recent))
    }
}

fn section_title(
    title: &'static str,
    link: Option<(&'static str, bool)>,
    cx: &Context<QuickPanel>,
) -> gpui_kit::gpui::AnyElement {
    let t = tokens(cx);
    h_flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.text2)
                .child(title),
        )
        .child(div().flex_1())
        .when_some(link, |r, (label, _)| {
            r.child(
                div()
                    .id(SharedString::from(format!("panel-link-{label}")))
                    .text_size(px(12.))
                    .text_color(t.accent)
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.8))
                    .on_click(|_, window, cx| {
                        // 全部 → 打开主窗口库页
                        update(window, cx, |g| {
                            g.active_tab = super::shell::Tab::Library;
                        });
                        crate::engine::enqueue(crate::engine::EngineAction::FocusMainWindow);
                    })
                    .child(label),
            )
        })
        .into_any_element()
}
