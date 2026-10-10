//! 显示器页（§4.4 桌面沙盘 + 详情条）。
//!
//! 方向契约（2026-10-04 改版，规格同步见 docs/design/界面与交互设计.md §4.4）：
//! THESIS——桌面本身是主角：屏不是表单卡片，而是等比"屏幕小像"铺在 desk 渐变底上
//! （macOS / Windows 显示器设置的排布语法）；控件一律不进小像（v1 卡片在拓扑缩放下
//! 控件不可用，是本次改版动因）。
//! OWN-WORLD——只用 DESIGN.md token；层级靠真实投影与留白表达，不做彩色 halo。
//! STORY——一眼看清哪块屏、在哪、放什么、什么状态；操作在底部详情条完成。
//! FIRST VIEWPORT——满版沙盘（顶栏：悬停提示 + 重新检测）+ 居中等比主屏 + 详情条；
//! 新显示器横幅浮于画布顶部。

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::IndexPath;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable as _, WindowExt as _};
// gpui::Size（画布几何）已被导入；kit 控件档位用别名
use gpui_kit::component::Size as KitSize;
use gpui_kit::gpui::prelude::FluentBuilder as _;
use gpui_kit::gpui::{
    canvas, div, linear_color_stop, linear_gradient, point, px, quad, relative, rgb, size,
    AnyElement, AppContext as _, BorderStyle, BorrowAppContext as _, Bounds, Context, Entity,
    FontWeight, InteractiveElement as _, IntoElement, ObjectFit, ParentElement, Pixels, Render,
    SharedString, Size, StatefulInteractiveElement as _, Styled, Window,
};

use super::app_state::{state, update, CardDrag, GessoState};
use super::data::MonitorEntry;
use super::strings::*;
use super::theme::tokens;
use super::widgets::{badge, empty_art, play_state_visual, tabular};

/// 壳层顶栏高度（shell.rs 与此处的画布高度推算耦合）。
const TITLEBAR_H: f32 = 44.;
/// 本页上下内边距。
const PAGE_PAD: f32 = 14.;
/// 画布与详情条的间距。
const PAGE_GAP: f32 = 10.;
/// 详情条固定高度。
const STRIP_H: f32 = 64.;
/// 画布顶部信息条高度（提示 + 重新检测）。
const DESK_HEAD_H: f32 = 44.;
/// 沙盘内容四边留白。
const DESK_PAD: f32 = 24.;
/// 小像缩放上限（单屏不至于顶满画布）。
const MAX_SCALE: f32 = 0.30;
/// 点阵间距（排布工具的图纸语义）。
const DOT_STEP: f32 = 22.;
/// 小像内容圆角 = 容器圆角 10 − 边框 2（这版 gpui 的 overflow_hidden
/// 只做矩形裁剪，内容须自圆角——widgets::preview 同款解法）。
const MINI_INNER_R: f32 = 8.;

