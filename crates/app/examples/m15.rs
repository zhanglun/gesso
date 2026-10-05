//! M1.5 spike v2 · macOS 压层（纯 AppKit 壁纸窗口）
//!
//! 架构修正（v1 教训）：壁纸窗口不再是 GPUI 管理的窗口——GPUI 的窗口协调器
//! 与桌面级全屏窗口的几何必然冲突（v1 用轮询对打压不住）。改为：
//!   壁纸窗口 = 纯 AppKit NSWindow（borderless + level + 穿透，一次设置）
//!   + wry webview 直接挂它的 contentView（不经 gpui-wry）
//! GPUI 只负责托盘与动作轮询；两者同跑一个 NSApplication。
//!
//! 运行：cargo run -p gesso-app --example m15
//! 验收：壁纸铺满整屏（含菜单栏带）、图标可点、Space 跟随、全屏无异常。

// m15 是 macOS 专用 spike（纯 AppKit 压层）；非 macOS 平台编译占位 main。
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
#[cfg(target_os = "macos")]
use objc2_foundation::MainThreadMarker;
#[cfg(target_os = "macos")]
use raw_window_handle::{AppKitWindowHandle, HasWindowHandle, RawWindowHandle, WindowHandle};

/// 图标层下缘之下一档（spike v1 定稿）
#[cfg(target_os = "macos")]
const PIN_LEVEL: isize = -2147483604;

#[cfg(target_os = "macos")]
static SWAPPED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static MENU_ACTIONS: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

#[cfg(target_os = "macos")]
const HOST_HTML_A: &str = r#"<!DOCTYPE html><html><head><style>
body{margin:0;background:linear-gradient(160deg,#101830,#0B0F1E 60%,#141A2E);color:#EBEBED;
font:14px -apple-system,'PingFang SC',sans-serif;display:grid;place-items:center;height:100vh}
.t{font-size:26px;font-weight:600}.s{opacity:.6;margin-top:8px}
#ac{font-variant-numeric:tabular-nums;color:#5B8DEF;font-size:46px;margin-top:16px;font-weight:600}
</style></head><body><div style="text-align:center">
<div class="t">Gesso 壁纸（纯 AppKit 窗口）</div>
<div class="s">菜单栏带应被本壁纸覆盖 · 图标在你上面且可点</div>
<div id="ac">--:--:--</div></div>
<script>setInterval(()=>{document.getElementById('ac').textContent=new Date().toLocaleTimeString('zh-CN')},500)</script>
</body></html>"#;

#[cfg(target_os = "macos")]
const HOST_HTML_B: &str = r#"<!DOCTYPE html><html><head><style>body{margin:0;background:#D33A3A;color:#fff;
font:600 26px -apple-system,'PingFang SC';display:grid;place-items:center;height:100vh}
</style></head><body>壁纸 B（红 · 换壁纸验证）</body></html>"#;

#[cfg(target_os = "macos")]
fn host_urls() -> (&'static str, &'static str) {
    let a = std::env::temp_dir().join("gesso-m15v2-a.html");
    let b = std::env::temp_dir().join("gesso-m15v2-b.html");
    std::fs::write(&a, HOST_HTML_A).unwrap();
    std::fs::write(&b, HOST_HTML_B).unwrap();
    let u = |p: &std::path::Path| Box::leak(format!("file://{}", p.display()).into_boxed_str());
    (u(&a), u(&b))
}

/// 让 wry 把 webview 挂到我们自己的 NSView 上（不经 GPUI 窗口）
#[cfg(target_os = "macos")]
struct DesktopViewHandle(*mut NSView);
#[cfg(target_os = "macos")]
unsafe impl Send for DesktopViewHandle {}
#[cfg(target_os = "macos")]
impl HasWindowHandle for DesktopViewHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let ns_view =
            std::ptr::NonNull::new(self.0 as *mut core::ffi::c_void).expect("NSView 指针非空");
        // SAFETY: 借用期内 contentView 由 NSWindow 持有
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::AppKit(AppKitWindowHandle::new(ns_view)))
        })
    }
}

