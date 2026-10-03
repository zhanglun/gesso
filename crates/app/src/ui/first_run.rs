//! 首启向导（§4.6）：独立 600×420 小窗，一次性。三步：
//! ① 欢迎「让桌面动起来」 ② 内置样例网格（双击即应用） ③ 完成。
//! 跳过 = 使用纯色桌面，之后不再骚扰。

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{h_flex, v_flex, Icon, WindowExt as _};
use gpui_kit::gpui::prelude::FluentBuilder as _;
use gpui_kit::gpui::{
    div, px, size, AnyElement, App, AppContext as _, Bounds, ClickEvent, Context, FocusHandle,
    Focusable, FontWeight, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, TitlebarOptions, Window, WindowBounds,
    WindowOptions,
};
use gpui_kit::AnyWindowHandle;

use super::app_state::state;
use super::data::{Kind, WIZARD_SAMPLES};
use super::strings::*;
use super::theme::tokens;
use super::widgets::preview;
use gpui_kit::BorrowAppContext as _;

pub struct FirstRun {
    step: usize,
    applied_name: Option<String>,
    focus: gpui_kit::gpui::FocusHandle,
}

impl Focusable for FirstRun {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl FirstRun {
    fn new(cx: &mut Context<Self>) -> Self {
        FirstRun {
            step: 0,
            applied_name: None,
            focus: cx.focus_handle(),
        }
    }

