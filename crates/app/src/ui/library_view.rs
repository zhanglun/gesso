//! 壁纸库页（§4.3，默认页）：筛选 / 搜索 / 卡片网格 / 状态条 / 空状态 /
//! 右键菜单 / 拖拽发起 + 拖拽时的全屏投放区（signature #2）。

use gpui_kit::component::Sizable as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{h_flex, v_flex, Icon, WindowExt as _};
use gpui_kit::gpui::prelude::FluentBuilder as _;
use gpui_kit::gpui::{
    div, px, rgba, AnyElement, App, AppContext as _, BorrowAppContext as _, ClickEvent, Context,
    Entity, Focusable as _, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement as _, Styled, Window,
};

use super::app_state::{state, update, CardDrag, Filter, GessoState};
use super::data::{Art, Kind, LibraryItem};
use super::strings::*;
use super::theme::tokens;
use super::widgets::{badge, empty_art, preview};

const CARD_W: f32 = 208.;

pub struct LibraryView {
    search_input: Entity<InputState>,
}

impl LibraryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder(SEARCH_PLACEHOLDER));
        // 搜索即时过滤：输入事件 → 全局 query（§4.3 交互表）
        cx.subscribe(&search_input, |_, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let q = input.read(cx).value().to_string();
                cx.update_global::<GessoState, _>(|g, _| g.query = q);
                cx.notify();
            }
        })
        .detach();
        LibraryView { search_input }
    }

    /// 键盘模型 `F`：聚焦搜索（Shell 调用）。
    pub fn focus_search(&self, window: &mut Window, cx: &mut App) {
        let handle = self.search_input.read(cx).focus_handle(cx).clone();
        window.focus(&handle, cx);
    }

    /// 键盘模型 `Esc`：清空搜索。
    pub fn clear_search(&self, window: &mut Window, cx: &mut App) {
        let focused = self
            .search_input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let empty = self.search_input.read(cx).value().is_empty();
        if focused || !empty {
            self.search_input
                .update(cx, |input, cx| input.set_value("", window, cx));
            update(window, cx, |g| {
                g.query.clear();
            });
        }
    }

    fn seg_button(&self, f: Filter, idx: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let selected = state(cx).filter == f;
        let label = f.label();
        div()
            .id(SharedString::from(format!("filter-{idx}")))
            .px_3()
            .py_1()
            .rounded(px(5.))
            .text_size(px(12.))
            .text_color(if selected { t.text1 } else { t.text2 })
            .when(selected, |d| {
                d.bg(t.elevated).font_weight(FontWeight::MEDIUM).shadow_sm()
            })
            .hover(|s| s.text_color(t.text1))
            .child(label)
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.update_global::<GessoState, _>(|g, _| g.filter = f);
                cx.notify();
            }))
            .into_any_element()
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let filters = [
            Filter::All,
            Filter::Kind(Kind::Video),
            Filter::Kind(Kind::Gif),
            Filter::Kind(Kind::Shader),
            Filter::Kind(Kind::Web),
            Filter::We,
        ];
        let seg = h_flex()
            .bg(t.panel)
            .rounded(px(7.))
            .p_1()
            .gap_0p5()
            .children(
                filters
                    .into_iter()
                    .enumerate()
                    .map(|(i, f)| self.seg_button(f, i, cx)),
            );

        let search = div()
            .w(px(220.))
            .child(
                Input::new(&self.search_input)
                    .cleanable(true)
                    .prefix(Icon::new(IconName::Search).into_any_element())
                    .with_size(px(36.)),
            )
            .into_any_element();

        let import = Button::new("btn-import")
            .label(BTN_IMPORT)
            .secondary()
            .icon(Icon::new(IconName::Plus))
            .on_click(|_, window, cx| {
                super::app_state::import_with_dialog(window, cx);
            });

        h_flex()
            .flex_none()
            .h(px(48.))
            .px(px(14.))
            .gap(px(10.))
            .items_center()
            .border_b_1()
            .border_color(t.hairline)
            .child(seg)
            .child(search)
            .child(div().flex_1())
            .child(import)
            .into_any_element()
    }

    fn card(&self, item: &LibraryItem, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let hairline2 = t.hairline2;
        let id = item.id.clone();
        let broken = item.broken;
        let selected = state(cx).selected.as_ref() == Some(&item.id);
        let assigned_to = item.assigned;

        // 信息条：3px 语义指示条（§4.3 卡片解剖）——已指派 accent / 失效 danger / 未指派无
        let indicator_color = if broken {
            t.danger
        } else if assigned_to.is_some() {
            t.accent
        } else {
            gpui_kit::gpui::transparent_black()
        };

        let meta_line: AnyElement = if broken {
            div()
                .text_size(px(12.))
                .text_color(t.danger)
                .child(FILE_REMOVED)
                .into_any_element()
        } else {
            let mut meta = item.meta.to_string();
            if let Some(m) = assigned_to {
                let short = state(cx)
                    .monitors
                    .get(m)
                    .map(|mm| mm.short.to_string())
                    .unwrap_or_else(|| "?".into());
                meta = format!("{meta} · → {short}");
            }
            div()
                .text_size(px(12.))
                .text_color(t.text2)
                .font_features(super::widgets::tabular())
                .child(meta)
                .into_any_element()
        };

        let badges = h_flex()
            .gap_1()
            .child(badge(item.kind.label(), false, cx))
            .when(item.we, |r| r.child(badge(BADGE_WE, true, cx)))
            .into_any_element();

        let hover_id = item.id.clone();
        let hover_frame = if state(cx).hovered.as_ref() == Some(&item.id) {
            state(cx).hover_frame
        } else {
            0
        };
        let mut card = div()
            .id(SharedString::from(format!("card-{}", item.id)))
            .w(px(CARD_W))
            .rounded(px(12.))
            .bg(t.panel)
            .border_1()
            .border_color(if selected { t.accent } else { t.hairline })
            .when(selected, |d| d.border_2())
            .overflow_hidden()
            .cursor_pointer()
            .hover(move |s| {
                if selected {
                    s.shadow_md()
                } else {
                    s.shadow_md().border_color(hairline2)
                }
            })
            .on_click(cx.listener(move |_, click: &ClickEvent, window, cx| {
                if click.click_count() >= 2 {
                    // 双击 = 设为主显示器（托盘气泡确认）；经桥写入真会话
                    let main_idx = state(cx).main_monitor();
                    match super::app_state::bridge_assign(window, cx, main_idx, &id) {
                        Ok(name) => {
                            window.push_notification(
                                Notification::success(toast_apply_main(&name)),
                                cx,
                            );
                        }
                        Err(e) => {
                            window.push_notification(Notification::warning(e), cx);
                        }
                    }
                } else {
                    update(window, cx, |g| g.selected = Some(id.clone()));
                }
            }))
            .on_drag(
                CardDrag {
                    item_id: item.id.clone(),
                    item_name: item.name.clone(),
                    art: item.art,
                },
                |drag, _, _, cx| {
                    cx.new(|_| CardGhost {
                        name: drag.item_name.clone(),
                        art: drag.art,
                    })
                },
            )
            .on_hover(cx.listener(move |_, hovering: &bool, window, cx| {
                // signature #1：悬停点亮对应显示器 + 启动视频帧轮播
                update(window, cx, |g| {
                    g.hovered = if *hovering {
                        Some(hover_id.clone())
                    } else {
                        None
                    };
                    if *hovering {
                        g.hover_frame = 0;
                    }
                });
                if *hovering {
                    start_hover_cycle(cx);
                }
            }))
            .context_menu({
                let id = item.id.clone();
                let broken = item.broken;
                move |menu, window, cx| card_context_menu(&id, broken, menu, window, cx)
            });

        card = card
            .child(preview(
                item.art,
                Some(item.kind),
                broken,
                &item.thumbs,
                hover_frame,
                cx,
            ))
            .child(
                div()
                    .relative()
                    .h(px(56.))
                    .py_2()
                    .pl(px(12.))
                    .pr(px(10.))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap_1()
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(10.))
                            .bottom(px(10.))
                            .w(px(3.))
                            .rounded(px(2.))
                            .bg(indicator_color),
                    )
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if broken { t.danger } else { t.text1 })
                                    .truncate()
                                    .child(item.name.to_string()),
                            )
                            .child(badges),
                    )
                    .child(meta_line),
            );

        card.into_any_element()
    }

    fn grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let visible = state(cx).visible_items();
        if state(cx).library.is_empty() {
            return empty_library(cx);
        }
        if visible.is_empty() {
            return empty_search(cx);
        }
        let items: Vec<LibraryItem> = {
            let g = state(cx);
            visible
                .iter()
                .filter_map(|i| g.library.get(*i).cloned())
                .collect()
        };
        let cards: Vec<AnyElement> = items.into_iter().map(|item| self.card(&item, cx)).collect();
        let grid_accent = t.accent;
        div()
            .id("library-grid")
            .flex_1()
            .overflow_y_scroll()
            .py_4()
            .px(px(14.))
            .min_h_0()
            .child(div().flex().flex_wrap().gap(px(14.)).children(cards))
            // 拖入文件 = 同一导入路径（§4.3 交互表；gpui 把 FileDrop 翻译成 ExternalPaths 载荷）
            .drag_over::<gpui_kit::gpui::ExternalPaths>(move |s, _, _, _| {
                s.border_color(grid_accent).rounded(px(8.))
            })
            .on_drop(|paths: &gpui_kit::gpui::ExternalPaths, window, cx| {
                super::app_state::import_paths(paths.0.iter().cloned(), window, cx);
            })
            .into_any_element()
    }

    fn statusbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let g = state(cx);
        let we_count = g.library.iter().filter(|w| w.we).count();
        let mut count_text = format!("{} 项 · WE {}", g.library.len(), we_count);
        if let Some(sel) = &g.selected {
            if let Some(item) = g.library.iter().find(|w| &w.id == sel) {
                count_text = format!("{count_text}  ·  {} · {}", item.name, item.meta);
            }
        }
        h_flex()
            .flex_none()
            .h(px(28.))
            .px(px(14.))
            .gap_3()
            .items_center()
            .border_t_1()
            .border_color(t.hairline)
            .text_size(px(12.))
            .text_color(t.text2)
            .font_features(super::widgets::tabular())
            .child(count_text)
            .when_some(g.status_error.clone(), |r, err| {
                r.child(div().text_color(t.danger).child(err))
            })
            .child(div().flex_1().child(HINT_LIBRARY))
            .into_any_element()
    }

    /// 拖拽中的全屏投放区（原型行为：不切页签，覆盖层上投放 = 指派）。
    fn drag_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !cx.has_active_drag() || !state(cx).card_dragging {
            return None;
        }
        let t = tokens(cx);
        let (accent, accent_soft, elevated, hairline, text1, text2) = (
            t.accent,
            t.accent_soft,
            t.elevated,
            t.hairline,
            t.text1,
            t.text2,
        );
        let targets: Vec<AnyElement> = state(cx)
            .monitors
            .iter()
            .enumerate()
            .map(move |(i, m)| {
                div()
                    .id(SharedString::from(format!("dz-{i}")))
                    .w(px(180.))
                    .h(px(120.))
                    .rounded(px(12.))
                    .bg(elevated)
                    .border_1()
                    .border_color(hairline)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .drag_over::<CardDrag>(move |s, _, _, _| {
                        s.border_color(accent).border_dashed().bg(accent_soft)
                    })
                    .on_drop(move |drag: &CardDrag, window, cx| {
                        assign_item(&drag.item_id, i, window, cx);
                    })
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(text1)
                            .child(m.name.to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(text2)
                            .font_features(super::widgets::tabular())
                            .child(m.label.to_string()),
                    )
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .absolute()
                .inset_0()
                .bg(rgba(0x00000073u32))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgba(0xFFFFFFFFu32))
                        .child(DZ_HINT),
                )
                .child(h_flex().gap_4().children(targets))
                .into_any_element(),
        )
    }
}

