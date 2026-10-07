//! 设置页（§4.5）：分组表单，全部即时生效（无保存按钮；M3 接 core 后设置即写盘）。
//! 危险操作红字 + 内联二次确认。

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex, WindowExt as _};
use gpui_kit::component::{IndexPath, Sizable as _, Size};
use gpui_kit::gpui::prelude::FluentBuilder as _;
use gpui_kit::gpui::{
    div, px, AnyElement, App, AppContext as _, BorrowAppContext as _, Context, Entity, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement, Render, StatefulInteractiveElement as _,
    Styled, Window,
};

use super::app_state::{state, update, GessoState};
use super::data::{Settings, SuspendPolicy};
use super::strings::*;
use super::theme::tokens;
use super::widgets::{select_slot, set_row};

type StringSelect = Entity<SelectState<SearchableVec<String>>>;

pub struct SettingsView {
    fps_cap: StringSelect,
    fullscreen: StringSelect,
    battery: StringSelect,
    startup: StringSelect,
    language: StringSelect,
    weather: StringSelect,
    weather_key_input: Entity<InputState>,
    /// 重置的内联二次确认（§4.5：红字，先点一次进入确认态，3s 后自动退回）。
    reset_armed: bool,
    /// 构建下拉项时的语言序数。SelectState 持有构建时的选项字符串，
    /// 语言切换后须按真源重建（见 render 开头的守卫）。
    built_lang: u8,
}

