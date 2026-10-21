//! macOS 优先的最小领养窗口：物种选择、三候选选择、reroll 和确认。

use gesso_core::{AdoptionEvent, AdoptionState, Candidate, PetSpecies};
use gpui_kit::base::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, Sizable as _, Size};
use gpui_kit::gpui::prelude::FluentBuilder as _;
use gpui_kit::gpui::{
    div, px, rgb, size, AnyElement, App, AppContext as _, Bounds, Context, FocusHandle, Focusable,
    FontWeight, IntoElement, ParentElement, Render, SharedString, Styled, TitlebarOptions, Window,
    WindowBounds, WindowOptions,
};
use gpui_kit::AnyWindowHandle;

use crate::engine::{self, EngineAction};
use crate::pet_template::FrameKind;
use crate::pet_template_candle;
use crate::ui::strings::{
    APP_NAME, BTN_CANCEL, PET_ADOPTED, PET_CANDLE, PET_CHOOSE_CANDIDATE, PET_CHOOSE_SPECIES,
    PET_CONFIRM, PET_REROLL, PET_RETRY, PET_SAVED_DESC, PET_SAVING, PET_TITLE,
};
use crate::ui::theme::tokens;

static WINDOW: std::sync::Mutex<Option<AnyWindowHandle>> = std::sync::Mutex::new(None);

pub struct PetAdoptionView {
    focus: FocusHandle,
    reroll_locked: bool,
}

impl Focusable for PetAdoptionView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

pub fn open(cx: &mut App) {
    if let Some(handle) = WINDOW.lock().ok().and_then(|guard| *guard) {
        if handle
            .update(cx, |_, window, cx| {
                cx.activate(true);
                window.activate_window();
            })
            .is_ok()
        {
            return;
        }
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(560.), px(300.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(SharedString::from(format!(
                "{} · {}",
                APP_NAME(),
                PET_TITLE()
            ))),
            ..Default::default()
        }),
        ..Default::default()
    };
    if let Ok((handle, _)) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            window.focus(&cx.focus_handle(), cx);
            PetAdoptionView {
                focus: cx.focus_handle(),
                reroll_locked: false,
            }
        })
    }) {
        if let Ok(mut guard) = WINDOW.lock() {
            *guard = Some(handle);
        }
    }
}

fn adopt_event(event: AdoptionEvent) {
    engine::enqueue(EngineAction::PetAdoption(event));
}