impl Render for LibraryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let toolbar = self.toolbar(cx);
        let grid = self.grid(cx);
        let statusbar = self.statusbar(cx);
        let overlay = self.drag_overlay(cx);
        v_flex()
            .size_full()
            .text_color(t.text1)
            .child(toolbar)
            .child(
                div()
                    .flex_1()
                    .relative()
                    .min_h_0()
                    .child(grid)
                    .when_some(overlay, |d, o| d.child(o)),
            )
            .child(statusbar)
    }
}

/* ---------- 右键菜单（§4.3：设为壁纸子菜单 · 打开目录 · 详情 · 移除红字置底） ---------- */

fn card_context_menu(
    item_id: &SharedString,
    broken: bool,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    if broken {
        // 失效卡片：从库移除置顶（§4.3 错误状态）
        return menu
            .item(PopupMenuItem::new(MENU_REMOVE).on_click({
                let id = item_id.clone();
                move |_, window, cx| remove_item(&id, window, cx)
            }))
            .separator()
            .item(PopupMenuItem::new(MENU_OPEN_FOLDER).disabled(true));
    }

    let sub = PopupMenu::build(window, cx, |m, _, _| {
        m.item(PopupMenuItem::new("主屏").on_click({
            let id = item_id.clone();
            move |_, window, cx| assign_item(&id, 0, window, cx)
        }))
        .item(PopupMenuItem::new("副屏").on_click({
            let id = item_id.clone();
            move |_, window, cx| assign_item(&id, 1, window, cx)
        }))
        .item(PopupMenuItem::new(MENU_ALL_MONITORS).on_click({
            let id = item_id.clone();
            move |_, window, cx| assign_all(&id, window, cx)
        }))
    });
    menu.item(PopupMenuItem::submenu(MENU_SET_WALLPAPER, sub))
        .separator()
        .item(
            PopupMenuItem::new(MENU_OPEN_FOLDER).on_click({
                let dir_path = crate::protocol::library_dir().join(item_id.as_ref());
                move |_, _, _| {
                    let _ = std::process::Command::new("open").arg(&*dir_path).spawn();
                }
            }),
        )
        .item(PopupMenuItem::new(MENU_DETAILS).on_click({
            let id = item_id.clone();
            move |_, window, cx| update(window, cx, |g| g.selected = Some(id.clone()))
        }))
        .separator()
        .item(
            PopupMenuItem::element(move |_, _| danger_item(MENU_REMOVE)).on_click({
                let id = item_id.clone();
                move |_, window, cx| remove_item(&id, window, cx)
            }),
        )
}

