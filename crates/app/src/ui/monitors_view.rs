//! 显示器页（§4.4）：拓扑图（按工作区坐标等比排布）+ 屏卡片 +
//! 投放指派（signature #2 的本页落点）+ 新显示器提示。

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::IndexPath;
use gpui_kit::component::{h_flex, v_flex, Icon, WindowExt as _};
use gpui_kit::gpui::prelude::FluentBuilder as _;
use gpui_kit::gpui::{
    div, px, AnyElement, AppContext as _, BorrowAppContext as _, Context, Entity, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Window,
};

use super::app_state::{state, update, CardDrag, GessoState};
use super::data::MonitorEntry;
use super::strings::*;
use super::theme::tokens;
use super::widgets::{play_state_visual, preview};

/// 屏卡片除缩略图外的固定 chrome 高度估算（head + 状态行 + 控件行 + 内边距 + 底座）。
const FRAME_CHROME: f32 = 122.;
/// 拓扑缩放系数：主屏（2560 逻辑宽）映射为 320px。
const TOPO_SCALE: f32 = 0.125;

pub struct MonitorsView {
    /// 每块屏一个帧率下拉（§4.4：FPS 下拉 60/30/15/5）。
    fps_selects: Vec<Entity<SelectState<SearchableVec<String>>>>,
}

