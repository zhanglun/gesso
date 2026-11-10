//! 「软件更新」小窗（§4.7）：一窗承载检查/下载/交付六态。
//!
//! 形态与 `url_import` 同款独立小窗；状态真源 = `app::update` 模块全局，经
//! `GessoState.update` 快照投影渲染——引擎主循环在状态代数变化时 `refresh_windows`，
//! 本窗随之重绘，不持有任何定时器。按钮写操作走引擎动作队列（API.md §4）；
//! 打开安装包/浏览器是只读交付动作，直接走系统命令（与「打开日志目录」同惯例）。

use std::sync::Mutex;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::progress::{Progress, ProgressCircle};
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable as _, Size};
use gpui_kit::gpui::{
    div, px, size, AnyElement, App, AppContext as _, Bounds, ClipboardItem, Context, FocusHandle,
    Focusable, Font, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement, Pixels,
    Render, SharedString, Size as WinSize, StatefulInteractiveElement as _, Styled, StyledText,
    TextRun, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use gpui_kit::AnyWindowHandle;

use crate::update::{MdBlock, MdSpan};

use super::app_state::state;
use super::data::UpdatePhase;
use super::strings::*;
use super::theme::{tokens, Tokens};

/// 小窗句柄（已开则激活，避免叠窗；窗口被用户关闭后 GPUI 移除句柄 → 重建）。
static UPDATE_WINDOW: Mutex<Option<AnyWindowHandle>> = Mutex::new(None);

pub struct UpdateModal {
    focus: FocusHandle,
    /// 「复制错误详情」点击后的短暂反馈态（1.6s 自动退回，同设置页重置确认的模式）
    copied: bool,
    /// 上一次渲染时的更新阶段：变化才触发窗高调整，避免每帧 resize。
    shown_phase: Option<UpdatePhase>,
}

impl Focusable for UpdateModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// 各状态的紧凑窗高（§4.7 2026-11-10）：正文两三行的态 ≈200px，
/// 带 notes/错误面板的态更宽裕；宽度恒 460。
fn phase_window_size(phase: UpdatePhase) -> WinSize<Pixels> {
    let h = match phase {
        UpdatePhase::Idle | UpdatePhase::Checking | UpdatePhase::UpToDate => px(190.),
        UpdatePhase::Downloading => px(210.),
        UpdatePhase::Ready => px(220.),
        UpdatePhase::Available => px(300.),
        UpdatePhase::Failed => px(260.),
    };
    size(px(460.), h)
}

/// 打开（或激活已打开的）更新小窗。托盘与设置页共用入口。
pub fn open(cx: &mut App) {
    let existing = UPDATE_WINDOW.lock().ok().and_then(|g| *g);
    if let Some(h) = existing {
        let activated = activate_update_window(h, cx);
        if activated {
            return;
        }
    }
    // 初建即按当前状态定高（Available/Ready 可跨窗存活，重开不总是 Idle）。
    let phase = state(cx).update.phase;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            phase_window_size(phase),
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
                copied: false,
                shown_phase: Some(phase),
            }
        })
    }) {
        if let Ok(mut g) = UPDATE_WINDOW.lock() {
            *g = Some(handle);
        }
        // 新建窗口也要在同一次托盘点击中激活；否则第一次只完成建窗，第二次才上屏。
        activate_update_window(handle, cx);
    }
}

fn activate_update_window(handle: gpui_kit::AnyWindowHandle, cx: &mut App) -> bool {
    handle
        .update(cx, |_: gpui_kit::gpui::AnyView, window, cx: &mut App| {
            cx.activate(true);
            window.activate_window();
            window.defer(cx, |window, cx| {
                window.activate_window();
                cx.activate(true);
            });
        })
        .is_ok()
}

fn close(window: &mut Window) {
    // 失败态随窗口关闭重置（重开从「检查更新」干净起步，不停留在旧错误）。
    // 同时丢弃句柄，避免托盘下次激活命中已移除的窗口。
    crate::update::reset_failure();
    if let Ok(mut handle) = UPDATE_WINDOW.lock() {
        *handle = None;
    }
    window.remove_window();
}