impl PetAdoptionView {
    fn reroll(&mut self, cx: &mut Context<Self>) {
        if self.reroll_locked {
            return;
        }
        self.reroll_locked = true;
        cx.notify();
        adopt_event(AdoptionEvent::Reroll);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(900))
                .await;
            let _ = this.update(cx, |view, cx| {
                view.reroll_locked = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn close(&mut self, window: &mut Window) {
        adopt_event(AdoptionEvent::Close);
        window.remove_window();
    }
}

fn candidate_art(candidate: &Candidate, cx: &Context<PetAdoptionView>) -> AnyElement {
    let t = tokens(cx);
    let a = &candidate.appearance;
    let candle = candidate.species == PetSpecies::Candle;
    let template = pet_template_candle::candle();
    let palette = template.palette(a.coat_color, a.accent, 0);
    let cells = template.compose(FrameKind::Idle, a.marking, a.body);
    let runs = template.row_runs(&cells);
    let color = |idx: u8| {
        let (r, g, b) = palette[idx as usize];
        rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
    };
    let body = match candidate.species {
        PetSpecies::Cat => t.accent,
        PetSpecies::Jelly => t.accent_soft,
        PetSpecies::Candle => color(1).into(),
    };
    let accent = match candidate.species {
        PetSpecies::Cat => t.text2,
        PetSpecies::Jelly => t.accent,
        PetSpecies::Candle => color(5).into(),
    };
    let scale = 28. + f32::from(a.body) * 4.;
    div()
        .relative()
        .h(px(130.))
        .w(px(150.))
        .bg(t.preview_bg)
        .rounded(px(10.))
        .border_1()
        .border_color(t.preview_frame)
        .flex()
        .items_center()
        .justify_center()
        .child(if candle {
            // 与桌面窗口同一套索引网格 + 调色板：逐行渲染色块行程，
            // 所见即所得（蜡身/火苗/蜡泪/托盘变化全部可见）。
            v_flex()
                .w(px(25. * 1.84))
                .children(runs.into_iter().map(|row| {
                    h_flex()
                        .h(px(1.4))
                        .children(row.into_iter().map(|(len, idx)| {
                            let mut cell = div().w(px(len as f32 * 1.84)).h(px(1.4));
                            if idx != 0 {
                                cell = cell.bg(color(idx));
                            }
                            cell
                        }))
                }))
        } else {
            div()
                .relative()
                .w(px(scale))
                .h(px(scale))
                .rounded(px(if candidate.species == PetSpecies::Cat {
                    18.
                } else {
                    12.
                }))
                .bg(body)
                .child(
                    div()
                        .absolute()
                        .left(px(9. + f32::from(a.face) * 2.))
                        .top(px(9.))
                        .w(px(6. + f32::from(a.eyes)))
                        .h(px(6. + f32::from(a.eyes)))
                        .rounded_full()
                        .bg(accent),
                )
                .child(
                    div()
                        .absolute()
                        .right(px(9. + f32::from(a.marking) * 2.))
                        .top(px(9. + f32::from(a.accent)))
                        .w(px(6.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(t.text1),
                )
        })
        .into_any_element()
}

impl Render for PetAdoptionView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let app = cx.global::<engine::AppState>();
        let adoption = &app.pet_adoption;
        let pet_error = app.pet_error.clone();
        let body: AnyElement = match adoption {
            AdoptionState::NotAdopted => v_flex()
                .gap_3()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(PET_CHOOSE_SPECIES()),
                )
                .child(div().text_color(t.text2).child(PET_TITLE()))
                .child(
                    Button::new("pet-candle")
                        .label(PET_CANDLE())
                        .primary()
                        .with_size(Size::Small)
                        .on_click(|_, _, _| {
                            engine::enqueue(EngineAction::StartPetAdoption {
                                species: PetSpecies::Candle,
                            })
                        }),
                )
                .into_any_element(),
            AdoptionState::CandidatesVisible(set) | AdoptionState::CandidateSelected(set) => {
                let selected = set.selected;
                let cards = set.candidates.iter().enumerate().map(|(index, candidate)| {
                    let label = format!("{} / {}", candidate.name.zh, candidate.name.en);
                    let button = if selected == Some(index) {
                        Button::new(format!("pet-candidate-{index}"))
                            .label(label)
                            .primary()
                    } else {
                        Button::new(format!("pet-candidate-{index}"))
                            .label(label)
                            .secondary()
                    };
                    v_flex()
                        .gap_2()
                        .items_center()
                        .child(candidate_art(candidate, cx))
                        .child(button.with_size(Size::Small).on_click(move |_, _, _| {
                            adopt_event(AdoptionEvent::SelectCandidate(index))
                        }))
                        .into_any_element()
                });
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(PET_CHOOSE_CANDIDATE()),
                    )
                    .child(h_flex().gap_2().children(cards))
                    .when_some(pet_error, |view, error| {
                        view.child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(div().text_color(t.danger).child(error))
                                .child(
                                    Button::new("pet-retry")
                                        .label(PET_RETRY())
                                        .secondary()
                                        .with_size(Size::Small)
                                        .on_click(|_, _, _| {
                                            adopt_event(AdoptionEvent::ConfirmAdoption)
                                        }),
                                ),
                        )
                    })
                    .child(
                        h_flex().gap_2().children([
                            Button::new("pet-reroll")
                                .label(PET_REROLL())
                                .text()
                                .with_size(Size::Small)
                                .disabled(self.reroll_locked)
                                .on_click(cx.listener(|view, _, _, cx| view.reroll(cx))),
                            Button::new("pet-confirm")
                                .label(PET_CONFIRM())
                                .primary()
                                .with_size(Size::Small)
                                .disabled(selected.is_none())
                                .on_click(|_, _, _| adopt_event(AdoptionEvent::ConfirmAdoption)),
                        ]),
                    )
                    .into_any_element()
            }
            AdoptionState::Persisting { .. } => div().child(PET_SAVING()).into_any_element(),
            AdoptionState::Adopted => v_flex()
                .gap_3()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(PET_ADOPTED()))
                .child(div().child(PET_SAVED_DESC()))
                .into_any_element(),
            AdoptionState::ResetConfirm => div().child("正在等待重置确认。").into_any_element(),
        };
        v_flex()
            .size_full()
            .bg(t.surface)
            .text_color(t.text1)
            .p(px(20.))
            .gap_3()
            .child(body)
            .child(div().flex_1())
            .child(
                Button::new("pet-close")
                    .label(BTN_CANCEL())
                    .text()
                    .with_size(Size::Small)
                    .on_click(cx.listener(|view, _, window, _| view.close(window))),
            )
            .into_any_element()
    }
}
