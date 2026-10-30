//! 桌面宠物页：纯展示（状态/统计）+ 管理入口。
//!
//! 2026-10-30 入口收敛：领养/设置/重置唯一管理面 = Web 控制台
//! （见 docs/design/桌面宠物设计.md v1.1 修订）；本页不再承载写操作。

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{v_flex, Sizable as _, Size};
use gpui_kit::gpui::{
    div, px, AnyElement, Context, FontWeight, IntoElement, ParentElement, Render, Styled, Window,
};

use super::app_state::state;
use super::strings::*;
use super::theme::tokens;
use super::widgets::set_row;

pub struct PetView;

impl PetView {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }

    /// 唯一管理入口：打开本机 Web 控制台（领养动线在其中）。
    fn manage_button(&self) -> Button {
        Button::new("pet-manage")
            .label(TRAY_PET_WEB())
            .primary()
            .with_size(Size::Small)
            .on_click(|_, _, _| {
                crate::engine::enqueue(crate::engine::EngineAction::OpenWebControl);
            })
    }

    fn group(&self, title: &'static str, rows: Vec<AnyElement>, cx: &Context<Self>) -> AnyElement {
        let t = tokens(cx);
        div()
            .px(px(18.))
            .py(px(14.))
            .border_b_1()
            .border_color(t.hairline)
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text1)
                            .mb_1()
                            .child(title),
                    )
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_unadopted(&self, cx: &Context<Self>) -> AnyElement {
        let t = tokens(cx);
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_2()
            .text_color(t.text2)
            .child(
                div()
                    .text_size(px(16.))
                    .text_color(t.text1)
                    .child(PET_TITLE()),
            )
            .child(PET_SAVED_DESC())
            .child(self.manage_button())
            .into_any_element()
    }
}

impl Render for PetView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pet = state(cx).pet;
        if !pet.adopted {
            return self.render_unadopted(cx);
        }
        let status_rows = vec![
            set_row(
                SET_PET_VISIBLE(),
                None,
                div()
                    .text_size(px(13.))
                    .text_color(tokens(cx).text2)
                    .child(if pet.visible {
                        TRAY_PET_HIDE().to_string()
                    } else {
                        TRAY_PET_SHOW().to_string()
                    })
                    .into_any_element(),
                cx,
            ),
            set_row(
                SET_PET_SIZE(),
                None,
                div()
                    .text_size(px(13.))
                    .text_color(tokens(cx).text2)
                    .child(
                        match pet.size {
                            gesso_core::PetSize::Small => PET_SIZE_SMALL(),
                            gesso_core::PetSize::Medium => PET_SIZE_MEDIUM(),
                            gesso_core::PetSize::Large => PET_SIZE_LARGE(),
                        }
                        .to_string(),
                    )
                    .into_any_element(),
                cx,
            ),
            set_row(
                SET_PET_ACTIVITY(),
                Some(SET_PET_ACTIVITY_DESC()),
                div()
                    .text_size(px(13.))
                    .text_color(tokens(cx).text2)
                    .child(
                        match pet.activity {
                            gesso_core::PetActivity::Quiet => PET_ACTIVITY_QUIET(),
                            gesso_core::PetActivity::Normal => PET_ACTIVITY_NORMAL(),
                            gesso_core::PetActivity::Lively => PET_ACTIVITY_LIVELY(),
                        }
                        .to_string(),
                    )
                    .into_any_element(),
                cx,
            ),
        ];
        let stats = pet.stats;
        let stats_rows = vec![set_row(
            PET_STATS(),
            None,
            v_flex()
                .gap_1()
                .child(format!(
                    "{}：{} · {}：{} · {}：{}s",
                    PET_TOUCH_COUNT(),
                    stats.touch_count,
                    PET_POKE_COUNT(),
                    stats.poke_count,
                    PET_COMPANION_TIME(),
                    stats.companion_seconds,
                ))
                .into_any_element(),
            cx,
        )];
        let t = tokens(cx);
        v_flex()
            .flex_1()
            .py_2()
            .child(self.group(GROUP_PET(), status_rows, cx))
            .child(self.group(PET_STATS(), stats_rows, cx))
            .child(
                div()
                    .px(px(18.))
                    .py(px(14.))
                    .flex_1()
                    .items_end()
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .italic()
                                    .text_color(t.text2)
                                    .child(PET_MANAGE_HINT()),
                            )
                            .child(self.manage_button()),
                    ),
            )
            .into_any_element()
    }
}