pub struct MonitorsView {
    /// 每块屏一个帧率下拉（§4.4：FPS 下拉 60/30/15/5）。
    fps_selects: Vec<Entity<SelectState<SearchableVec<String>>>>,
    /// 详情条展示的屏；None = 跟随主屏。
    selected: Option<usize>,
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
                // 真 UI 控件（§4.4 FPS 下拉）：确认即入队引擎动作，热重载该屏宿主页。
                // 只改本地投影会导致引擎侧 fps 与 UI 永久漂移（SetMonitorFps 曾
                // 因此从未被构造）。
                let real_id = m.real_id.clone();
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
                                crate::engine::enqueue(
                                    crate::engine::EngineAction::SetMonitorFps {
                                        monitor_id: real_id.clone(),
                                        fps: fps as u8,
                                    },
                                );
                                cx.notify();
                            }
                        }
                    },
                )
                .detach();
                entity
            })
            .collect();
        MonitorsView {
            fps_selects,
            selected: None,
        }
    }

    /// 详情条当前指向的屏（None 或越界时跟随主屏；热插拔后自愈）。
    fn resolve_selected(&self, monitors: &[MonitorEntry]) -> usize {
        match self.selected {
            Some(i) if i < monitors.len() => i,
            _ => monitors.iter().position(|m| m.is_main).unwrap_or(0),
        }
    }

    /// 画布逻辑尺寸（视口 − 顶栏 − 页边距 − 详情条；确定性布局，见 render）。
    fn desk_size(window: &Window) -> Size<Pixels> {
        let vp = window.viewport_size();
        Size {
            width: px((vp.width.as_f32() - PAGE_PAD * 2.).max(320.)),
            height: px(
                (vp.height.as_f32() - TITLEBAR_H - PAGE_PAD * 2. - PAGE_GAP - STRIP_H).max(220.),
            ),
        }
    }

    /// 沙盘渐变底（desk 地面：亮色 = panel→hairline，暗色 = panel→surface，均向下沉）。
    fn desk_ground(cx: &Context<Self>) -> gpui_kit::gpui::Background {
        let t = tokens(cx);
        let (from, to) = if cx.theme().mode.is_dark() {
            (t.panel, t.surface)
        } else {
            (t.panel, t.hairline)
        };
        linear_gradient(180., linear_color_stop(from, 0.), linear_color_stop(to, 1.))
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let redetect = Button::new("btn-redetect")
            .label(BTN_REDETECT())
            .secondary()
            .icon(Icon::new(IconName::RefreshCw))
            .on_click(|_, window, cx| {
                // 重新检测：走引擎动作队列（sync_monitors 幂等；API.md §1/§4）
                crate::engine::enqueue(crate::engine::EngineAction::SyncMonitors);
                update(window, cx, |g| g.redetect());
                window.push_notification(Notification::info(TOAST_REDETECT()), cx);
            });
        h_flex()
            .flex_none()
            .h(px(DESK_HEAD_H))
            .px(px(14.))
            .gap(px(10.))
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.))
                    .text_color(t.text2)
                    .child(MONITOR_HOVER_NOTE()),
            )
            .child(redetect)
            .into_any_element()
    }

    /// 屏幕小像内的壁纸面层（contain 适配：竖版/超宽完整可见、与真实桌面一致；
    /// 失效=灰底问号；加载中=渐变+指示）。
    fn screen_surface(
        item: Option<&super::data::LibraryItem>,
        ratio: f32,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = tokens(cx);
        let Some(item) = item else {
            return div().into_any_element();
        };
        if item.broken {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(MINI_INNER_R))
                .bg(t.preview_frame)
                .text_color(t.text2)
                .child(Icon::new(IconName::CircleQuestionMark).size_4())
                .into_any_element();
        }
        if let Some(path) = item.thumbs.first() {
            if std::path::Path::new(path).exists() {
                use gpui_kit::gpui::StyledImage as _;
                let source = gpui_kit::gpui::ImageSource::Resource(gpui_kit::gpui::Resource::Path(
                    std::path::PathBuf::from(path).into(),
                ));
                return gpui_kit::gpui::img(source)
                    .size_full()
                    // 与容器同比例，压制固有宽高比（否则竖图会被撑高裁切）
                    .aspect_ratio(ratio)
                    .object_fit(ObjectFit::Contain)
                    .rounded(px(MINI_INNER_R))
                    .into_any_element();
            }
        }
        // 加载中：素材色渐变 + 居中指示（缩略图后台生成中；永不白屏）
        let from: gpui_kit::gpui::Hsla = rgb(item.art.from).into();
        let to: gpui_kit::gpui::Hsla = rgb(item.art.to).into();
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(MINI_INNER_R))
            .bg(linear_gradient(
                135.,
                linear_color_stop(from, 0.),
                linear_color_stop(to, 1.),
            ))
            .child(
                div()
                    .size_7()
                    .rounded_full()
                    .bg(gpui_kit::gpui::black().opacity(0.30))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::RefreshCw)
                            .size_3()
                            .text_color(gpui_kit::gpui::white()),
                    ),
            )
            .into_any_element()
    }

    /// 小像顶部状态胶囊（深色半透玻璃，任何壁纸上可读）。
    fn status_chip(icon: IconName, label: &str, dimmed: bool) -> AnyElement {
        let fg = if dimmed {
            gpui_kit::gpui::white().opacity(0.82)
        } else {
            gpui_kit::gpui::white()
        };
        h_flex()
            .absolute()
            .top(px(8.))
            .left(px(8.))
            .items_center()
            .gap_1()
            .px(px(9.))
            .py(px(3.))
            .rounded_full()
            .bg(gpui_kit::gpui::black().opacity(0.55))
            .border_1()
            .border_color(gpui_kit::gpui::white().opacity(0.16))
            .text_size(px(11.))
            .text_color(fg)
            .child(Icon::new(icon).size_3().text_color(fg))
            .child(label.to_string())
            .into_any_element()
    }

    /// 小像底部名称托底（锚底绝对定位；已指派 = 黑渐变托底白字，
    /// 未指派 = 无托底、主题色文字）。
    fn name_scrim(m: &MonitorEntry, assigned: bool, cx: &Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let base = h_flex()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .items_baseline()
            .gap_2()
            .px(px(10.));
        if assigned {
            base.pt(px(24.))
                .pb(px(8.))
                .rounded_b(px(MINI_INNER_R))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(gpui_kit::gpui::black().opacity(0.), 0.),
                    linear_color_stop(gpui_kit::gpui::black().opacity(0.62), 1.),
                ))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(gpui_kit::gpui::white())
                        .child(m.name.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .font_features(tabular())
                        .text_color(gpui_kit::gpui::white().opacity(0.72))
                        .child(m.label.to_string()),
                )
                .into_any_element()
        } else {
            base.py(px(8.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.text1)
                        .child(m.name.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .font_features(tabular())
                        .text_color(t.text2)
                        .child(m.label.to_string()),
                )
                .into_any_element()
        }
    }

    /// 屏幕小像（§4.4：等比 + 壁纸面层 + 状态胶囊 + 名称托底；控件不进小像）。
    fn screen_mini(
        &self,
        i: usize,
        m: &MonitorEntry,
        left: Pixels,
        top: Pixels,
        width: Pixels,
        ratio: f32,
        _selected_idx: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = tokens(cx);
        let glow = state(cx)
            .hovered
            .as_ref()
            .zip(m.wallpaper.as_ref())
            .is_some_and(|(h, w)| h == w);
        let item = m
            .wallpaper
            .as_ref()
            .and_then(|wid| state(cx).library.iter().find(|it| &it.id == wid))
            .cloned();
        let assigned = item.is_some();
        // 描边只在显式点选时出现——默认跟随主屏不描边（持续蓝框 = 视觉噪音）
        let is_selected = self.selected == Some(i);

        // 状态胶囊（§5 状态矩阵投影；与托盘角标一致）
        let (st_icon, st_label, dimmed) = match (&item, m.state) {
            (None, _) => (IconName::MonitorPause, ST_UNASSIGNED(), true),
            (Some(_), s) => {
                let (icon, label, _) = play_state_visual(s, cx);
                (icon, label, s.paused())
            }
        };

        // 拖放底色（拖拽库卡片到屏上放手 = 指派）
        let (dz_accent, dz_accent_soft) = (t.accent, t.accent_soft);
        let wallpaper = m.wallpaper.clone();

        div()
            .id(SharedString::from(format!("screen-{i}")))
            .absolute()
            .left(left)
            .top(top)
            .w(width)
            .aspect_ratio(ratio)
            .rounded(px(10.))
            .overflow_hidden()
            .border_2()
            .border_color(if glow || is_selected {
                t.accent
            } else {
                t.hairline2
            })
            .when(assigned, |d| d.shadow_md())
            .when(glow, |d| d.shadow_lg())
            .when(!assigned, |d| d.border_dashed().bg(t.panel).shadow_none())
            .drag_over::<CardDrag>(move |s, _, _, _| {
                s.border_color(dz_accent).border_dashed().bg(dz_accent_soft)
            })
            .on_drop(move |drag: &CardDrag, window, cx| {
                super::library_view::assign_from_drop(&drag.item_id, i, window, cx);
            })
            .on_hover({
                // 悬停小像 → 描边库页对应卡片（in-window 版 signature #1）
                let wallpaper = wallpaper.clone();
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
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected = Some(i);
                cx.notify();
                if !this.selected.is_some_and(|s| {
                    state(cx)
                        .monitors
                        .get(s)
                        .is_some_and(|m| m.wallpaper.is_some())
                }) {
                    // 未指派屏：小像即 CTA，直接跳库页
                    super::shell::switch_tab(window, cx, super::shell::Tab::Library);
                }
            }))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(MINI_INNER_R))
                    .overflow_hidden()
                    .child(Self::screen_surface(item.as_ref(), ratio, cx)),
            )
            .when(assigned, |d| {
                d.child(Self::status_chip(st_icon, st_label, dimmed))
            })
            .child(Self::name_scrim(m, assigned, cx))
            .when(!assigned, |d| {
                d.child(
                    v_flex()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .text_color(t.text2)
                        .child(
                            div()
                                .size(px(34.))
                                .rounded_full()
                                .border_1()
                                .border_dashed()
                                .border_color(t.hairline2)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(Icon::new(IconName::Plus).size_4()),
                        )
                        .child(div().text_size(px(12.)).child(ST_SELECT_WALLPAPER())),
                )
            })
            .into_any_element()
    }

    /// 沙盘：desk 渐变底 + 点阵 + 顶栏 + 等比小像 + 新显示器横幅 / 空态。
    fn desk(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let monitors = state(cx).monitors.clone();
        let desk = Self::desk_size(window);
        let selected_idx = self.resolve_selected(&monitors);

        // —— 等比排布：工作区包围盒适配画布（y 轴翻转：macOS 坐标向上，画布向下）——
        let (min_x, min_y, max_x, max_y) = monitors.iter().fold(
            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
            |(a, b, c, d), m| {
                (
                    a.min(m.rect.0),
                    b.min(m.rect.1),
                    c.max(m.rect.0 + m.rect.2),
                    d.max(m.rect.1 + m.rect.3),
                )
            },
        );
        let placements: Vec<(Pixels, Pixels, Pixels, f32)> = if monitors.is_empty() {
            vec![]
        } else {
            let bw = (max_x - min_x).max(1.);
            let bh = (max_y - min_y).max(1.);
            let avail_w = (desk.width.as_f32() - DESK_PAD * 2.).max(80.);
            let avail_h = (desk.height.as_f32() - DESK_HEAD_H - DESK_PAD * 2.).max(60.);
            let scale = ((avail_w / bw).min(avail_h / bh)).min(MAX_SCALE);
            let off_x = DESK_PAD + (avail_w - bw * scale) / 2.;
            let off_y = DESK_HEAD_H + DESK_PAD + (avail_h - bh * scale) / 2.;
            monitors
                .iter()
                .map(|m| {
                    (
                        px(off_x + (m.rect.0 - min_x) * scale),
                        px(off_y + (max_y - (m.rect.1 + m.rect.3)) * scale),
                        px(m.rect.2 * scale),
                        m.rect.2 / m.rect.3.max(1.),
                    )
                })
                .collect()
        };

        // 点阵：排布工具的图纸语义（低对比，只在 desk 地面上）
        let dots = canvas(
            |bounds, _, _| bounds,
            |bounds, _, window, cx| {
                let t = tokens(cx);
                let dot = t.hairline2.opacity(0.45);
                let step = px(DOT_STEP);
                let half = px(1.);
                let mut y = bounds.origin.y + px(11.);
                while y < bounds.bottom() {
                    let mut x = bounds.origin.x + px(11.);
                    while x < bounds.right() {
                        window.paint_quad(quad(
                            Bounds {
                                origin: point(x - half, y - half),
                                size: size(px(2.), px(2.)),
                            },
                            px(1.),
                            dot,
                            px(0.),
                            dot,
                            BorderStyle::Solid,
                        ));
                        x += step;
                    }
                    y += step;
                }
            },
        );

        let minis = placements
            .into_iter()
            .enumerate()
            .map(|(i, (left, top, w, ratio))| {
                self.screen_mini(i, &monitors[i], left, top, w, ratio, selected_idx, cx)
            })
            .collect::<Vec<_>>();

        div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(t.hairline)
            .bg(Self::desk_ground(cx))
            .child(
                // 内缩一个圆角半径：quad 绘制不受容器圆角裁剪
                div().absolute().inset(px(12.)).child(dots),
            )
            .child(self.toolbar(cx))
            .children(minis)
            // 新显示器横幅（浮于画布顶，不挤压内容）
            .children(state(cx).pending_new_monitor.then(|| {
                h_flex()
                    .absolute()
                    .top(px(DESK_HEAD_H + 8.))
                    .left(relative(0.5))
                    .items_center()
                    .gap_2()
                    .px(px(14.))
                    .py(px(7.))
                    .rounded(px(8.))
                    .bg(t.accent_soft)
                    .shadow_md()
                    .text_size(px(12.))
                    .text_color(t.text1)
                    .child(Icon::new(IconName::TriangleAlert).size_4())
                    .child(super::strings::notice_new_monitor("DELL U2723QE"))
                    .child(
                        Button::new("go-assign")
                            .label(BTN_GO_ASSIGN())
                            .text()
                            .with_size(KitSize::Small)
                            .on_click(|_, window, cx| {
                                super::shell::switch_tab(window, cx, super::shell::Tab::Library);
                            }),
                    )
                    .into_any_element()
            }))
            // 空态：未检测到显示器
            .when(monitors.is_empty(), |d| {
                d.child(
                    v_flex()
                        .absolute()
                        .inset_0()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(empty_art(IconName::Monitor, cx))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.text1)
                                .child(MONITORS_EMPTY_TITLE()),
                        )
                        .child(
                            Button::new("btn-redetect-empty")
                                .label(BTN_REDETECT())
                                .primary()
                                .icon(Icon::new(IconName::RefreshCw))
                                .on_click(|_, window, cx| {
                                    crate::engine::enqueue(
                                        crate::engine::EngineAction::SyncMonitors,
                                    );
                                    update(window, cx, |g| g.redetect());
                                    window.push_notification(
                                        Notification::info(TOAST_REDETECT()),
                                        cx,
                                    );
                                }),
                        ),
                )
            })
            .into_any_element()
    }

    /// 详情条（画布下方，固定高）：选中屏的身份与状态 + 全部控件。
    fn detail_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let monitors = state(cx).monitors.clone();
        let Some(m) = monitors.get(self.resolve_selected(&monitors)) else {
            return div().into_any_element();
        };
        let sel = self.resolve_selected(&monitors);
        let item = m
            .wallpaper
            .as_ref()
            .and_then(|wid| state(cx).library.iter().find(|it| &it.id == wid))
            .cloned();

        // 状态：壁纸名 · 运行状态（与托盘角标一致）
        let (st_icon, st_text, st_color) = match (&item, m.state) {
            (None, _) => (None, ST_UNASSIGNED().to_string(), t.text2),
            (Some(item), s) => {
                let (icon, label, color) = play_state_visual(s, cx);
                (Some(icon), format!("{} · {}", item.name, label), color)
            }
        };
        let paused = m.state.paused();

        // 更换：跳库页并带上当前壁纸作为上下文
        let change_ctx = m.wallpaper.clone();
        let toggle_real_id = m.real_id.clone();
        let fps_select = self.fps_selects.get(sel).map(|s| {
            div().w(px(96.)).child(
                // 详情条控件统一 Small（§1.2 工艺基准；Medium 32px 在 12px 状态行旁头重脚轻）
                Select::new(s).with_size(KitSize::Small).appearance(true),
            )
        });

        h_flex()
            .flex_none()
            .h(px(STRIP_H))
            .px(px(16.))
            .gap(px(14.))
            .items_center()
            .rounded(px(12.))
            .border_1()
            .border_color(t.hairline)
            .bg(t.surface)
            .child(
                // 身份块：短名 + 主屏角标 + 分辨率 / EDID 备注
                v_flex()
                    .gap(px(3.))
                    .min_w_0()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.text1)
                                    .child(m.name.to_string()),
                            )
                            .child(badge(m.short.as_ref(), m.is_main, cx))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_features(tabular())
                                    .text_color(t.text2)
                                    .child(m.label.to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.text2)
                            .child(EDID_NOTE_SHORT()),
                    ),
            )
            .child(
                // 状态（随选中屏切换；图标带状态色，正文保持前景色）
                h_flex()
                    .flex_1()
                    .items_center()
                    .gap_1()
                    .text_size(px(12.))
                    .font_features(tabular())
                    .text_color(t.text1)
                    .when_some(st_icon, |r, icon| {
                        r.child(
                            Icon::new(icon)
                                .size_3()
                                .text_color(st_color)
                                .into_any_element(),
                        )
                    })
                    .child(st_text),
            )
            .when_some(item.as_ref(), {
                // 已指派：[更换][暂停/恢复] + FPS 下拉
                move |strip, _| {
                    strip
                        .child(
                            Button::new("btn-change")
                                .label(BTN_CHANGE())
                                .secondary()
                                .with_size(KitSize::Small)
                                .on_click(cx.listener(move |_, _, window, cx| {
                                    update(window, cx, |g| {
                                        g.selected = change_ctx.clone();
                                    });
                                    super::shell::switch_tab(
                                        window,
                                        cx,
                                        super::shell::Tab::Library,
                                    );
                                    window.push_notification(
                                        Notification::info(TOAST_CHANGE_HINT()),
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            Button::new("btn-toggle")
                                .label(if paused { BTN_RESUME() } else { BTN_PAUSE() })
                                .secondary()
                                .with_size(KitSize::Small)
                                .on_click(move |_, window, cx| {
                                    if toggle_real_id.is_empty() {
                                        // 切换 UI 投影状态
                                        update(window, cx, |g| g.toggle_pause(sel));
                                    } else {
                                        // 真源：引擎 pause_one（≤150ms 执行，快照回灌刷新状态行）
                                        crate::engine::enqueue(
                                            crate::engine::EngineAction::PauseOne {
                                                monitor_id: toggle_real_id.clone(),
                                                paused: !paused,
                                            },
                                        );
                                    }
                                }),
                        )
                        .children(fps_select)
                }
            })
            .when_none(&item, {
                // 未指派：只有「选择壁纸」主按钮
                |strip| {
                    strip.child(
                        Button::new("btn-pick")
                            .label(ST_SELECT_WALLPAPER())
                            .primary()
                            .with_size(KitSize::Small)
                            .on_click(|_, window, cx| {
                                super::shell::switch_tab(window, cx, super::shell::Tab::Library);
                            }),
                    )
                }
            })
            .into_any_element()
    }
}

impl Render for MonitorsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let desk = self.desk(window, cx);
        v_flex()
            .size_full()
            .px(px(PAGE_PAD))
            .py(px(PAGE_PAD))
            .gap(px(PAGE_GAP))
            .text_color(t.text1)
            .child(desk)
            .when(!state(cx).monitors.is_empty(), |page| {
                page.child(self.detail_strip(cx))
            })
    }
}
