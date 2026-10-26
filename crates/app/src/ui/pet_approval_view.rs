//! 宠物事件源首连审批小窗（P3-S3）：「应用 X 想给宠物发事件」→ 批准/拒绝。
//!
//! 形态与 `update_view` 同款独立小窗；监听线程发现未授权源 → `PetApprovalNeeded`
//! → 引擎循环开本窗；结论经 `PetApprovalDecision` 回灌引擎（名单 + 持久化 +
//! 镜像 + 唤醒监听者），本窗只发动作不持状态。用户直接关窗不产生结论 →
//! 监听方按超时拒绝并清槽（选型文档 §5），期间其他未知源按 Busy 拒绝。

use std::sync::Mutex;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, Sizable as _, Size};
use gpui_kit::gpui::{
    div, px, size, App, AppContext as _, Bounds, Context, FocusHandle, Focusable, FontWeight,
    IntoElement, ParentElement, Render, SharedString, Styled, TitlebarOptions, Window,
    WindowBounds, WindowOptions,
};
use gpui_kit::AnyWindowHandle;

use super::strings::*;
use super::theme::tokens;
use crate::bridge::pet_events;
use crate::engine;

/// 小窗句柄（已开则激活，避免叠窗；窗口被用户关闭后 GPUI 移除句柄 → 重建）。
static APPROVAL_WINDOW: Mutex<Option<AnyWindowHandle>> = Mutex::new(None);

pub struct PetApprovalModal {
    focus: FocusHandle,
}

impl Focusable for PetApprovalModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// 打开（或激活已打开的）审批小窗。引擎循环在 PetApprovalNeeded 时调用。
pub fn open(cx: &mut App) {
    let existing = APPROVAL_WINDOW.lock().ok().and_then(|g| *g);
    if let Some(h) = existing {
        let activated = h
            .update(cx, |_: gpui_kit::gpui::AnyView, window, cx: &mut App| {
                cx.activate(true);
                window.activate_window();
                // 槽里可能已是新的待审批源（旧源超时清槽后新源入槽）——
                // 激活不触发重绘，强制刷新让按钮闭包与显示一致
                cx.refresh_windows();
            })
            .is_ok();
        if activated {
            return;
        }
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(420.), px(190.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from(format!(
                "{} · {}",
                APP_NAME(),
                PET_EVT_TITLE()
            ))),
            ..Default::default()
        }),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            let _ = window;
            PetApprovalModal {
                focus: cx.focus_handle(),
            }
        })
    }) {
        Ok((handle, _)) => {
            println!("[pet-events] 审批窗已创建");
            if let Ok(mut g) = APPROVAL_WINDOW.lock() {
                *g = Some(handle);
            }
        }
        Err(e) => println!("[pet-events] 审批窗创建失败：{e:?}"),
    }
}

fn close(window: &mut Window) {
    window.remove_window();
}

fn decide(src: String, approved: bool, window: &mut Window) {
    engine::enqueue(engine::EngineAction::PetApprovalDecision { src, approved });
    close(window);
}

impl Render for PetApprovalModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let src = pet_events::pending_src();

        let title = div()
            .text_size(px(15.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(PET_EVT_TITLE());

        let footer: gpui_kit::gpui::AnyElement = match &src {
            Some(src) => {
                let approve_src = src.clone();
                let deny_src = src.clone();
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("pet-evt-deny")
                            .label(PET_EVT_BTN_DENY())
                            .text()
                            .with_size(Size::Small)
                            .on_click(move |_, window, _| {
                                decide(deny_src.clone(), false, window);
                            }),
                    )
                    .child(
                        Button::new("pet-evt-approve")
                            .label(PET_EVT_BTN_APPROVE())
                            .primary()
                            .with_size(Size::Small)
                            .on_click(move |_, window, _| {
                                decide(approve_src.clone(), true, window);
                            }),
                    )
                    .into_any_element()
            }
            None => h_flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("pet-evt-close")
                        .label(UPD_BTN_CLOSE())
                        .text()
                        .with_size(Size::Small)
                        .on_click(|_, window, _| close(window)),
                )
                .into_any_element(),
        };

        let body = match src {
            Some(src) => v_flex()
                .gap_2()
                .child(div().text_color(t.text1).child(PET_EVT_BODY()))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.text1)
                        .child(src),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.text2)
                        .child(PET_EVT_NOTE()),
                ),
            None => v_flex().child(div().text_color(t.text2).child(PET_EVT_NONE())),
        };

        v_flex()
            .size_full()
            .bg(t.panel)
            .p(px(16.))
            .gap_3()
            .child(title)
            .child(body)
            .child(footer)
    }
}
