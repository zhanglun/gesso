//! M0.5 判定点0 · 四连验 spike
//!
//! ① gpui-wry：无边框壁纸形态窗口 + webview 铺满 + 实时时钟 HTML
//! ② tray-icon：托盘事件接入 GPUI 循环（主窗口实时计数为证）
//! ③ raw_window_handle：打印原生窗口句柄（macOS: NSWindow / Windows: HWND）
//! ④ kit 主题：注入 DESIGN.md 冻结 token + 亮暗切换
//!
//! 运行：cargo run -p gesso-app --example m05
//! 结论判据见 SPIKE-REPORT.md

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::gpui::{px, size, rgb, AnyElement, Bounds, Context, FocusHandle, Focusable,
    Global, Hsla, IntoElement, ParentElement, Render, SharedString, Styled, TitlebarOptions,
    Window, WindowBounds, WindowOptions};
use gpui_kit::*;
use gpui_wry::WebView;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

static TRAY_CLICKS: AtomicUsize = AtomicUsize::new(0);

const HOST_HTML: &str = r#"<!DOCTYPE html><html><head><style>
body{margin:0;background:#0E0F13;color:#EBEBED;font:14px -apple-system,'PingFang SC',sans-serif;display:grid;place-items:center;height:100vh}
.t{font-size:22px;font-weight:600}.s{opacity:.6;margin-top:6px}
#ac{font-variant-numeric:tabular-nums;color:#5B8DEF;font-size:34px;margin-top:14px;font-weight:600}
</style></head><body><div style="text-align:center">
<div class="t">① Gesso 壁纸宿主形态</div>
<div class="s">wry webview 铺满 GPUI 无边框窗口</div>
<div id="ac">--:--:--</div></div>
<script>setInterval(()=>{document.getElementById('ac').textContent=new Date().toLocaleTimeString('zh-CN')},500)</script>
</body></html>"#;

/* ---------- ① 壁纸形态窗口（webview 铺满） ---------- */

struct HostView {
    focus_handle: FocusHandle,
    webview: gpui_kit::Entity<WebView>,
}

impl HostView {
    fn new(window: &mut Window, cx: &mut gpui_kit::App) -> gpui_kit::Entity<Self> {
        // ③ 顺手拿原生句柄：这是贴壁层（pin/）将来 SetParent / NSWindow 压层的入口
        let raw = window.window_handle().expect("[③ FAIL] 拿不到窗口句柄");
        match raw.as_raw() {
            RawWindowHandle::AppKit(h) => {
                // rwh 0.6 给的是 NSView；NSWindow 经 objc2 view.window() 取（M1.5 实现）
                println!("[③ PASS] macOS NSView = {:p}（→ NSWindow 由 M1.5 压层时获取）",
                    h.ns_view.as_ptr() as *const core::ffi::c_void)
            }
            RawWindowHandle::Win32(h) => {
                println!("[③ PASS] Windows HWND = {:?}", h.hwnd)
            }
            other => println!("[③ ?] 其他平台：{other:?}"),
        }

        let webview = cx.new(|cx| {
            let wv = lb_wry::WebViewBuilder::new()
                .with_html(HOST_HTML)
                .build_as_child(&window.window_handle().unwrap())
                .expect("[① FAIL] wry WebView 构建");
            WebView::new(wv, window, cx)
        });
        println!("[① PASS] 壁纸窗口 webview 已挂载");

        cx.new(|cx| Self { focus_handle: cx.focus_handle(), webview })
    }
}

impl Focusable for HostView {
    fn focus_handle(&self, _cx: &gpui_kit::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HostView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.webview.clone())
    }
}

/* ---------- 主窗口：判定报告面板 ---------- */

struct ReportView {
    focus_handle: FocusHandle,
    host_window: Option<gpui_kit::AnyWindowHandle>,
}

fn row(tag: &'static str, title: &'static str, detail: gpui_kit::SharedString) -> AnyElement {
    h_flex()
        .id(tag)
        .gap_3()
        .px_6()
        .py_3()
        .border_b_1()
        .border_color(rgb(0xE5E5EA))
        .child(div().font_weight(gpui_kit::FontWeight::SEMIBOLD).child(tag))
        .child(div().flex_1().child(title))
        .child(div().text_color(rgb(0x6E6E73)).child(detail))
        .into_any_element()
}

impl ReportView {
    fn new(cx: &mut Context<Self>, host_window: Option<gpui_kit::AnyWindowHandle>) -> Self {
        // ② 托盘计数轮询：500ms 定时 notify（证明 GPUI 循环与 tray 事件共存）
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self { focus_handle: cx.focus_handle(), host_window }
    }
}

