//! 壁纸库页（§4.3，默认页）：筛选 / 搜索 / 卡片网格 / 状态条 / 空状态 /
//! 右键菜单 / 拖拽发起 + 拖拽时的全屏投放区（signature #2）。

use gpui_kit::component::icon::{Icon, IconName};
use gpui_kit::component::input::{InputState, TextInput};
use gpui_kit::component::menu::{PopupMenuItem, PopupMenu};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::gpui::{
    AnyElement, App, ClickEvent, Context, Entity, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled, Window,
    div, px, rgba,
};

use super::app_state::{CardDrag, Filter, GessoState, state, update};
use super::data::LibraryItem;
use super::strings::*;
use super::theme::tokens;
use super::widgets::{badge, empty_art, play_state_visual, preview};

const CARD_W: f32 = 208.;

pub struct LibraryView {
    search_input: Entity<InputState>,
}

impl LibraryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(SEARCH_PLACEHOLDER)
                .cleanable(true)
        });
        // 搜索即时过滤：输入事件 → 全局 query（§4.3 交互表）
        cx.subscribe(&search_input, |_, input, event: &gpui_kit::component::input::InputEvent, cx| {
            if matches!(event, gpui_kit::component::input::InputEvent::Change) {
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
        let handle = self.search_input.read(cx).focus_handle(cx);
        window.focus(&handle);
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
            self.search_input.update(cx, |input, cx| {
                input.set_value("", window, cx);
            });
            update(window, cx, |g| g.query.clear());
        }
    }

    fn seg_button(
        &self,
        f: Filter,
        idx: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            .when_(selected, |d| {
                d.bg(t.elevated)
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .shadow_sm()
            })
            .hover(|s| s.text_color(t.text1))
            .child(label.to_string())
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
            Filter::Kind(super::data::Kind::Video),
            Filter::Kind(super::data::Kind::Gif),
            Filter::Kind(super::data::Kind::Shader),
            Filter::Kind(super::data::Kind::Web),
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
                TextInput::new(&self.search_input)
                    .prefix(Icon::new(IconName::Search).size_3().into_any_element()),
            )
            .into_any_element();

        let import = gpui_kit::component::button::Button::new("btn-import")
            .label(BTN_IMPORT)
            .secondary()
            .icon(IconName::Plus)
            .on_click(|_, window, cx| {
                let name = cx.update_global::<GessoState, _>(|g, _| g.import_demo());
                window.push_notification(
                    Notification::success(format!("已导入「{name}」（演示）")),
                    cx,
                );
            });
        let scan = gpui_kit::component::button::Button::new("btn-scan")
            .label(BTN_SCAN_WORKSHOP)
            .secondary()
            .icon(IconName::Scan)
            .on_click(|_, window, cx| {
                window.push_notification(Notification::info(TOAST_SCAN_FOUND), cx);
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
            .child(scan)
            .into_any_element()
    }

    fn card(&self, idx: usize, item: &LibraryItem, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let id = item.id.clone();
        let name = item.name.clone();
        let broken = item.broken;
        let selected = state(cx).selected.as_ref() == Some(&item.id);
        let assigned_to = item.assigned;

        // 信息条：3px 语义指示条（§4.3 卡片解剖）
        let indicator_color = if broken {
            t.danger
        } else if assigned_to.is_some() {
            t.accent
        } else {
            gpui_kit::gpui::transparent_black().into()
        };

        let meta_line: AnyElement = if broken {
            div()
                .text_size(px(12.))
                .text_color(t.danger)
                .child(FILE_REMOVED.to_string())
                .into_any_element()
        } else {
            let mut meta = item.meta.to_string();
            if let Some(m) = assigned_to {
                meta = format!("{} · → {}", meta, state(cx).monitor_name(m).replace("显示器", "屏"));
            }
            div()
                .text_size(px(12.))
                .text_color(t.text2)
                .child(meta)
                .into_any_element()
        };

        let badges = h_flex()
            .gap_1()
            .child(badge(item.kind.label(), false, cx))
            .when_(item.we, |r| r.child(badge(BADGE_WE, true, cx)))
            .into_any_element();

        let mut card = div()
            .id(SharedString::from(format!("card-{}", item.id)))
            .group("card")
            .w(px(CARD_W))
            .rounded(px(12.))
            .bg(t.panel)
            .border_1()
            .border_color(if selected { t.accent } else { t.hairline })
            .when_(selected, |d| d.border_2())
            .overflow_hidden()
            .cursor_pointer()
            .hover(|s| s.shadow_md().when_(!selected, |s| s.border_color(t.hairline2)))
            .on_click(cx.listener(move |_, click: &ClickEvent, window, cx| {
                if click.up.click_count >= 2 {
                    // 双击 = 设为主显示器（托盘气泡确认）
                    let result = cx.update_global::<GessoState, _>(|g, _| {
                        g.assign(g.main_monitor(), id.as_ref()).map(|_| ()).map_err(|e| e.to_string())
                    });
                    match result {
                        Ok(Ok(())) => {
                            window.refresh();
                            window.push_notification(
                                Notification::success(format!(
                                    "{}",
                                    TOAST_APPLY_MAIN.replace("{}", &name)
                                )),
                                cx,
                            );
                        }
                        Ok(Err(e)) => window.push_notification(Notification::warning(e), cx),
                        Err(_) => {}
                    }
                } else {
                    update(window, cx, |g| g.selected = Some(id.clone()));
                }
            }))
            .on_drag(
                CardDrag { item_id: item.id.clone(), item_name: item.name.clone() },
                |drag, _, _, cx| {
                    cx.new(|_| CardGhost {
                        name: drag.item_name.clone(),
                        art: drag_art(&drag.item_id),
                    })
                },
            )
            .on_hover(cx.listener(move |_, hovering: &bool, window, cx| {
                // signature #1：悬停库卡片 → 对应显示器边框点亮
                update(window, cx, |g| {
                    g.hovered = if *hovering { Some(item.id.clone()) } else { None };
                });
            }))
            .context_menu(move |menu, _, _| {
                let item_id = item.id.clone();
                let item_name = item.name.clone();
                let is_broken = broken;
                let menu = if is_broken {
                    // 失效卡片：从库移除置顶（§4.3 错误状态）
                    menu.item(
                        PopupMenuItem::new(MENU_REMOVE)
                            .element_icon(None)
                            .on_click(move |_, window, cx| remove_item(&item_id, window, cx)),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new(MENU_OPEN_FOLDER)
                            .disabled(true),
                    )
                } else {
                    let sub_targets = [("主屏", 0usize), ("副屏", 1usize)];
                    let sub = PopupMenu::build(cx.window, cx.app, |m, _, _| {
                        let mut m = m
                            .item(PopupMenuItem::new(sub_targets[0].0).on_click({
                                let id = item_id.clone();
                                move |_, window, cx| assign_item(&id, 0, window, cx)
                            }))
                            .item(PopupMenuItem::new(sub_targets[1].0).on_click({
                                let id = item_id.clone();
                                move |_, window, cx| assign_item(&id, 1, window, cx)
                            }));
                        m = m.item(PopupMenuItem::new(MENU_ALL_MONITORS).on_click({
                            let id = item_id.clone();
                            move |_, window, cx| assign_all(&id, window, cx)
                        }));
                        m
                    });
                    menu.item(
                        PopupMenuItem::new(MENU_SET_WALLPAPER)
                            .submenu(MENU_SET_WALLPAPER, sub),
                    )
                    .separator()
                    .item(PopupMenuItem::new(MENU_OPEN_FOLDER).on_click(|_, window, cx| {
                        window.push_notification(Notification::info(TOAST_OPEN_FOLDER), cx);
                    }))
                    .item(PopupMenuItem::new(MENU_DETAILS).on_click({
                        let id = item_id.clone();
                        move |_, window, cx| {
                            update(window, cx, |g| g.selected = Some(id.clone()));
                        }
                    }))
                    .separator()
                    .item(
                        PopupMenuItem::element(move |_, _| {
                            danger_item(MENU_REMOVE)
                        })
                        .on_click(move |_, window, cx| {
                            remove_item(&item_id2(&item_name, &item_id), window, cx)
                        }),
                    )
                };
                menu
            });

        card = card
            .child(preview(item.art, Some(item.kind), broken, !broken, cx))
            .child(
                div()
                    .relative()
                    .h(px(56.))
                    .pt_2()
                    .pb_2()
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
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
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
        let g = state(cx);
        let visible = g.visible_items();
        if g.library.is_empty() {
            return empty_library(cx);
        }
        if visible.is_empty() {
            return empty_search(cx);
        }
        let cards: Vec<AnyElement> = visible
            .into_iter()
            .filter_map(|i| g.library.get(i).cloned())
            .map(|item| self.card_by_item(item, cx))
            .collect();
        div()
            .id("library-grid")
            .flex_1()
            .overflow_y_scroll()
            .p_4()
            .px(px(14.))
            .child(div().flex().flex_wrap().gap(px(14.)).children(cards))
            .into_any_element()
    }

    fn card_by_item(&self, item: LibraryItem, cx: &mut Context<Self>) -> AnyElement {
        self.card(0, &item, cx)
    }

    fn statusbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let g = state(cx);
        let we_count = g.library.iter().filter(|w| w.we).count();
        let mut count_text = format!("{} 项 · WE {}", g.library.len(), we_count);
        if let Some(sel) = &g.selected {
            if let Some(item) = g.library.iter().find(|w| &w.id == sel) {
                count_text = format!(
                    "{}  ·  {} · {}",
                    count_text,
                    item.name,
                    item.meta
                );
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
            .child(div().flex_1().child(HINT_LIBRARY))
            .into_any_element()
    }

    /// 拖拽中的全屏投放区（原型行为：不切页签，覆盖层投放）。
    fn drag_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let dragging = cx
            .active_drag
            .as_ref()
            .and_then(|d| d.value.downcast_ref::<CardDrag>().is_some())
            .unwrap_or(false);
        if !dragging {
            return None;
        }
        let t = tokens(cx);
        let targets: Vec<AnyElement> = state(cx)
            .monitors
            .iter()
            .enumerate()
            .map(|(i, m)| {
                div()
                    .id(SharedString::from(format!("dz-{i}")))
                    .w(px(180.))
                    .h(px(120.))
                    .rounded(px(12.))
                    .bg(t.elevated)
                    .border_1()
                    .border_color(t.hairline)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .drag_over::<CardDrag>(|s, _, _, _| {
                        s.border_color(t.accent)
                            .border_dashed()
                            .bg(t.accent_soft)
                    })
                    .on_drop(move |drag: &CardDrag, window, cx| {
                        assign_item(&drag.item_id, i, window, cx);
                    })
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .text_color(t.text1)
                            .child(m.name.to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text2)
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
            .child(toolbar)
            .child(
                div()
                    .flex_1()
                    .relative()
                    .min_h_0()
                    .child(grid)
                    .when_(overlay.is_some(), |d| d.child(overlay.unwrap())),
            )
            .child(statusbar)
            .text_color(t.text1)
    }
}

/* ---------- 动作辅助（菜单/拖放共享） ---------- */

fn item_id2(_name: &str, id: &SharedString) -> SharedString {
    id.clone()
}

fn assign_item(item_id: &str, monitor: usize, window: &mut Window, cx: &mut App) {
    let outcome = cx.update_global::<GessoState, _>(|g, _| {
        g.assign(monitor, item_id).map(|name| name.to_string())
    });
    match outcome {
        Ok(Ok(name)) => {
            let target = cx.update_global::<GessoState, _>(|g, _| g.monitor_name(monitor));
            window.refresh();
            window.push_notification(
                Notification::success(format!(
                    "{}",
                    TOAST_ASSIGN.replace("{}", &name).replacen("{}", &target, 1)
                )),
                cx,
            );
        }
        Ok(Err(e)) => window.push_notification(Notification::warning(e), cx),
        Err(_) => {}
    }
}

fn assign_all(item_id: &str, window: &mut Window, cx: &mut App) {
    let count = state(cx).monitors.len();
    let mut last_name = String::new();
    for i in 0..count {
        let r = cx.update_global::<GessoState, _>(|g, _| {
            g.assign(i, item_id).map(|name| name.to_string())
        });
        if let Ok(Ok(name)) = r {
            last_name = name;
        }
    }
    window.refresh();
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
    window.push_notification(
        Notification::info(format!("{}", TOAST_REMOVED.replace("{}", &name))),
        cx,
    );
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

fn drag_art(item_id: &str) -> super::data::Art {
    state(&mut phantom_app())
        .library
        .iter()
        .find(|w| w.id.as_ref() == item_id)
        .map(|w| w.art)
        .unwrap_or(super::data::Art { from: 0x26262A, to: 0x0E0F13 })
}

/// on_drag 构造器里拿不到 App：ghost 渐变退化为中性色即可。
fn phantom_app() -> &'static mut App {
    unreachable!("drag ghost 不读取全局")
}

/* ---------- 空状态 ---------- */

fn empty_library(cx: &mut Context<LibraryView>) -> AnyElement {
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
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .text_color(t.text1)
                .child(EMPTY_LIBRARY_TITLE),
        )
        .child(div().text_size(px(13.)).text_color(t.text2).child(EMPTY_LIBRARY_DESC))
        .child(
            h_flex().gap(px(10.))
                .child(
                    gpui_kit::component::button::Button::new("empty-import")
                        .label(BTN_IMPORT_FILE)
                        .primary()
                        .on_click(|_, window, cx| {
                            let name = cx.update_global::<GessoState, _>(|g, _| g.import_demo());
                            window.push_notification(
                                Notification::success(format!("已导入「{name}」（演示）")),
                                cx,
                            );
                        }),
                )
                .child(
                    gpui_kit::component::button::Button::new("empty-samples")
                        .label(BTN_BROWSE_SAMPLES)
                        .secondary()
                        .on_click(|_, window, cx| {
                            let name = cx.update_global::<GessoState, _>(|g, _| g.import_demo());
                            window.push_notification(
                                Notification::success(format!("已应用内置样例「{name}」（演示）")),
                                cx,
                            );
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
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .text_color(t.text1)
                .child(format!("没有匹配「{query}」的壁纸")),
        )
        .child(
            gpui_kit::component::button::Button::new("clear-search")
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
    art: super::data::Art,
}

impl Render for CardGhost {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(preview(self.art, None, false, false, cx))
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

/* ---------- 辅助 ---------- */

trait WhenExt: Sized {
    fn when_(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self;
}
impl<E: Sized> WhenExt for E {
    fn when_(self, cond: bool, f: impl FnOnce(Self) -> Self) -> Self {
        if cond {
            f(self)
        } else {
            self
        }
    }
}

fn play_state_visual_unused() {
    let _ = play_state_visual;
}