impl Render for UpdateModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let u = state(cx).update.clone();

        // 状态切换时收紧/放开窗高（§4.7 2026-11-10）：defer 到本轮渲染后再 resize，
        // 不在 framing 中途改窗口尺寸。
        if self.shown_phase != Some(u.phase) {
            self.shown_phase = Some(u.phase);
            let target = phase_window_size(u.phase);
            window.defer(cx, move |window, _| window.resize(target));
        }

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
                // release notes：markdown 子集渲染（§4.7 2026-11-10），flex_1 占满
                // 剩余高度、限高滚动（NOTES：overflow_y_scroll 必须在 .id() 之后）
                body.push(
                    div()
                        .id("upd-notes")
                        .flex_1()
                        .min_h(px(64.))
                        .overflow_y_scroll()
                        .text_size(px(12.))
                        .text_color(t.text2)
                        .child(if u.notes.is_empty() {
                            div()
                                .child(SharedString::from(UPD_NOTES_EMPTY()))
                                .into_any_element()
                        } else {
                            let font = window.text_style().font();
                            let blocks = crate::update::parse_markdown_notes(&u.notes);
                            v_flex()
                                .gap_1()
                                .children(md_blocks(&blocks, &font, &t))
                                .into_any_element()
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
                // 错误信息是一个可操作的反馈单元：用浅红面板把原因、出路和复制动作
                // 绑在一起，避免「复制错误详情」看起来像一行普通说明文字。
                let err_detail = u.error.clone();
                let copied = self.copied;
                body.push(
                    v_flex()
                        .gap_2()
                        .p_3()
                        .rounded(px(8.))
                        .bg(t.danger_soft)
                        .border_1()
                        .border_color(t.danger.opacity(0.24))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(Icon::new(IconName::TriangleAlert).size_4())
                                .child(
                                    div()
                                        .text_size(px(13.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(t.danger)
                                        .child(UPD_FAILED_TITLE()),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.danger)
                                .child(update_failed_detail(&u.error)),
                        )
                        .child(
                            h_flex()
                                .id("upd-copy-err")
                                .gap_1()
                                .items_center()
                                .text_size(px(12.))
                                .text_color(t.text2)
                                .cursor_pointer()
                                .hover(|s| s.text_color(t.accent))
                                .child(
                                    Icon::new(if copied {
                                        IconName::Check
                                    } else {
                                        IconName::Copy
                                    })
                                    .size_3(),
                                )
                                .child(if copied {
                                    UPD_COPIED()
                                } else {
                                    UPD_COPY_ERROR()
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(format!(
                                        "Gesso {} · {}\n{}",
                                        env!("CARGO_PKG_VERSION"),
                                        UPDATE_TITLE(),
                                        err_detail
                                    )));
                                    this.copied = true;
                                    cx.notify();
                                    cx.spawn(async move |this, cx| {
                                        cx.background_executor()
                                            .timer(std::time::Duration::from_millis(1600))
                                            .await;
                                        let _ = this.update(cx, |this, cx| {
                                            this.copied = false;
                                            cx.notify();
                                        });
                                    })
                                    .detach();
                                })),
                        )
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

        // Available 态的 notes 区已用 flex_1 占满剩余空间，不再需要底部弹性占位。
        let mut layout = v_flex()
            .size_full()
            .bg(t.surface)
            .text_color(t.text1)
            .p(px(20.))
            .gap_3()
            .child(title)
            .children(body);
        if u.phase != UpdatePhase::Available {
            layout = layout.child(div().flex_1());
        }
        layout.child(h_flex().justify_end().gap_2().children(footer))
    }
}

/* ---------- release notes markdown 子集渲染（§4.7 2026-11-10） ---------- */

fn md_blocks(blocks: &[MdBlock], font: &Font, t: &Tokens) -> Vec<AnyElement> {
    blocks
        .iter()
        .map(|b| match b {
            MdBlock::Heading { level, spans } => {
                let size = if *level >= 3 { px(12.5) } else { px(13.5) };
                div()
                    .text_size(size)
                    .text_color(t.text1)
                    .child(md_line(spans, font, FontWeight::SEMIBOLD, t.text1))
                    .into_any_element()
            }
            // 列表项：项目符号独立成格，正文在自身格内换行——换行自然悬挂对齐
            MdBlock::ListItem { spans } => h_flex()
                .pl(px(6.))
                .gap_1()
                .child(div().text_size(px(12.)).text_color(t.text2).child("•"))
                .child(md_line(spans, font, FontWeight::NORMAL, t.text2))
                .into_any_element(),
            MdBlock::Paragraph { spans } => div()
                .text_size(px(12.))
                .text_color(t.text2)
                .child(md_line(spans, font, FontWeight::NORMAL, t.text2))
                .into_any_element(),
        })
        .collect()
}

/// 一行的片段 → 元素。有粗体片段走 `StyledText` + `TextRun` 逐段覆写字重
/// （runs 的字体/颜色以字节长度分段；粗体片段自身不参与断行——发布说明的
/// 粗体都是短词，可接受）；纯文本走普通 div，与既有文本管线一致。
fn md_line(spans: &[MdSpan], font: &Font, base_weight: FontWeight, color: Hsla) -> AnyElement {
    let joined: String = spans
        .iter()
        .map(|s| match s {
            MdSpan::Text(s) | MdSpan::Bold(s) => s.as_str(),
        })
        .collect();
    let has_bold = spans.iter().any(|s| matches!(s, MdSpan::Bold(_)));
    if !has_bold {
        return div()
            .text_color(color)
            .child(SharedString::from(joined))
            .into_any_element();
    }
    let mut runs = Vec::new();
    for span in spans {
        let (s, bold) = match span {
            MdSpan::Text(s) => (s, false),
            MdSpan::Bold(s) => (s, true),
        };
        let mut f = font.clone();
        f.weight = if bold { FontWeight::BOLD } else { base_weight };
        runs.push(TextRun {
            len: s.len(),
            font: f,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        });
    }
    div()
        .text_color(color)
        .child(StyledText::new(SharedString::from(joined)).with_runs(runs))
        .into_any_element()
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