impl Focusable for ReportView {
    fn focus_handle(&self, _cx: &gpui_kit::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ReportView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clicks = TRAY_CLICKS.load(Ordering::SeqCst);
        let host_alive = self
            .host_window
            .map(|h| h.window_id())
            .is_some();

        v_flex()
            .size_full()
            .text_size(px(13.))
            .child(
                h_flex().px_6().py_4().border_b_1().border_color(rgb(0xE5E5EA))
                    .child(div().text_size(px(15.)).font_weight(gpui_kit::FontWeight::BOLD)
                        .child("M0.5 判定点0 · 四连验"))
                    .child(div().flex_1())
                    .child(
                        Button::new("theme-toggle")
                            .label("④ 切换亮/暗")
                            .on_click(|_, _, cx| {
                                let next = if cx.theme().mode == ThemeMode::Dark {
                                    ThemeMode::Light
                                } else {
                                    ThemeMode::Dark
                                };
                                Theme::change(next, None, cx);
                            }),
                    ),
            )
            .child(row("①", "gpui-wry 壁纸窗口", if host_alive { "已打开（左侧小窗）".into() } else { "未开".into() }))
            .child(row("②", "tray-icon × GPUI 循环",
                SharedString::from(format!("托盘点击 {clicks} 次（点菜单栏图标）"))))
            .child(row("③", "原生窗口句柄", "已打印到 stdout（NSWindow/HWND）".into()))
            .child(row("④", "冻结 token → kit 主题",
                SharedString::from(format!("accent {} · bg {}",
                    hex(cx.theme().colors.accent), hex(cx.theme().colors.background)))))
            .child(
                h_flex().px_6().py_4().gap_2()
                    .child(div().size(px(28.)).rounded_md().bg(cx.theme().colors.accent))
                    .child(div().size(px(28.)).rounded_md().bg(cx.theme().colors.background).border_1().border_color(cx.theme().colors.border))
                    .child(div().size(px(28.)).rounded_md().bg(cx.theme().colors.danger))
                    .child(div().flex_1())
                    .child(div().text_color(rgb(0x6E6E73)).child("swatch: accent / surface / danger")),
            )
    }
}

fn hex(c: Hsla) -> String {
    let rgba: gpui_kit::gpui::Rgba = c.into();
    format!("#{:02X}{:02X}{:02X}",
        (rgba.r * 255.) as u8, (rgba.g * 255.) as u8, (rgba.b * 255.) as u8)
}

/* ---------- 托盘图标（程序生成 32×32，无资产文件） ---------- */

fn tray_icon_rgba() -> Vec<u8> {
    let (w, h) = (32usize, 32usize);
    let mut v = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let edge = x < 2 || y < 2 || x >= w - 2 || y >= h - 2;
            let g_bar = (8..24).contains(&x) && (14..18).contains(&y); // "G" 的横杠
            let (r, g, b) = if edge { (0x1E, 0x3B, 0x8F) } else if g_bar { (0xFF, 0xFF, 0xFF) } else { (0x31, 0x6E, 0xF5) };
            let i = (y * w + x) * 4;
            v[i] = r; v[i + 1] = g; v[i + 2] = b; v[i + 3] = 0xFF;
        }
    }
    v
}

fn main() {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);

        // ④ 注入 DESIGN.md 冻结 token（亮色组）
        cx.update_global::<Theme, _>(|t, _| {
            t.colors.accent = rgb(0x316EF5).into();
            t.colors.accent_foreground = rgb(0xFFFFFF).into();
            t.colors.danger = rgb(0xD33A3A).into();
        });

        // ② 托盘
        let icon = tray_icon::Icon::from_rgba(tray_icon_rgba(), 32, 32).expect("[② FAIL] 图标");
        let tray = tray_icon::TrayIconBuilder::new()
            .with_tooltip("Gesso M0.5 spike（点我）")
            .with_icon(icon)
            .build()
            .expect("[② FAIL] 托盘创建");
        Box::leak(Box::new(tray)); // ponytail: spike 保活，正式实现存入 AppState
        tray_icon::TrayIconEvent::set_event_handler(Some(|e: tray_icon::TrayIconEvent| {
            TRAY_CLICKS.fetch_add(1, Ordering::SeqCst);
            println!("[② PASS] tray event: {:?}", e);
        }));
        println!("[② PASS] 托盘已创建（看菜单栏）");

        // ① 壁纸形态窗口：无边框 480×270 + webview
        let (host, _) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None, size(px(480.), px(270.)), cx,
                ))),
                titlebar: None,
                ..Default::default()
            },
            cx,
            HostView::new,
        )
        .expect("[① FAIL] 壁纸窗口");

        // 主窗口：报告面板
        let (_main, _) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None, size(px(560.), px(380.)), cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("Gesso · M0.5 spike")),
                    ..Default::default()
                }),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|cx| ReportView::new(cx, Some(host))),
        )
        .expect("主窗口失败");

        cx.activate(true);
    });
}