/// 创建纯 AppKit 壁纸窗口 + wry webview，返回 (window, webview)（主线程）
#[cfg(target_os = "macos")]
fn create_wallpaper_window(mtm: MainThreadMarker) -> (Retained<NSWindow>, lb_wry::WebView) {
    let screen = objc2_app_kit::NSScreen::mainScreen(mtm).expect("主屏");
    let frame = screen.frame();

    // SAFETY: 主线程 + MainThreadMarker 证明；NSWindow::new 因释放语义标记 unsafe
    let window = unsafe { NSWindow::new(mtm) };
    window.setStyleMask(NSWindowStyleMask::Borderless);
    window.setLevel(PIN_LEVEL);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    window.setIgnoresMouseEvents(true); // 点击穿透到图标
                                        // 透明底：webview 隐藏时透出系统壁纸（= 产品「回退静态壁纸」语义），
                                        // 而不是露出窗口默认白底
    window.setOpaque(false);
    // SAFETY: clearColor 类方法，主线程调用
    let clear = unsafe { objc2_app_kit::NSColor::clearColor() };
    window.setBackgroundColor(Some(&clear));
    window.setFrame_display(frame, true); // 一次到位，无人来抢
    window.orderFrontRegardless();

    let content = window.contentView().expect("contentView");
    let handle = DesktopViewHandle(Retained::as_ptr(&content) as *mut NSView);

    let (url_a, _url_b) = host_urls();
    let webview = lb_wry::WebViewBuilder::new()
        .with_url(url_a)
        .build_as_child(&handle)
        .expect("[M1.5 FAIL] wry 挂载");
    // 无 GPUI 布局系统 → 自己设满屏（顶左原点逻辑坐标）
    webview
        .set_bounds(lb_wry::Rect {
            size: lb_wry::dpi::Size::Logical(lb_wry::dpi::LogicalSize::new(
                frame.size.width,
                frame.size.height,
            )),
            position: lb_wry::dpi::Position::Logical(lb_wry::dpi::LogicalPosition::new(0., 0.)),
        })
        .expect("[M1.5 FAIL] webview bounds");
    println!(
        "[M1.5] 壁纸窗口就绪：level={} frame=({:.0},{:.0},{:.0}×{:.0}) 穿透=开",
        window.level(),
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height
    );
    (window, webview)
}

#[cfg(target_os = "macos")]
fn tray_icon_rgba() -> Vec<u8> {
    let (w, h) = (32usize, 32usize);
    let mut v = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let edge = x < 2 || y < 2 || x >= w - 2 || y >= h - 2;
            let bar = (8..24).contains(&x) && (14..18).contains(&y);
            let (r, g, b) = if edge {
                (0x1E, 0x3B, 0x8F)
            } else if bar {
                (0xFF, 0xFF, 0xFF)
            } else {
                (0x31, 0x6E, 0xF5)
            };
            let i = (y * w + x) * 4;
            v[i] = r;
            v[i + 1] = g;
            v[i + 2] = b;
            v[i + 3] = 0xFF;
        }
    }
    v
}

#[cfg(target_os = "macos")]
fn main() {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);

        // 壁纸窗口（纯 AppKit，非 GPUI 窗口）
        let mtm = MainThreadMarker::new().expect("必须在主线程");
        let (_window, webview) = create_wallpaper_window(mtm);

        // 托盘
        use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
        let menu = Menu::new();
        let mi_sw = MenuItem::with_id("swap", "换壁纸 A/B", true, None);
        let mi_hd = MenuItem::with_id("hide", "隐藏 / 显示壁纸", true, None);
        let mi_q = MenuItem::with_id("quit", "退出", true, None);
        menu.append_items(&[&mi_sw, &mi_hd, &PredefinedMenuItem::separator(), &mi_q])
            .expect("菜单");
        MenuEvent::set_event_handler(Some(|e: MenuEvent| {
            MENU_ACTIONS
                .lock()
                .unwrap()
                .push(Box::leak(e.id().as_ref().to_string().into_boxed_str()));
        }));
        let icon = tray_icon::Icon::from_rgba(tray_icon_rgba(), 32, 32).unwrap();
        let tray = tray_icon::TrayIconBuilder::new()
            .with_tooltip("Gesso M1.5 v2（纯 AppKit 壁纸窗口）")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .expect("托盘");
        Box::leak(Box::new(tray));

        // 动作轮询（wry WebView 仅限主线程操作；GPUI 前台执行器保证主线程）
        let wv: &'static lb_wry::WebView = Box::leak(Box::new(webview));
        let hidden: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
        cx.spawn(async move |cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;
            let actions: Vec<&'static str> = {
                let mut q = MENU_ACTIONS.lock().unwrap();
                std::mem::take(&mut *q)
            };
            let _ = cx.update(|_cx| {
                for a in actions {
                    match a {
                        "swap" => {
                            let to_b = !SWAPPED.fetch_xor(true, Ordering::SeqCst);
                            let (a, b) = host_urls();
                            let u = if to_b { b } else { a };
                            let _ = wv.load_url(u);
                            println!(
                                "[M1.5] 换壁纸 → {}",
                                if to_b { "B（红）" } else { "A（时钟）" }
                            );
                        }
                        "hide" => {
                            let now_hidden = !hidden.fetch_xor(true, Ordering::SeqCst);
                            let _ = wv.set_visible(!now_hidden);
                            println!("[M1.5] 壁纸 {}", if now_hidden { "隐藏" } else { "显示" });
                        }
                        "quit" => {
                            println!("[M1.5] 退出");
                            std::process::exit(0);
                        }
                        _ => {}
                    }
                }
            });
        })
        .detach();
    });
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("[m15] 纯 AppKit 压层 spike 仅 macOS；Windows 贴壁随 M1 接入。");
}