impl MonitorsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let monitors_snapshot: Vec<super::data::MonitorEntry> = state(cx).monitors.clone();
        let fps_selects = monitors_snapshot
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let items = SearchableVec::new(
                    super::data::FPS_OPTIONS
                        .map(|f| format!("{f} fps"))
                        .to_vec(),
                );
                let selected = super::data::FPS_OPTIONS
                    .iter()
                    .position(|f| *f == m.fps)
                    .unwrap_or(0);
                let entity = cx
                    .new(|cx| SelectState::new(items, Some(IndexPath::new(selected)), window, cx));
                cx.subscribe(
                    &entity,
                    move |_, _, event: &SelectEvent<SearchableVec<String>>, cx| {
                        if let SelectEvent::Confirm(Some(v)) = event {
                            if let Ok(fps) = v.trim_end_matches(" fps").parse::<u32>() {
                                cx.update_global::<GessoState, _>(|g, _| {
                                    if let Some(m) = g.monitors.get_mut(i) {
                                        m.fps = fps;
                                    }
                                });
                                cx.notify();
                            }
                        }
                    },
                )
                .detach();
                entity
            })
            .collect();
        MonitorsView { fps_selects }
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let redetect = Button::new("btn-redetect")
            .label(BTN_REDETECT)
            .secondary()
            .icon(Icon::new(IconName::RefreshCw))
            .on_click(|_, window, cx| {
                // 重新检测：走引擎动作队列（sync_monitors 幂等；API.md §1/§4）
                crate::engine::enqueue(crate::engine::EngineAction::SyncMonitors);
                update(window, cx, |g| g.redetect());
                window.push_notification(Notification::info(TOAST_REDETECT), cx);
            });
        h_flex()
            .flex_none()
            .h(px(48.))
            .px(px(14.))
            .gap(px(10.))
            .items_center()
            .border_b_1()
            .border_color(t.hairline)
            .child(redetect)
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text2)
                    .child(MONITOR_HOVER_NOTE),
            )
            .into_any_element()
    }

    fn monitor_frame(
        &self,
        i: usize,
        m: &MonitorEntry,
        fixed_w: Option<f32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = tokens(cx);
        let glow = state(cx)
            .hovered
            .as_ref()
            .zip(m.wallpaper.as_ref())
            .is_some_and(|(h, w)| h == w);
        let (dz_accent, dz_accent_soft) = (t.accent, t.accent_soft);
        let item = m
            .wallpaper
            .as_ref()
            .and_then(|wid| state(cx).library.iter().find(|it| &it.id == wid))
            .cloned();

        // 状态行：壁纸名 · 状态图标 · fps（§4.4；与托盘角标一致）
        let (st_icon, st_label, st_color) = match (&item, m.state) {
            (None, _) => (None, ST_UNASSIGNED.to_string(), t.text2),
            (Some(item), s) => {
                let (icon, label, color) = play_state_visual(s, cx);
                (Some(icon), format!("{} · {}", item.name, label), color)
            }
        };
        let fps_text = if m.state.paused() {
            "0 fps".to_string()
        } else {
            format!("{} fps", m.fps)
        };

        let mut frame = div()
            .id(SharedString::from(format!("monitor-{i}")))
            .relative()
            .bg(t.surface)
            .border_2()
            .border_color(if glow { t.accent } else { t.hairline2 })
            .rounded(px(8.))
            .p(px(10.))
            .min_w(px(240.))
            .when(glow, |d| d.shadow_sm())
            .drag_over::<CardDrag>(move |s, _, _, _| {
                s.border_color(dz_accent).border_dashed().bg(dz_accent_soft)
            })
            .on_drop(move |drag: &CardDrag, window, cx| {
                super::library_view::assign_from_drop(&drag.item_id, i, window, cx);
            })
            .on_hover({
                // 悬停本页卡片 → 描边对应显示器（in-window 版 signature #1）
                let wallpaper = m.wallpaper.clone();
                cx.listener(move |_, hovering: &bool, window, cx| {
                    update(window, cx, |g| {
                        g.hovered = if *hovering {
                            wallpaper.clone()
                        } else if g.hovered == wallpaper {
                            None
                        } else {
                            g.hovered.clone()
                        };
                    });
                })
            })
            .flex()
            .flex_col();

        // 拓扑模式给固定宽；无则自然宽
        if let Some(w) = fixed_w {
            frame = frame.w(px(w));
        }

        let head = h_flex()
            .items_baseline()
            .gap_2()
            .mb_2()
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.text1)
                    .child(m.name.to_string()),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text2)
                    .font_features(super::widgets::tabular())
                    .child(m.label.to_string()),
            );

        let thumb = div().mb_2().child(match &item {
            Some(item) => preview(
                item.art,
                Some(item.kind),
                item.broken,
                false,
                item.thumb.as_deref(),
                cx,
            ),
            None => preview(
                super::data::Art {
                    from: 0x26262A,
                    to: 0x1E1E20,
                },
                None,
                false,
                false,
                None,
                cx,
            ),
        });

        let status = h_flex()
            .items_center()
            .gap_2()
            .mb(px(10.))
            .text_size(px(12.))
            .font_features(super::widgets::tabular())
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .text_color(st_color)
                    .when_some(st_icon, |r, icon| {
                        r.child(Icon::new(icon).size_3().into_any_element())
                    })
                    .child(st_label),
            )
            .child(div().ml_auto().text_color(t.text2).child(fps_text));

        // 控件行：[更换][暂停/恢复] + FPS 下拉（右）
        let paused = m.state.paused();
        let current_wallpaper = m.wallpaper.clone();
        let change = Button::new(SharedString::from(format!("change-{i}")))
            .label(BTN_CHANGE)
            .secondary()
            .compact()
            .on_click(move |_, window, cx| {
                // 跳库页（选中当前壁纸作为上下文）
                update(window, cx, |g| {
                    g.selected = current_wallpaper.clone();
                });
                super::shell::switch_tab(window, cx, super::shell::Tab::Library);
                window.push_notification(Notification::info(TOAST_CHANGE_HINT), cx);
            });
        let toggle = Button::new(SharedString::from(format!("toggle-{i}")))
            .label(if paused { BTN_RESUME } else { BTN_PAUSE })
            .secondary()
            .compact()
            .on_click({
                let real_id = m.real_id.clone();
                move |_, window, cx| {
                    if real_id.is_empty() {
                        // 演示数据：无真实会话，仅切换投影
                        update(window, cx, |g| g.toggle_pause(i));
                    } else {
                        // 真源：引擎 pause_one（≤150ms 执行，快照回灌刷新状态行）
                        crate::engine::enqueue(crate::engine::EngineAction::PauseOne {
                            monitor_id: real_id.clone(),
                            paused: !paused,
                        });
                    }
                }
            });
        let fps_select = self
            .fps_selects
            .get(i)
            .map(|s| {
                div()
                    .w(px(96.))
                    .child(Select::new(s).appearance(true))
                    .into_any_element()
            })
            .unwrap_or_else(|| div().into_any_element());

        let ctl = h_flex()
            .items_center()
            .gap_2()
            .child(change)
            .child(toggle)
            .child(div().flex_1())
            .child(fps_select);

        let stand = div()
            .mx_auto()
            .mt_1()
            .w(px(56.))
            .h(px(6.))
            .rounded_b(px(4.))
            .bg(t.hairline2);

        frame
            .child(head)
            .child(thumb)
            .child(status)
            .child(ctl)
            .child(stand)
            .into_any_element()
    }

    fn topo(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let monitors = state(cx).monitors.clone();
        // 工作区包围盒 → 等比缩放（§4.4 拓扑图按坐标等比排布）
        let scale = TOPO_SCALE;
        let max_right = monitors
            .iter()
            .map(|m| (m.rect.0 + m.rect.2) * scale)
            .fold(0.0f32, f32::max);
        let max_bottom = monitors
            .iter()
            .map(|m| m.rect.1 * scale + FRAME_CHROME + m.rect.2 * scale * 9. / 16.)
            .fold(0.0f32, f32::max);

        // 单屏时工作区原点即窗口原点（0,0）——直接流式布局居中；
        // 多屏按工作区坐标绝对排布（保留物理相对位置）
        let single = monitors.len() == 1;
        let frames: Vec<AnyElement> = monitors
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let w = m.rect.2 * scale;
                let frame = self.monitor_frame(i, m, Some(w), cx);
                if single {
                    div().child(frame).into_any_element()
                } else {
                    div()
                        .absolute()
                        .left(px(m.rect.0 * scale))
                        .top(px(m.rect.1 * scale))
                        .child(frame)
                        .into_any_element()
                }
            })
            .collect();

        let topo_box = if single {
            div()
                .p(px(18.))
                .border_1()
                .border_color(t.hairline)
                .rounded(px(12.))
                .bg(t.panel)
                .children(frames)
        } else {
            div()
                .relative()
                .p(px(18.))
                .border_1()
                .border_color(t.hairline)
                .rounded(px(12.))
                .bg(t.panel)
                .h(px(max_bottom + 36.))
                .w(px(max_right + 36.))
                .children(frames)
        };
        topo_box.into_any_element()
    }

    fn notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !state(cx).pending_new_monitor {
            return None;
        }
        let t = tokens(cx);
        Some(
            h_flex()
                .mx_auto()
                .items_center()
                .gap_2()
                .px(px(14.))
                .py_2()
                .rounded(px(8.))
                .bg(t.accent_soft)
                .text_size(px(12.))
                .text_color(t.text1)
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size_4()
                        .into_any_element(),
                )
                .child(NOTICE_NEW_MONITOR)
                .child(
                    Button::new("go-assign")
                        .label(BTN_GO_ASSIGN)
                        .text()
                        .compact()
                        .on_click(|_, window, cx| {
                            super::shell::switch_tab(window, cx, super::shell::Tab::Library);
                        }),
                )
                .into_any_element(),
        )
    }
}

impl Render for MonitorsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let toolbar = self.toolbar(cx);
        let topo = self.topo(cx);
        let notice = self.notice(cx);
        v_flex()
            .size_full()
            .text_color(t.text1)
            .child(toolbar)
            .child(
                div()
                    .id("monitors-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .min_h_0()
                    .child(
                        v_flex()
                            .py(px(20.))
                            .px(px(14.))
                            .gap_4()
                            .child(topo)
                            .when_some(notice, |r, n| r.child(n))
                            .child(
                                div()
                                    .mx_auto()
                                    .text_size(px(12.))
                                    .text_color(t.text2)
                                    .child(EDID_NOTE),
                            ),
                    ),
            )
    }
}
