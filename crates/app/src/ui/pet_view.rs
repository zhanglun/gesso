//! 桌面宠物页：宠物设置与统计的一级入口。

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{v_flex, IndexPath, Sizable as _, Size};
use gpui_kit::gpui::{
    div, px, AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement, Render, StatefulInteractiveElement as _, Styled, Window,
};

use super::app_state::state;
use super::strings::*;
use super::theme::tokens;
use super::widgets::{select_slot, set_row};

type StringSelect = Entity<SelectState<SearchableVec<String>>>;

pub struct PetView {
    activity: StringSelect,
    size: StringSelect,
    reset_armed: bool,
}

impl PetView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let pet = state(cx).pet;
        let activity = make_select(
            window,
            cx,
            vec![
                PET_ACTIVITY_QUIET().to_string(),
                PET_ACTIVITY_NORMAL().to_string(),
                PET_ACTIVITY_LIVELY().to_string(),
            ],
            activity_index(pet.activity),
        );
        let size = make_select(
            window,
            cx,
            vec![
                PET_SIZE_SMALL().to_string(),
                PET_SIZE_MEDIUM().to_string(),
                PET_SIZE_LARGE().to_string(),
            ],
            size_index(pet.size),
        );

        cx.subscribe(
            &activity,
            |_, _, event: &SelectEvent<SearchableVec<String>>, _cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    let activity = match value {
                        v if v == PET_ACTIVITY_QUIET() => gesso_core::PetActivity::Quiet,
                        v if v == PET_ACTIVITY_LIVELY() => gesso_core::PetActivity::Lively,
                        _ => gesso_core::PetActivity::Normal,
                    };
                    crate::engine::enqueue(crate::engine::EngineAction::SetPetActivity(activity));
                }
            },
        )
        .detach();
        cx.subscribe(
            &size,
            |_, _, event: &SelectEvent<SearchableVec<String>>, _cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    let size = match value {
                        v if v == PET_SIZE_SMALL() => gesso_core::PetSize::Small,
                        v if v == PET_SIZE_LARGE() => gesso_core::PetSize::Large,
                        _ => gesso_core::PetSize::Medium,
                    };
                    crate::engine::enqueue(crate::engine::EngineAction::SetPetSize(size));
                }
            },
        )
        .detach();

        Self {
            activity,
            size,
            reset_armed: false,
        }
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
                            .font_weight(gpui_kit::gpui::FontWeight::SEMIBOLD)
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
            .child(
                Button::new("pet-adopt")
                    .label(TRAY_PET_ADOPT())
                    .primary()
                    .with_size(Size::Small)
                    .on_click(|_, _, _| {
                        crate::engine::enqueue(crate::engine::EngineAction::OpenPetAdoption);
                    }),
            )
            .into_any_element()
    }
}

impl Render for PetView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pet = state(cx).pet;
        if !pet.adopted {
            return self.render_unadopted(cx);
        }
        let stats = pet.stats;
        let settings = vec![
            set_row(
                SET_PET_ACTIVITY(),
                Some(SET_PET_ACTIVITY_DESC()),
                select_slot(
                    Select::new(&self.activity)
                        .with_size(Size::Small)
                        .into_any_element(),
                ),
                cx,
            ),
            set_row(
                SET_PET_SIZE(),
                None,
                select_slot(
                    Select::new(&self.size)
                        .with_size(Size::Small)
                        .into_any_element(),
                ),
                cx,
            ),
            set_row(
                SET_PET_VISIBLE(),
                None,
                Switch::new("pet-visible")
                    .checked(pet.visible)
                    .on_click(|_, window, _| {
                        crate::engine::enqueue(crate::engine::EngineAction::TogglePetVisibility);
                        window.refresh();
                    })
                    .into_any_element(),
                cx,
            ),
        ];
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
        v_flex()
            .id("pet-scroll")
            .flex_1()
            .overflow_y_scroll()
            .min_h_0()
            .py_2()
            .child(self.group(GROUP_PET(), settings, cx))
            .child(self.group(PET_STATS(), stats_rows, cx))
            .child(self.group(
                GROUP_ADVANCED(),
                vec![set_row(
                    PET_RESET(),
                    Some(PET_RESET_CONFIRM()),
                    Button::new("pet-reset")
                        .label(if self.reset_armed {
                            PET_RESET_CONFIRM()
                        } else {
                            PET_RESET()
                        })
                        .secondary()
                        .with_size(Size::Small)
                        .on_click(cx.listener(|view, _, _, cx| {
                            if view.reset_armed {
                                view.reset_armed = false;
                                crate::engine::enqueue(
                                    crate::engine::EngineAction::ResetPetData,
                                );
                            } else {
                                view.reset_armed = true;
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                    cx,
                )],
                cx,
            ))
            .into_any_element()
    }
}

fn make_select(
    window: &mut Window,
    cx: &mut Context<PetView>,
    items: Vec<String>,
    selected: usize,
) -> StringSelect {
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(items),
            Some(IndexPath::new(selected)),
            window,
            cx,
        )
    })
}

fn activity_index(activity: gesso_core::PetActivity) -> usize {
    match activity {
        gesso_core::PetActivity::Quiet => 0,
        gesso_core::PetActivity::Normal => 1,
        gesso_core::PetActivity::Lively => 2,
    }
}

fn size_index(size: gesso_core::PetSize) -> usize {
    match size {
        gesso_core::PetSize::Small => 0,
        gesso_core::PetSize::Medium => 1,
        gesso_core::PetSize::Large => 2,
    }
}