/* ---------- 动作辅助（菜单 / 拖放共享） ---------- */

/// 显示器页拓扑投放落点（跨页签拖拽的指派入口）。
pub fn assign_from_drop(item_id: &str, monitor: usize, window: &mut Window, cx: &mut App) {
    assign_item(item_id, monitor, window, cx)
}

fn assign_item(item_id: &str, monitor: usize, window: &mut Window, cx: &mut App) {
    let target = state(cx).monitor_name(monitor);
    match super::app_state::bridge_assign(window, cx, monitor, item_id) {
        Ok(name) => {
            window.push_notification(Notification::success(toast_assign(&name, &target)), cx);
        }
        Err(e) => window.push_notification(Notification::warning(e), cx),
    }
}

fn assign_all(item_id: &str, window: &mut Window, cx: &mut App) {
    let count = state(cx).monitors.len();
    let mut last_name = String::new();
    for i in 0..count {
        if let Ok(name) = super::app_state::bridge_assign(window, cx, i, item_id) {
            last_name = name;
        }
    }
    window.push_notification(
        Notification::success(format!("已将「{last_name}」指派到全部显示器")),
        cx,
    );
}

fn remove_item(item_id: &str, window: &mut Window, cx: &mut App) {
    let name = state(cx)
        .library
        .iter()
        .find(|w| w.id.as_ref() == item_id)
        .map(|w| w.name.to_string())
        .unwrap_or_default();
    update(window, cx, |g| g.remove(item_id));
    window.push_notification(Notification::info(toast_removed(&name)), cx);
}

