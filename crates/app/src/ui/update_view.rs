//! 「软件更新」小窗（§4.7）：一窗承载检查/下载/交付六态。
//!
//! 形态与 `url_import` 同款独立小窗；状态真源 = `app::update` 模块全局，经
//! `GessoState.update` 快照投影渲染——引擎主循环在状态代数变化时 `refresh_windows`，
//! 本窗随之重绘，不持有任何定时器。按钮写操作走引擎动作队列（API.md §4）；
//! 打开安装包/浏览器是只读交付动作，直接走系统命令（与「打开日志目录」同惯例）。

use std::sync::Mutex;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::progress::{Progress, ProgressCircle};
use gpui_kit::component::{h_flex, v_flex, Sizable as _, Size};
use gpui_kit::gpui::{
    div, px, size, AnyElement, App, AppContext as _, Bounds, Context, FocusHandle, Focusable,
    FontWeight, InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use gpui_kit::AnyWindowHandle;

use super::app_state::state;
use super::data::UpdatePhase;
use super::strings::*;
use super::theme::tokens;

/// 小窗句柄（已开则激活，避免叠窗；窗口被用户关闭后 GPUI 移除句柄 → 重建）。
static UPDATE_WINDOW: Mutex<Option<AnyWindowHandle>> = Mutex::new(None);

pub struct UpdateModal {
    focus: FocusHandle,
}

impl Focusable for UpdateModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// 打开（或激活已打开的）更新小窗。托盘与设置页共用入口。
pub fn open(cx: &mut App) {
    let existing = UPDATE_WINDOW.lock().ok().and_then(|g| *g);
    if let Some(h) = existing {
        let activated = h
            .update(cx, |_: gpui_kit::gpui::AnyView, window, cx: &mut App| {
                cx.activate(true);
                window.activate_window();
            })
            .is_ok();
        if activated {
            return;
        }
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(480.), px(320.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from(format!(
                "{} · {}",
                APP_NAME(),
                UPDATE_TITLE()
            ))),
            ..Default::default()
        }),
        ..Default::default()
    };
    if let Ok((handle, _)) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            let _ = window;
            UpdateModal {
                focus: cx.focus_handle(),
            }
        })
    }) {
        if let Ok(mut g) = UPDATE_WINDOW.lock() {
            *g = Some(handle);
        }
    }
}

fn close(window: &mut Window) {
    // 失败态随窗口关闭重置（重开从「检查更新」干净起步，不停留在旧错误）
    crate::update::reset_failure();
    window.remove_window();
}