    /// 打开向导小窗（顶栏「重放首启向导」钮 / 正式版首启自动调用）。
    pub fn open(_window: &mut Window, cx: &mut App) -> Option<AnyWindowHandle> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(600.), px(420.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from(format!("{APP_NAME} · 首启向导"))),
                ..Default::default()
            }),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| cx.new(FirstRun::new))
            .ok()
            .map(|(handle, _)| handle)
    }

    fn apply_sample(&mut self, name: &str, cx: &mut Context<Self>) {
        // 双击样例即应用到主显示器并进入完成步（§4.6 省去「下一步」）
        let sample = WIZARD_SAMPLES
            .iter()
            .find(|s| s.name == name)
            .expect("向导样例必然存在");
        let item = super::data::LibraryItem {
            id: format!("sample-{}", sample.name).into(),
            name: sample.name.into(),
            kind: sample.kind,
            we: false,
            meta: "内置样例".into(),
            assigned: None,
            broken: false,
            real: false,
            art: sample.art,
        };
        cx.update_global::<super::app_state::GessoState, _>(|g, _| {
            let id = item.id.clone();
            g.library.push(item);
            let _ = g.assign(g.main_monitor(), id.as_ref());
        });
        cx.notify();
        self.applied_name = Some(name.to_string());
        self.step = 2;
        cx.notify();
    }

    fn step_indicator(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        h_flex()
            .gap(px(6.))
            .children([0usize, 1, 2].map(|i| {
                div()
                    .size(px(6.))
                    .rounded_full()
                    .bg(if i == self.step {
                        t.accent
                    } else {
                        t.hairline2
                    })
                    .into_any_element()
            }))
            .into_any_element()
    }

    fn step_welcome(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let kinds = [
            (Kind::Video, "视频"),
            (Kind::Shader, "Shader"),
            (Kind::Web, "网页"),
        ];
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_4()
            .child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(28.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text1)
                            .child(WIZARD_TITLE),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(t.text2)
                            .child(WIZARD_SUBTITLE),
                    ),
            )
            .child(h_flex().gap_3().children(kinds.map(|(k, label)| {
                v_flex()
                    .w(px(132.))
                    .gap_1()
                    .items_center()
                    .child(preview(sample_art(k), Some(k), false, false, cx))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text2)
                            .child(label.to_string()),
                    )
                    .into_any_element()
            })))
            .into_any_element()
    }

    fn step_pick(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let samples: Vec<AnyElement> = WIZARD_SAMPLES
            .iter()
            .map(|s| {
                v_flex()
                    .id(SharedString::from(format!("sample-{}", s.name)))
                    .w(px(118.))
                    .gap_1()
                    .rounded(px(8.))
                    .p_1()
                    .cursor_pointer()
                    .hover(|st| st.bg(t.panel))
                    .on_click(cx.listener(move |this, click: &ClickEvent, _, cx| {
                        if click.click_count() >= 2 {
                            this.apply_sample(s.name, cx);
                        }
                    }))
                    .child(preview(s.art, Some(s.kind), false, false, cx))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.))
                                    .text_color(t.text1)
                                    .truncate()
                                    .child(s.name.to_string()),
                            )
                            .child(
                                div()
                                    .px_1()
                                    .text_size(px(9.))
                                    .line_height(px(14.))
                                    .text_color(t.text2)
                                    .border_1()
                                    .border_color(t.hairline)
                                    .rounded(px(3.))
                                    .child(WIZARD_SAMPLE_TAG),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        v_flex()
            .flex_1()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text1)
                            .child(WIZARD_PICK_TITLE),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text2)
                            .child(WIZARD_PICK_SUBTITLE),
                    ),
            )
            .child(
                div().child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .justify_center()
                        .children(samples),
                ),
            )
            .into_any_element()
    }

    fn step_done(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let multi = state(cx).monitors.len() > 1;
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .size(px(48.))
                    .rounded_full()
                    .bg(t.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(t.accent)
                    .child(Icon::new(IconName::CircleCheck).size_6()),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.text1)
                    .child(format!(
                        "{}{}",
                        WIZARD_DONE_TITLE,
                        self.applied_name
                            .as_ref()
                            .map(|n| format!("：{n}"))
                            .unwrap_or_default()
                    )),
            )
            .when(multi, |r| {
                r.child(
                    Button::new("wiz-another")
                        .label(WIZARD_ANOTHER)
                        .secondary()
                        .on_click(cx.listener(|this, _, _, cx| {
                            // 多屏再配：回选样例步
                            this.step = 1;
                            this.applied_name = None;
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }
}

impl Render for FirstRun {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let step = self.step;

        let body = match step {
            0 => self.step_welcome(cx),
            1 => self.step_pick(cx),
            _ => self.step_done(cx),
        };

        let foot: Vec<AnyElement> = match step {
            0 => vec![
                Button::new("wiz-start")
                    .label(WIZARD_START)
                    .primary()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.step = 1;
                        cx.notify();
                    }))
                    .into_any_element(),
                Button::new("wiz-skip")
                    .label(WIZARD_SKIP)
                    .text()
                    .on_click(|_, window, cx| {
                        // 跳过 = 使用纯色桌面，不再骚扰
                        window.push_notification(
                            Notification::info("已使用纯色桌面；随时可以从托盘开始"),
                            cx,
                        );
                    })
                    .into_any_element(),
            ],
            1 => vec![
                Button::new("wiz-import")
                    .label(WIZARD_IMPORT_OWN)
                    .text()
                    .on_click(|_, window, cx| {
                        let name = cx.update_global::<super::app_state::GessoState, _>(|g, _| {
                            g.import_demo()
                        });
                        window.refresh();
                        window.push_notification(
                            Notification::success(super::strings::toast_imported(&name)),
                            cx,
                        );
                    })
                    .into_any_element(),
                Button::new("wiz-back")
                    .label("上一步")
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.step = 0;
                        cx.notify();
                    }))
                    .into_any_element(),
            ],
            _ => vec![Button::new("wiz-finish")
                .label(WIZARD_FINISH)
                .primary()
                .on_click(|_, window, _| {
                    // 关闭向导窗口
                    window.remove_window();
                })
                .into_any_element()],
        };

        v_flex()
            .key_context("GessoWizard")
            .track_focus(&self.focus)
            .size_full()
            .bg(t.surface)
            .text_size(px(13.))
            .text_color(t.text1)
            .p_5()
            .child(div().flex().justify_center().child(self.step_indicator(cx)))
            .child(body)
            .child(h_flex().justify_end().gap_2().children(foot))
    }
}

fn sample_art(kind: Kind) -> super::data::Art {
    match kind {
        Kind::Video => super::data::Art {
            from: 0x24345C,
            to: 0x0E0F13,
        },
        Kind::Shader => super::data::Art {
            from: 0x2E2A5E,
            to: 0x0E0F13,
        },
        Kind::Web => super::data::Art {
            from: 0x2A2A30,
            to: 0x101012,
        },
        Kind::Gif => super::data::Art {
            from: 0x2E3E50,
            to: 0x0E0F13,
        },
    }
}