const POLICY_ITEMS: [SuspendPolicy; 3] = [
    SuspendPolicy::Pause,
    SuspendPolicy::Downclock,
    SuspendPolicy::Ignore,
];

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = state(cx).settings.clone();

        let fps_cap = make_select(
            window,
            cx,
            super::data::FPS_OPTIONS
                .map(|f| format!("{f} fps"))
                .to_vec(),
            super::data::FPS_OPTIONS
                .iter()
                .position(|f| *f == s.fps_cap)
                .unwrap_or(0),
        );
        let fullscreen = make_select(
            window,
            cx,
            POLICY_ITEMS.map(policy_label).map(String::from).to_vec(),
            POLICY_ITEMS
                .iter()
                .position(|p| *p == s.fullscreen)
                .unwrap_or(0),
        );
        let battery = make_select(
            window,
            cx,
            POLICY_ITEMS.map(policy_label).map(String::from).to_vec(),
            POLICY_ITEMS
                .iter()
                .position(|p| *p == s.battery)
                .unwrap_or(0),
        );
        let startup = make_select(
            window,
            cx,
            vec![STARTUP_RESTORE().to_string(), STARTUP_RANDOM().to_string()],
            if s.startup_random { 1 } else { 0 },
        );
        let language = make_select(
            window,
            cx,
            vec![
                LANGUAGE_AUTO().to_string(),
                LANGUAGE_ZH().to_string(),
                LANGUAGE_EN().to_string(),
            ],
            s.language as usize,
        );
        let weather = make_select(
            window,
            cx,
            vec![
                WEATHER_OPEN_METEO().to_string(),
                WEATHER_CUSTOM().to_string(),
            ],
            if s.weather_custom_key { 1 } else { 0 },
        );

        // 下拉确认 → 写回全局（全部即时生效）
        cx.subscribe(
            &fps_cap,
            |_, _, event: &SelectEvent<SearchableVec<String>>, cx| {
                if let SelectEvent::Confirm(Some(v)) = event {
                    if let Ok(fps) = v.trim_end_matches(" fps").parse::<u32>() {
                        cx.update_global::<GessoState, _>(|g, _| g.settings.fps_cap = fps);
                        cx.notify();
                    }
                }
            },
        )
        .detach();
        cx.subscribe(
            &fullscreen,
            |_, _, event: &SelectEvent<SearchableVec<String>>, cx| {
                if let SelectEvent::Confirm(Some(v)) = event {
                    cx.update_global::<GessoState, _>(|g, _| {
                        g.settings.fullscreen = parse_policy(v)
                    });
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe(
            &battery,
            |_, _, event: &SelectEvent<SearchableVec<String>>, cx| {
                if let SelectEvent::Confirm(Some(v)) = event {
                    cx.update_global::<GessoState, _>(|g, _| g.settings.battery = parse_policy(v));
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe(
            &startup,
            |_, _, event: &SelectEvent<SearchableVec<String>>, cx| {
                if let SelectEvent::Confirm(Some(v)) = event {
                    cx.update_global::<GessoState, _>(|g, _| {
                        g.settings.startup_random = v == STARTUP_RANDOM()
                    });
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe(
            &language,
            |_, _, event: &SelectEvent<SearchableVec<String>>, cx| {
                if let SelectEvent::Confirm(Some(v)) = event {
                    let idx = if v == LANGUAGE_ZH() {
                        1
                    } else if v == LANGUAGE_EN() {
                        2
                    } else {
                        0
                    };
                    cx.update_global::<GessoState, _>(|g, _| g.settings.language = idx);
                    // 立即切换运行时语言 + 落盘（重渲染取新文案）
                    super::strings::set_lang(state(cx).settings.to_core_settings().language);
                    persist_settings(cx);
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe(
            &weather,
            |_, _, event: &SelectEvent<SearchableVec<String>>, cx| {
                if let SelectEvent::Confirm(Some(v)) = event {
                    cx.update_global::<GessoState, _>(|g, _| {
                        g.settings.weather_custom_key = v == WEATHER_CUSTOM()
                    });
                    cx.notify();
                }
            },
        )
        .detach();

        let weather_key_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(WEATHER_KEY_PLACEHOLDER()));
        cx.subscribe(&weather_key_input, |_, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let v = input.read(cx).value().to_string();
                cx.update_global::<GessoState, _>(|g, _| g.settings.weather_key = v);
                cx.notify();
            }
        })
        .detach();

        SettingsView {
            fps_cap,
            fullscreen,
            battery,
            startup,
            language,
            weather,
            weather_key_input,
            reset_armed: false,
            built_lang: super::strings::lang(),
        }
    }

    fn group(&self, title: AnyElement, rows: Vec<AnyElement>, cx: &Context<Self>) -> AnyElement {
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

    /// 语言切换后重建带文案的下拉项，并按真源重设选中
    /// （SelectState 持有构建时的选项字符串，不重建则旧语言残留）。
    fn rebuild_localized_selects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = state(cx).settings.clone();
        let policy_items = || POLICY_ITEMS.map(policy_label).map(String::from).to_vec();
        reset_select(
            &self.fullscreen,
            policy_items(),
            POLICY_ITEMS
                .iter()
                .position(|p| *p == s.fullscreen)
                .unwrap_or(0),
            window,
            cx,
        );
        reset_select(
            &self.battery,
            policy_items(),
            POLICY_ITEMS
                .iter()
                .position(|p| *p == s.battery)
                .unwrap_or(0),
            window,
            cx,
        );
        reset_select(
            &self.startup,
            vec![STARTUP_RESTORE().to_string(), STARTUP_RANDOM().to_string()],
            if s.startup_random { 1 } else { 0 },
            window,
            cx,
        );
        reset_select(
            &self.weather,
            vec![
                WEATHER_OPEN_METEO().to_string(),
                WEATHER_CUSTOM().to_string(),
            ],
            if s.weather_custom_key { 1 } else { 0 },
            window,
            cx,
        );
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 语言切换后重建带文案的下拉项（fps 与语言两项的选项文案与语言无关，
        // 无需重建；语言项选项恒为原生名「简体中文 / English」）。
        if super::strings::lang() != self.built_lang {
            self.rebuild_localized_selects(window, cx);
            self.built_lang = super::strings::lang();
        }
        let t = tokens(cx);
        let s = state(cx).settings.clone();

        // —— 性能 ——
        let perf = vec![
            set_row(
                SET_FPS_CAP(),
                None,
                select_slot(
                    Select::new(&self.fps_cap)
                        .with_size(Size::Small)
                        .into_any_element(),
                ),
                cx,
            ),
            set_row(
                SET_FULLSCREEN(),
                Some(SET_FULLSCREEN_DESC()),
                select_slot(
                    Select::new(&self.fullscreen)
                        .with_size(Size::Small)
                        .into_any_element(),
                ),
                cx,
            ),
            set_row(
                SET_BATTERY(),
                None,
                select_slot(
                    Select::new(&self.battery)
                        .with_size(Size::Small)
                        .into_any_element(),
                ),
                cx,
            ),
            set_row(
                SET_IDLE_DOWNCLOCK(),
                Some(SET_IDLE_DESC()),
                Switch::new("set-idle")
                    .checked(s.idle_downclock)
                    .on_click(|checked, window, cx| {
                        update(window, cx, |g| g.settings.idle_downclock = *checked);
                        persist_settings(cx);
                    })
                    .into_any_element(),
                cx,
            ),
        ];

        // —— 启动 ——
        let startup_rows = vec![
            set_row(
                SET_AUTOLAUNCH(),
                None,
                Switch::new("set-autolaunch")
                    .checked(s.autolaunch)
                    .on_click(|checked, window, cx| {
                        update(window, cx, |g| g.settings.autolaunch = *checked);
                        persist_settings(cx);
                        crate::engine::enqueue(crate::engine::EngineAction::SetAutostart(*checked));
                    })
                    .into_any_element(),
                cx,
            ),
            set_row(
                SET_STARTUP_BEHAVIOR(),
                None,
                select_slot(
                    Select::new(&self.startup)
                        .with_size(Size::Small)
                        .into_any_element(),
                ),
                cx,
            ),
            set_row(
                SET_LANGUAGE(),
                None,
                select_slot(
                    Select::new(&self.language)
                        .with_size(Size::Small)
                        .into_any_element(),
                ),
                cx,
            ),
        ];

        // —— 联动（实验） ——
        let mut linkage = vec![set_row(
            SET_WEATHER(),
            Some(SET_WEATHER_DESC()),
            select_slot(
                Select::new(&self.weather)
                    .with_size(Size::Small)
                    .into_any_element(),
            ),
            cx,
        )];
        if s.weather_custom_key {
            linkage.push(set_row(
                "API Key",
                None,
                div()
                    .w(px(220.))
                    .child(
                        Input::new(&self.weather_key_input)
                            .with_size(Size::Small)
                            .cleanable(true),
                    )
                    .into_any_element(),
                cx,
            ));
        }

        // —— 高级 ——
        // 表单行动作钮统一 Small（24px/14px 字）：默认 Medium 是 32px/16px，
        // 在 13px 的设置行里头重脚轻（§1.2 控件 13px 工艺基准）
        let log_btn = Button::new("open-log")
            .label(BTN_OPEN())
            .secondary()
            .with_size(Size::Small)
            .on_click(|_, _, _| {
                let dir = crate::protocol::config_dir();
                let _ = std::process::Command::new("open").arg(&dir).spawn();
            });
        let armed = self.reset_armed;
        // 红字文字按钮（kit 无 danger 文字变体；§4.5 红字 + danger-soft 悬停底）
        // 高度对齐上方 Small 钮（24px），高级区两行控件等高
        let reset_btn = div()
            .id("reset-settings")
            .child(if armed {
                BTN_RESET_CONFIRM()
            } else {
                BTN_RESET()
            })
            .px_3()
            .h(px(24.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .text_size(px(13.))
            .text_color(t.danger)
            .when(armed, |d| {
                d.bg(t.danger_soft).font_weight(FontWeight::MEDIUM)
            })
            .hover(|s| s.bg(t.danger_soft))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.reset_armed {
                    update(window, cx, |g| g.settings = Settings::default());
                    persist_settings(cx);
                    window.push_notification(Notification::info(TOAST_RESET()), cx);
                    this.reset_armed = false;
                } else {
                    this.reset_armed = true;
                }
                cx.notify();
                // 3 秒后自动退回，避免误留确认态
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(3))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if this.reset_armed {
                            this.reset_armed = false;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }));
        let advanced = vec![
            set_row(SET_LOG_DIR(), None, log_btn.into_any_element(), cx),
            set_row(SET_RESET(), None, reset_btn.into_any_element(), cx),
        ];

        v_flex()
            .id("settings-scroll")
            .flex_1()
            .overflow_y_scroll()
            .min_h_0()
            .py_2()
            .text_color(t.text1)
            .child(self.group(div().child(GROUP_PERF()).into_any_element(), perf, cx))
            .child(self.group(
                div().child(GROUP_STARTUP()).into_any_element(),
                startup_rows,
                cx,
            ))
            .child({
                let head = h_flex()
                    .gap_2()
                    .items_baseline()
                    .mb_1()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text1)
                            .child(GROUP_LINKAGE()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text2)
                            .child(GROUP_EXPERIMENTAL()),
                    );
                let rows = v_flex().gap_1().child(head).children(linkage);
                div()
                    .px(px(18.))
                    .py(px(14.))
                    .border_b_1()
                    .border_color(t.hairline)
                    .child(rows)
            })
            .child(self.group(
                div().child(GROUP_ADVANCED()).into_any_element(),
                advanced,
                cx,
            ))
    }
}

/* ---------- 辅助 ---------- */

/// 投影 → 引擎落盘（UpdateSettings 动作；API.md §1 update_settings）。
fn persist_settings(cx: &mut App) {
    let core = state(cx).settings.to_core_settings();
    crate::engine::enqueue(crate::engine::EngineAction::UpdateSettings(core));
}

fn policy_label(p: SuspendPolicy) -> &'static str {
    p.label()
}

fn parse_policy(v: &str) -> SuspendPolicy {
    POLICY_ITEMS
        .iter()
        .copied()
        .find(|p| policy_label(*p) == v)
        .unwrap_or(SuspendPolicy::Pause)
}

fn make_select(
    window: &mut Window,
    cx: &mut Context<SettingsView>,
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

/// 替换下拉项数据并按索引重设选中（语言切换时选项字符串需换语言；
/// set_items 只换数据源，选中值须重设，否则旧语言文本残留在触发器上）。
fn reset_select(
    sel: &StringSelect,
    items: Vec<String>,
    selected: usize,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) {
    sel.update(cx, |st, cx| {
        st.set_items(SearchableVec::new(items), window, cx);
        st.set_selected_index(Some(IndexPath::new(selected)), window, cx);
    });
}
