//! gesso-app —— M0 骨架：管理窗口三页签空壳。
//!
//! UI 栈：gpui-kit 0.7（re-export gpui-pre 快照）。视觉契约：gesso-design/DESIGN.md。
//! M0 只交付结构与静态 token；kit 组件与主题映射在 M0.5/M3 落地。

use gpui_kit::component::button::Button;
use gpui_kit::component::*;
use gpui_kit::*;

/// 主题 token（DESIGN.md 亮色组；M0 静态，M3 换 kit 语义主题 + 系统监听）
mod theme {
    use gpui_kit::gpui::Rgba;
    pub const SURFACE: Rgba = gpui_kit::gpui::rgb(0xFFFFFF);
    pub const PANEL: Rgba = gpui_kit::gpui::rgb(0xF5F5F7);
    pub const TEXT_2: Rgba = gpui_kit::gpui::rgb(0x6E6E73);
    pub const ACCENT: Rgba = gpui_kit::gpui::rgb(0x316EF5);
    pub const HAIRLINE: Rgba = gpui_kit::gpui::rgb(0xE5E5EA);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Library,
    Monitors,
    Settings,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Library => "壁纸库",
            Tab::Monitors => "显示器",
            Tab::Settings => "设置",
        }
    }
}

struct Shell {
    tab: Tab,
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.tab;

        let tabs = [Tab::Library, Tab::Monitors, Tab::Settings].map(|t| {
            let selected = t == active;
            Button::new(SharedString::from(format!("tab-{}", t.label())))
                .label(t.label())
                .when(selected, |b| b.primary())
                .on_click(move |this: &mut Shell, _ev, _win, _cx| this.tab = t)
        });

        div()
            .v_flex()
            .size_full()
            .bg(theme::SURFACE)
            .text_size(px(13.))
            // 顶栏
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .h(px(44.))
                    .px_4()
                    .border_b_1()
                    .border_color(theme::HAIRLINE)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Gesso"))
                    .child(div().flex().gap_1().children(tabs)),
            )
            .child(match active {
                Tab::Library => library_placeholder(),
                Tab::Monitors => monitors_placeholder(),
                Tab::Settings => settings_placeholder(),
            })
    }
}

fn library_placeholder() -> Div {
    let cards = ["nebula · VIDEO", "waves · SHADER", "clock · WEB", "sakura · WE"];
    div()
        .id("library")
        .flex_1()
        .flex()
        .flex_wrap()
        .gap_4()
        .p_4()
        .children(cards.map(|t| {
            div()
                .id(("card", t))
                .w(px(196.))
                .rounded_xl()
                .border_1()
                .border_color(theme::HAIRLINE)
                .bg(theme::PANEL)
                .overflow_hidden()
                .child(div().h(px(110.)).bg(gpui_kit::gpui::rgb(0x0E0F13)))
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .text_color(theme::TEXT_2)
                        .child(SharedString::from(t.to_string())),
                )
        }))
}

fn monitors_placeholder() -> Div {
    let frames = [("主显示器", "27″ · 3840×2160"), ("副显示器", "24″ · 1920×1080")];
    div()
        .id("monitors")
        .flex_1()
        .flex()
        .gap_6()
        .p_8()
        .justify_center()
        .children(frames.map(|(name, sub)| {
            div()
                .id(("monitor", name))
                .v_flex()
                .gap_2()
                .min_w(px(280.))
                .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(name))
                .child(div().text_color(theme::TEXT_2).child(sub))
                .child(
                    div()
                        .id(("mt", name))
                        .aspect_ratio(16. / 9.)
                        .rounded_lg()
                        .border_2()
                        .border_color(theme::HAIRLINE)
                        .bg(gpui_kit::gpui::rgb(0x0E0F13)),
                )
        }))
}

fn settings_placeholder() -> Div {
    let groups = ["性能", "启动", "联动", "高级"];
    div()
        .id("settings")
        .flex_1()
        .v_flex()
        .py_2()
        .children(groups.map(|g| {
            div()
                .id(("set", g))
                .px_6()
                .py_4()
                .border_b_1()
                .border_color(theme::HAIRLINE)
                .font_weight(FontWeight::SEMIBOLD)
                .child(g)
        }))
}

fn main() {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);

        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(880.), px(600.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("Gesso")),
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|_| Shell { tab: Tab::Library }),
        )
        .expect("打开管理窗口失败");
        cx.activate(true);
    });
}
