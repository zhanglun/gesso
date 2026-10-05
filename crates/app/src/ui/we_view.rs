//! Wallpaper Engine 工坊浏览窗（M6）：独立小窗，扫描本机已订阅的 WE 素材，
//! 列出 video/web 条目供逐个导入；scene/application 标注不支持，无 Steam 给出说明。
//!
//! 只读本机内容（`crate::we::scan`）；导入入队 `EngineAction::ImportWe`，
//! 已导入的条目在本窗口标记（避免重复点）。

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::gpui::{
    div, px, size, AnyElement, App, AppContext as _, Bounds, Context, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use gpui_kit::AnyWindowHandle;

use crate::engine::{self, EngineAction};
use crate::we::{self, WeEntry, WeKind};

use super::strings::*;
use super::theme::tokens;

pub struct WeView {
    focus: FocusHandle,
    /// None = 未找到 Steam；Some(vec) = 扫描结果（可能空）。
    entries: Option<Vec<WeEntry>>,
    /// 本窗口已点过导入的工坊 ID。
    imported: Vec<String>,
}

impl Focusable for WeView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl WeView {
    fn new(cx: &mut Context<Self>) -> Self {
        let entries = we::find_steam().map(|s| we::scan(&s));
        WeView {
            focus: cx.focus_handle(),
            entries,
            imported: Vec::new(),
        }
    }

    /// 打开工坊浏览窗（库页「导入」旁的 WE 入口）。
    pub fn open(cx: &mut App) -> Option<AnyWindowHandle> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(640.), px(520.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from(format!("{APP_NAME} · Wallpaper Engine"))),
                ..Default::default()
            }),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| cx.new(WeView::new))
            .ok()
            .map(|(h, _)| h)
    }

    fn type_badge(&self, kind: &WeKind, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let (label, fg, bg) = match kind {
            WeKind::Video => ("视频", t.accent, t.accent_soft),
            WeKind::Web => ("网页", t.text2, t.hairline),
            WeKind::Unsupported(_) | WeKind::UnsupportedStr(_) => {
                ("不支持", t.text2, t.hairline)
            }
        };
        div()
            .px(px(7.))
            .py(px(2.))
            .rounded(px(4.))
            .text_size(px(11.))
            .text_color(fg)
            .bg(bg)
            .child(label)
            .into_any_element()
    }

    fn row(&mut self, e: &WeEntry, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let kind = e.kind();
        let supported = matches!(kind, WeKind::Video | WeKind::Web);
        let done = self.imported.contains(&e.workshop_id);

        let title = if e.project.title.is_empty() {
            format!("#{}", e.workshop_id)
        } else {
            e.project.title.clone()
        };

        // 无 on_click 即视觉禁用（gpui-component Button 无公开 disabled）
        let action = if !supported {
            Button::new(format!("we-unsup-{}", e.workshop_id))
                .label(WE_UNSUPPORTED)
                .ghost()
                .into_any_element()
        } else if done {
            Button::new(format!("we-done-{}", e.workshop_id))
                .label(WE_IMPORTED)
                .secondary()
                .into_any_element()
        } else {
            let dir = e.dir.display().to_string();
            let wid = e.workshop_id.clone();
            Button::new(format!("we-import-{}", e.workshop_id))
                .label(WE_IMPORT_BTN)
                .primary()
                .on_click(cx.listener(move |this, _, _, _| {
                    engine::enqueue(EngineAction::ImportWe {
                        dir: dir.clone(),
                        workshop_id: wid.clone(),
                    });
                    this.imported.push(wid.clone());
                }))
                .into_any_element()
        };

        h_flex()
            .w_full()
            .px(px(14.))
            .py(px(10.))
            .gap(px(10.))
            .items_center()
            .border_b_1()
            .border_color(t.hairline)
            .id(format!("we-row-{}", e.workshop_id))
            .hover(|s| s.bg(t.hairline))
            .child(
                v_flex()
                    .flex_1()
                    .gap(px(3.))
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(t.text1)
                            .child(SharedString::from(title)),
                    )
                    .child(self.type_badge(&kind, cx)),
            )
            .child(action)
            .into_any_element()
    }
}

impl Render for WeView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);

        // 先决定空态分支；有内容则 clone 出列表，避免 self 借用冲突
        let snapshot: Option<Vec<WeEntry>> = self.entries.clone();
        let body: AnyElement = match snapshot {
            None => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .child(Icon::new(IconName::Gamepad).size(px(32.)).text_color(t.text2))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(t.text2)
                        .child(WE_NO_STEAM),
                )
                .into_any_element(),
            Some(v) if v.is_empty() => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .child(Icon::new(IconName::FolderOpen).size(px(32.)).text_color(t.text2))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(t.text2)
                        .child(WE_EMPTY),
                )
                .into_any_element(),
            Some(v) => {
                let rows: Vec<AnyElement> =
                    v.iter().map(|e| self.row(e, cx)).collect();
                div()
                    .flex_1()
                    .id("we-list")
                    .overflow_y_scroll()
                    .children(rows)
                    .into_any_element()
            }
        };

        v_flex()
            .size_full()
            .bg(t.panel)
            .child(
                h_flex()
                    .flex_none()
                    .h(px(48.))
                    .px(px(16.))
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(t.hairline)
                    .child(Icon::new(IconName::Gamepad).text_color(t.text2))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(gpui_kit::gpui::FontWeight::SEMIBOLD)
                            .text_color(t.text1)
                            .child(WE_TITLE),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.text2)
                            .child(WE_HINT),
                    ),
            )
            .child(body)
    }
}
