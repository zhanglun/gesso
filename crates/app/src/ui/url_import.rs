//! 「从 URL 导入」小窗（§4.3 工具条 🔗，2026-10-07 变更）：粘贴 https 网页
//! 地址 → 导入库。远端条目零素材目录（origin=url，entry.source_url），
//! 渲染 = 宿主页 iframe 直装 URL（session::content_spec 分支）。

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{h_flex, v_flex, WindowExt as _};
use gpui_kit::gpui::prelude::FluentBuilder as _;
use gpui_kit::gpui::{
    div, px, size, App, AppContext as _, Bounds, Context, FocusHandle, Focusable, FontWeight,
    IntoElement, ParentElement, Render, SharedString, Styled, TitlebarOptions, Window,
    WindowBounds, WindowOptions,
};
use gpui_kit::AnyWindowHandle;

use super::strings::*;
use super::theme::tokens;

pub struct UrlImport {
    input: gpui_kit::gpui::Entity<InputState>,
    /// 解析失败的内联红字（§1 语义色：danger 只表真错误）
    error: Option<&'static str>,
    focus: gpui_kit::gpui::FocusHandle,
}

impl Focusable for UrlImport {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl UrlImport {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx
            .new(|cx| InputState::new(window, cx).placeholder(URL_IMPORT_PLACEHOLDER()));
        UrlImport {
            input,
            error: None,
            focus: cx.focus_handle(),
        }
    }

    /// 打开导入小窗（库页工具条 🔗）。
    pub fn open(cx: &mut App) -> Option<AnyWindowHandle> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(480.), px(210.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from(format!(
                    "{} · {}",
                    APP_NAME(),
                    URL_IMPORT_TITLE()
                ))),
                ..Default::default()
            }),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| UrlImport::new(window, cx))
        })
        .ok()
        .map(|(handle, _)| handle)
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.input.read(cx).value().to_string();
        match crate::encoding::parse_remote_url(&raw) {
            Ok(url) => {
                let title = crate::encoding::title_from_url(&url);
                crate::engine::enqueue(crate::engine::EngineAction::ImportUrl { url });
                self.error = None;
                window.push_notification(Notification::success(toast_url_imported(&title)), cx);
                window.remove_window();
            }
            Err(_) => {
                self.error = Some(URL_IMPORT_INVALID());
                cx.notify();
            }
        }
    }
}

impl Render for UrlImport {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        v_flex()
            .size_full()
            .bg(t.surface)
            .text_color(t.text1)
            .p(px(20.))
            .gap_3()
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(URL_IMPORT_TITLE()),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(t.text2)
                    .child(URL_IMPORT_DESC()),
            )
            .child(div().child(Input::new(&self.input).cleanable(true)))
            .when_some(self.error, |r, err| {
                r.child(div().text_size(px(12.)).text_color(t.danger).child(err))
            })
            .child(div().flex_1())
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("btn-cancel")
                            .label(BTN_CANCEL())
                            .text()
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        Button::new("btn-ok")
                            .label(BTN_IMPORT())
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm(window, cx)
                            })),
                    ),
            )
    }
}