fn danger_item(label: &'static str) -> AnyElement {
    div()
        .px_2()
        .py_1()
        .text_size(px(13.))
        .text_color(gpui_kit::gpui::rgb(0xD33A3A))
        .child(label)
        .into_any_element()
}

/* ---------- 空状态（区分「库为空」与「无结果」，§4.3） ---------- */

fn empty_library(cx: &App) -> AnyElement {
    let t = tokens(cx);
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .child(empty_art(IconName::Wallpaper, cx))
        .child(
            div()
                .text_size(px(15.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.text1)
                .child(EMPTY_LIBRARY_TITLE),
        )
        .child(
            div()
                .text_size(px(13.))
                .text_color(t.text2)
                .child(EMPTY_LIBRARY_DESC),
        )
        .child(
            h_flex()
                .gap(px(10.))
                .child(
                    Button::new("empty-import")
                        .label(BTN_IMPORT_FILE)
                        .primary()
                        .on_click(|_, window, cx| {
                            super::app_state::import_with_dialog(window, cx);
                        }),
                )
                .child(
                    Button::new("empty-samples")
                        .label(BTN_BROWSE_SAMPLES)
                        .secondary()
                        .on_click(|_, window, cx| {
                            // 内置样例已由 bootstrap 注册；此钮跳到显示器页指派
                            crate::engine::enqueue(crate::engine::EngineAction::FocusMainWindow);
                            update(window, cx, |g| g.active_tab = super::shell::Tab::Monitors);
                        }),
                ),
        )
        .into_any_element()
}

fn empty_search(cx: &mut Context<LibraryView>) -> AnyElement {
    let t = tokens(cx);
    let query = state(cx).query.clone();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .child(empty_art(IconName::Search, cx))
        .child(
            div()
                .text_size(px(15.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.text1)
                .child(format!("没有匹配「{query}」的壁纸")),
        )
        .child(
            Button::new("clear-search")
                .label(BTN_CLEAR_SEARCH)
                .text()
                .on_click(|_, window, cx| {
                    cx.update_global::<GessoState, _>(|g, _| {
                        g.query.clear();
                        g.filter = Filter::All;
                    });
                    window.refresh();
                }),
        )
        .into_any_element()
}

/* ---------- 拖拽 ghost ---------- */

struct CardGhost {
    name: SharedString,
    art: Art,
}

impl Render for CardGhost {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // ghost 只在卡片拖拽期间存活：置标志供投放覆盖层区分「卡片拖拽 vs 文件拖放」
        cx.update_global::<GessoState, _>(|g, _| g.card_dragging = true);
        let t = tokens(cx);
        div()
            .w(px(CARD_W))
            .rounded(px(12.))
            .overflow_hidden()
            .border_1()
            .border_color(t.accent)
            .shadow_lg()
            .opacity(0.9)
            .bg(t.panel)
            .child(preview(self.art, None, false, &[], 0, cx))
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(t.text1)
                    .child(self.name.to_string()),
            )
    }
}

/// 悬停轮播驱动：125ms/帧（~8fps）推进 `hover_frame`，悬停离开即停。
/// 单一全局循环：每次进入新悬停都会先停旧循环（通过 hovered 校验）。
fn start_hover_cycle(cx: &mut gpui_kit::gpui::Context<LibraryView>) {
    cx.spawn(async move |this, cx| loop {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(125))
            .await;
        let still_hovering = this
            .update(cx, |_, cx| {
                let g = cx.global::<GessoState>();
                let alive = g.hovered.is_some();
                if alive {
                    let len = g
                        .library
                        .iter()
                        .find(|w| Some(&w.id) == g.hovered.as_ref())
                        .map(|w| w.thumbs.len())
                        .unwrap_or(0);
                    if len > 0 {
                        cx.update_global::<GessoState, _>(|g, _| {
                            g.hover_frame = (g.hover_frame + 1) % len;
                        });
                        cx.notify();
                    }
                }
                alive
            })
            .unwrap_or(false);
        if !still_hovering {
            break;
        }
    })
    .detach();
}
