//! gesso-app —— M0 骨架：GPUI 管理窗口三页签空壳。
//! 视觉契约：DESIGN.md（gesso-design/）；本里程碑只交付结构与主题 token。

use gpui::{
    px, size, App, Application, Bounds, Context, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions,
};

/// 主题 token（DESIGN.md，亮色组；M0 静态，双主题随 M3 接入系统监听）
mod theme {
    use gpui::Rgba;
    pub const SURFACE: Rgba = gpui::rgb(0xFFFFFF);
    pub const PANEL: Rgba = gpui::rgb(0xF5F5F7);
    pub const TEXT_1: Rgba = gpui::rgb(0x1D1D1F);
    pub const TEXT_2: Rgba = gpui::rgb(0x6E6E73);
    pub const ACCENT: Rgba = gpui::rgb(0x316EF5);
    pub const ACCENT_SOFT: Rgba = gpui::rgba(0x316EF51Eu32); // 12%
    pub const HAIRLINE: Rgba = gpui::rgb(0xE5E5EA);
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
            gpui::div()
                .id(("tab", t.label()))
                .px(4.)
                .py(1.)
                .rounded_md()
                .text_color(if selected { theme::ACCENT } else { theme::TEXT_2 })
                .bg(if selected { theme::ACCENT_SOFT } else { theme::SURFACE })
                .cursor_pointer()
                .child(SharedString::from(t.label()))
                .on_click(move |this: &mut Shell, _ev, _win, _cx| this.tab = t)
        });

        gpui::div()
            .id("root")
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::SURFACE)
            .text_color(theme::TEXT_1)
            .text_size(px(13.))
            .font_family("System")
            // 顶栏
            .child(
                gpui::div()
                    .id("titlebar")
                    .flex()
                    .items_center()
                    .gap_3()
                    .h(px(44.))
                    .px_4()
                    .border_b_1()
                    .border_color(theme::HAIRLINE)
                    .child(gpui::div().font_weight(gpui::FontWeight::SEMIBOLD).child("Gesso"))
                    .child(gpui::div().flex().gap_1().children(tabs)),
            )
            // 内容区
            .child(match active {
                Tab::Library => library_placeholder(),
                Tab::Monitors => monitors_placeholder(),
                Tab::Settings => settings_placeholder(),
            })
    }
}

fn library_placeholder() -> gpui::Div {
    let cards = ["nebula · VIDEO", "waves · SHADER", "clock · WEB", "sakura · WE"]
        .map(|t| card_placeholder(t));
    gpui::div()
        .id("library")
        .flex_1()
        .flex()
        .flex_wrap()
        .gap_4()
        .p_4()
        .bg(theme::SURFACE)
        .children(cards)
}

fn card_placeholder(title: &str) -> gpui::Div {
    gpui::div()
        .id(("card", title))
        .w(px(196.))
        .rounded_xl()
        .border_1()
        .border_color(theme::HAIRLINE)
        .bg(theme::PANEL)
        .overflow_hidden()
        .child(gpui::div().h(px(110.)).bg(gpui::rgb(0x0E0F13)))
        .child(
            gpui::div()
                .px_3()
                .py_2()
                .text_color(theme::TEXT_2)
                .child(SharedString::from(title.to_string())),
        )
}

fn monitors_placeholder() -> gpui::Div {
    gpui::div()
        .id("monitors")
        .flex_1()
        .flex()
        .gap_6()
        .p_8()
        .justify_center()
        .children(vec![
            monitor_frame("主显示器", "27″ · 3840×2160"),
            monitor_frame("副显示器", "24″ · 1920×1080"),
        ])
}

fn monitor_frame(name: &str, sub: &str) -> gpui::Div {
    gpui::div()
        .id(("monitor", name))
        .flex()
        .flex_col()
        .gap_2()
        .min_w(px(280.))
        .child(gpui::div().text_size(px(15.)).font_weight(gpui::FontWeight::SEMIBOLD).child(name))
        .child(gpui::div().text_color(theme::TEXT_2).child(sub))
        .child(
            gpui::div()
                .id(("mt", name))
                .aspect_ratio(16. / 9.)
                .rounded_lg()
                .border_2()
                .border_color(theme::HAIRLINE)
                .bg(gpui::rgb(0x0E0F13)),
        )
}

fn settings_placeholder() -> gpui::Div {
    let rows = ["性能", "启动", "联动", "高级"];
    gpui::div()
        .id("settings")
        .flex_1()
        .flex()
        .flex_col()
        .py_2()
        .children(rows.map(|r| {
            gpui::div()
                .id(("set", r))
                .px_6()
                .py_4()
                .border_b_1()
                .border_color(theme::HAIRLINE)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(r)
        }))
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(880.), px(600.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("Gesso")),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new_view(|_| Shell { tab: Tab::Library }),
        )
        .expect("打开管理窗口失败");
        cx.activate(true);
    });
}