impl Render for UpdateModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let u = state(cx).update.clone();

        let title = div()
            .text_size(px(15.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(UPDATE_TITLE());

        // —— 各态正文与按钮（§4.7 状态表；字符串克隆提前，闭包各自持有所需字段）——
        let mut body: Vec<AnyElement> = Vec::new();
        let mut footer: Vec<AnyElement> = Vec::new();
        match u.phase {
            UpdatePhase::Idle | UpdatePhase::Checking => {
                body.push(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(ProgressCircle::new("upd-checking").loading(true))
                        .child(div().text_color(t.text2).child(UPD_CHECKING()))
                        .into_any_element(),
                );
                footer.push(close_button());
            }
            UpdatePhase::UpToDate => {
                body.push(
                    div()
                        .text_size(px(12.5))
                        .text_color(t.text2)
                        .child(update_uptodate_desc(&u.current_version))
                        .into_any_element(),
                );
                footer.push(
                    Button::new("upd-close")
                        .label(UPD_BTN_CLOSE())
                        .with_size(Size::Small)
                        .on_click(|_, window, _| close(window))
                        .into_any_element(),
                );
            }
            UpdatePhase::Available => {
                body.push(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child(update_found_title(&u.version))
                        .into_any_element(),
                );
                // release notes：限高滚动（NOTES：overflow_y_scroll 必须在 .id() 之后）
                body.push(
                    div()
                        .id("upd-notes")
                        .h(px(96.))
                        .overflow_y_scroll()
                        .text_size(px(12.))
                        .text_color(t.text2)
                        .child(if u.notes.is_empty() {
                            SharedString::from(UPD_NOTES_EMPTY())
                        } else {
                            u.notes.clone()
                        })
                        .into_any_element(),
                );
                body.push(
                    div()
                        .text_size(px(12.))
                        .text_color(t.text2)
                        .child(update_asset_line(&u.asset_name, u.asset_size))
                        .into_any_element(),
                );
                footer.push(
                    Button::new("upd-later")
                        .label(UPD_BTN_LATER())
                        .text()
                        .with_size(Size::Small)
                        .on_click(|_, window, _| close(window))
                        .into_any_element(),
                );
                footer.push(release_page_button());
                footer.push(
                    Button::new("upd-download")
                        .label(UPD_BTN_DOWNLOAD())
                        .primary()
                        .with_size(Size::Small)
                        .on_click(|_, _, _| {
                            crate::engine::enqueue(crate::engine::EngineAction::UpdateDownload);
                        })
                        .into_any_element(),
                );
            }
            UpdatePhase::Downloading => {
                body.push(
                    div()
                        .text_size(px(12.5))
                        .text_color(t.text2)
                        .child(UPD_DOWNLOADING())
                        .into_any_element(),
                );
                // kit 进度值 = 0..100 百分比；总大小未知走不确定滑动动画
                let bar = if u.asset_size > 0 {
                    Progress::new("upd-bar").value(u.downloaded as f32 / u.asset_size as f32 * 100.)
                } else {
                    Progress::new("upd-bar").loading(true)
                };
                body.push(bar.into_any_element());
                body.push(
                    div()
                        .text_size(px(12.))
                        .text_color(t.text2)
                        .child(update_dl_progress(u.downloaded, u.asset_size))
                        .into_any_element(),
                );
                footer.push(
                    Button::new("upd-cancel")
                        .label(UPD_BTN_CANCEL_DL())
                        .secondary()
                        .with_size(Size::Small)
                        .on_click(|_, _, _| {
                            crate::engine::enqueue(
                                crate::engine::EngineAction::UpdateCancelDownload,
                            );
                        })
                        .into_any_element(),
                );
            }
            UpdatePhase::Ready => {
                body.push(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child(UPD_READY_TITLE())
                        .into_any_element(),
                );
                body.push(
                    div()
                        .text_size(px(12.5))
                        .text_color(t.text2)
                        .child(UPD_READY_DESC())
                        .into_any_element(),
                );
                body.push(
                    div()
                        .text_size(px(12.))
                        .text_color(t.text2)
                        .child(u.file_path.clone())
                        .into_any_element(),
                );
                let path = u.file_path.clone();
                footer.push(
                    Button::new("upd-close")
                        .label(UPD_BTN_CLOSE())
                        .text()
                        .with_size(Size::Small)
                        .on_click(|_, window, _| close(window))
                        .into_any_element(),
                );
                footer.push(
                    Button::new("upd-reveal")
                        .label(UPD_BTN_REVEAL())
                        .secondary()
                        .with_size(Size::Small)
                        .on_click(move |_, _, _| crate::update::reveal(&path))
                        .into_any_element(),
                );
                let path = u.file_path.clone();
                footer.push(
                    Button::new("upd-install")
                        .label(UPD_BTN_INSTALL())
                        .primary()
                        .with_size(Size::Small)
                        .on_click(move |_, _, _| crate::update::open_installer(&path))
                        .into_any_element(),
                );
            }
            UpdatePhase::Failed => {
                body.push(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.danger)
                        .child(UPD_FAILED_TITLE())
                        .into_any_element(),
                );
                body.push(
                    div()
                        .text_size(px(12.))
                        .text_color(t.danger)
                        .child(update_failed_detail(&u.error))
                        .into_any_element(),
                );
                footer.push(
                    Button::new("upd-close")
                        .label(UPD_BTN_CLOSE())
                        .text()
                        .with_size(Size::Small)
                        .on_click(|_, window, _| close(window))
                        .into_any_element(),
                );
                footer.push(release_page_button());
                let retry_download = u.can_retry_download;
                footer.push(
                    Button::new("upd-retry")
                        .label(if retry_download {
                            UPD_BTN_RETRY()
                        } else {
                            BTN_CHECK_UPDATE()
                        })
                        .primary()
                        .with_size(Size::Small)
                        .on_click(move |_, _, _| {
                            if retry_download {
                                crate::engine::enqueue(crate::engine::EngineAction::UpdateDownload);
                            } else {
                                crate::engine::enqueue(crate::engine::EngineAction::UpdateCheck {
                                    open_modal: false,
                                });
                            }
                        })
                        .into_any_element(),
                );
            }
        }

        v_flex()
            .size_full()
            .bg(t.surface)
            .text_color(t.text1)
            .p(px(20.))
            .gap_3()
            .child(title)
            .children(body)
            .child(div().flex_1())
            .child(h_flex().justify_end().gap_2().children(footer))
    }
}

fn close_button() -> gpui_kit::gpui::AnyElement {
    Button::new("upd-close")
        .label(UPD_BTN_CLOSE())
        .text()
        .with_size(Size::Small)
        .on_click(|_, window, _| close(window))
        .into_any_element()
}

fn release_page_button() -> gpui_kit::gpui::AnyElement {
    Button::new("upd-release")
        .label(UPD_BTN_RELEASE())
        .secondary()
        .with_size(Size::Small)
        .on_click(|_, _, _| crate::update::open_release_page())
        .into_any_element()
}
