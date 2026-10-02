//! gesso-app —— M0 骨架：管理窗口三页签空壳。
//!
//! UI 栈：gpui-kit 0.7（re-export gpui-pre 快照，runtime_shaders 免 Xcode）。
//! 视觉契约：gesso-design/DESIGN.md。M0 只交付结构；kit 主题映射在 M0.5/M3 落地。

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::*;
use gpui_kit::gpui::{px, size, AnyElement, Bounds, SharedString, TitlebarOptions, WindowBounds};
use gpui_kit::*;

/// 主题 token（DESIGN.md 亮色组；M0 静态，M3 换 kit 语义主题 + 系统监听）
mod theme {
    use gpui_kit::gpui::{rgb, Rgba};
    pub fn surface() -> Rgba {
        rgb(0xFFFFFF)
    }
    pub fn panel() -> Rgba {
        rgb(0xF5F5F7)
    }
    pub fn text_2() -> Rgba {
        rgb(0x6E6E73)
    }
    pub fn hairline() -> Rgba {
        rgb(0xE5E5EA)
    }
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

struct Shell;

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = cx.global::<TabGlobal>().tab;

        let tabs = [Tab::Library, Tab::Monitors, Tab::Settings].map(|t| {
            let selected = t == active;
            let button = Button::new(SharedString::from(format!("tab-{}", t.label())))
                .label(t.label());
            let button = if selected {
                button.primary()
            } else {
                button.ghost()
            };
            button.on_click(move |_, window, cx| {
                cx.update_global::<TabGlobal, _>(|g, _| g.tab = t);
                window.refresh();
            })
        });

        div()
            .v_flex()
            .size_full()
            .bg(theme::surface())
            .text_size(px(13.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .h(px(44.))
                    .px_4()
                    .border_b_1()
                    .border_color(theme::hairline())
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

/// M0 过渡：页签状态用全局容器，M3 接入 kit 主题与 Entity 状态后移除。
struct TabGlobal {
    tab: Tab,
}

impl Default for TabGlobal {
    fn default() -> Self {
        Self { tab: Tab::Library }
    }
}

thread_local! {
    static MAIN_WINDOW: std::cell::RefCell<Option<WindowHandle<()>>> = const { std::cell::RefCell::new(None) };
}

impl Global for TabGlobal {}

fn library_placeholder() -> AnyElement {
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
                .id(SharedString::from(t))
                .w(px(196.))
                .rounded_xl()
                .border_1()
                .border_color(theme::hairline())
                .bg(theme::panel())
                .overflow_hidden()
                .child(div().h(px(110.)).bg(gpui_kit::gpui::rgb(0x0E0F13)))
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .text_color(theme::text_2())
                        .child(SharedString::from(t)),
                )
        }))
        .into_any_element()
}

fn monitors_placeholder() -> AnyElement {
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
                .id(SharedString::from(name))
                .v_flex()
                .gap_2()
                .min_w(px(280.))
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name),
                )
                .child(div().text_color(theme::text_2()).child(sub))
                .child(
                    div()
                        .id(SharedString::from(format!("mt-{name}")))
                        .aspect_ratio(16. / 9.)
                        .rounded_lg()
                        .border_2()
                        .border_color(theme::hairline())
                        .bg(gpui_kit::gpui::rgb(0x0E0F13)),
                )
        }))
        .into_any_element()
}

fn settings_placeholder() -> AnyElement {
    let groups = ["性能", "启动", "联动", "高级"];
    div()
        .id("settings")
        .flex_1()
        .v_flex()
        .py_2()
        .children(groups.map(|g| {
            div()
                .id(SharedString::from(g))
                .px_6()
                .py_4()
                .border_b_1()
                .border_color(theme::hairline())
                .font_weight(FontWeight::SEMIBOLD)
                .child(g)
        }))
        .into_any_element()
}

fn main() {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        cx.set_global(TabGlobal { tab: Tab::Library });

        let (_window_handle, _view) = gpui_kit::open_window(
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
            |_, cx| cx.new(|_| Shell),
        )
        .expect("打开管理窗口失败");
        cx.activate(true);
    });
}
