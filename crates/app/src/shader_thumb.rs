//! Shader 缩略图采集（macOS · 主线程）：隐藏窗口 + wry 宿主页渲染 → WKWebView 快照 → PNG。
//!
//! 复用宿主页渲染器（同 GLSL / 同 uniforms / 同预置 iChannel 纹理），产出与视频条目
//! 同构的 `thumb.png + thumb-1..15.png` —— 卡片静态预览与 hover 轮播零改动复用。
//!
//! 线程与生命周期纪律（踩坑沉淀）：
//! 1. AppKit / wry / WKWebView 快照全部要求主线程——任务跑在 GPUI 前台执行器，
//!    等待一律用定时器挂起让出 runloop，不阻塞 UI。
//! 2. **采集窗口进程级持久、永不 close**：快照 completion 悬挂期间关闭窗口 =
//!    WebKit 异步回调摸已释放层 → autorelease 池排出时 over-release SIGSEGV
//!    （2026-10-04 崩溃实证，同 thumb.rs v1 指纹）。
//! 3. **全局串行**：同一时刻只有一个采集任务在跑（并发多 webview 是崩溃放大器）。

use std::sync::Arc;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSImage, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{MainThreadMarker, NSError, NSRect};
use objc2_web_kit::WKWebView;
use raw_window_handle::{AppKitWindowHandle, HasWindowHandle, RawWindowHandle, WindowHandle};

use crate::thumb::{self, HOVER_FRAMES, SAMPLE_FPS};

/// 采集分辨率：640×360 覆盖卡片预览与 hover 轮播（retina 快照 2x = 1280×720）
const CAP_W: f64 = 640.0;
const CAP_H: f64 = 360.0;
/// 页面就绪上限（含 shader 编译；坏 GLSL 时 `__gessoReady` 永不置位 → 放弃）
const READY_TIMEOUT: Duration = Duration::from_secs(5);
/// seek 后等待 RAF 重绘 + 渲染稳定（RAF 60fps 下一帧 ≤16ms，留足余量）
const FRAME_SETTLE: Duration = Duration::from_millis(140);
/// 单帧快照上限
const SHOT_TIMEOUT: Duration = Duration::from_secs(2);

/// 进程级采集队列（串行消费）；`false` = 工作任务未在跑
pub(crate) static CAPTURE_QUEUE: Mutex<Vec<String>> = Mutex::new(Vec::new());
pub(crate) static WORKER_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct CaptureWindow {
    /// 字段序 = drop 序：窗口最后释放（本设计中进程级存活，正常路径永不 drop）
    wk: Retained<WKWebView>,
    webview: lb_wry::WebView,
    #[allow(dead_code)] // 只为 drop 顺序占位（字段序 = 释放序），运行期不读
    window: Retained<NSWindow>,
}

thread_local! {
    /// 采集窗口：主线程独占；创建后整进程复用（见模块纪律第 2 条）
    static CAP_WINDOW: std::cell::RefCell<Option<CaptureWindow>> = const { std::cell::RefCell::new(None) };
}

/// wry 直挂所需的句柄包装（同 pin::macos::ViewHandle，仅主线程使用）
struct ViewHandle(*mut NSView);
// SAFETY: 指针仅在该句柄传给 wry（主线程）期间解引用
impl HasWindowHandle for ViewHandle {
    fn window_handle(
        &self,
    ) -> std::result::Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        let ns_view =
            std::ptr::NonNull::new(self.0 as *mut core::ffi::c_void).expect("NSView 非空");
        // SAFETY: contentView 由 NSWindow 持有，借用期内有效
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::AppKit(AppKitWindowHandle::new(ns_view)))
        })
    }
}

/// 取出采集窗口（惰性创建）；调用方用完必须 `put_back`（含失败路径）
fn take_window(mtm: MainThreadMarker) -> CaptureWindow {
    CAP_WINDOW.with(|c| {
        if let Some(w) = c.borrow_mut().take() {
            return w;
        }
        // 屏内窗口 + 2% 透明 + 壁纸层之上一档：屏幕外或被壁纸完全盖住（遮挡）都会
        // 让 WebKit 停摆 RAF/合成 → 快照全黑（两次实测）。几何上不被任何窗口盖住
        // WebKit 才全速渲染；2% 透明肉眼不可见，采集窗口整进程常驻。
        let window = unsafe { NSWindow::new(mtm) }; // SAFETY: 主线程标记保证
        window.setStyleMask(NSWindowStyleMask::Borderless);
        window.setOpaque(false);
        window.setAlphaValue(0.02);
        window.setLevel(crate::pin::macos::PIN_LEVEL + 1);
        window.setFrame_display(
            NSRect::new(
                objc2_foundation::NSPoint::new(0.0, 0.0),
                objc2_foundation::NSSize::new(CAP_W, CAP_H),
            ),
            true,
        );
        window.orderFrontRegardless();

        let content = window
            .contentView()
            .expect("contentView 缺失");
        let handle = ViewHandle(Retained::as_ptr(&content) as *mut NSView);
        // 首帧中立页（同 M2 教训：首帧 URL 必须必然成功），导航由调用方显式发起
        let webview = crate::protocol::create_webview(handle, "about:blank")
            .expect("wry 创建失败");
        webview
            .set_bounds(lb_wry::Rect {
                size: lb_wry::dpi::Size::Logical(lb_wry::dpi::LogicalSize::new(CAP_W, CAP_H)),
                position: lb_wry::dpi::Position::Logical(lb_wry::dpi::LogicalPosition::new(
                    0., 0.,
                )),
            })
            .expect("bounds 失败");

        // WKWebView = contentView 的首个子视图（wry 挂载结构，pin 层同款）
        let wk = {
            let content = window.contentView().expect("contentView 缺失");
            let subs = content.subviews();
            let v = subs
                .iter()
                .next()
                .expect("WKWebView 未挂载")
                .clone();
            Retained::downcast(v).expect("WKWebView 已挂载 contentView[0]")
        };

        CaptureWindow {
            wk,
            webview,
            window,
        }
    })
}

fn put_window(w: CaptureWindow) {
    CAP_WINDOW.with(|c| *c.borrow_mut() = Some(w));
}

/// 快照一帧（completion 经共享 cell 回传，主线程轮询收割）
async fn snapshot(
    bg: &gpui_kit::gpui::BackgroundExecutor,
    wk: &WKWebView,
) -> Option<Retained<NSImage>> {
    let cell: Arc<Mutex<Option<Retained<NSImage>>>> = Arc::new(Mutex::new(None));
    {
        let cell = cell.clone();
        let block = RcBlock::<dyn Fn(*mut NSImage, *mut NSError)>::new(
            move |img: *mut NSImage, _err: *mut NSError| {
                // SAFETY: 主线程；completion 传入 +0（autorelease）对象，retain 自持一份
                *cell.lock().unwrap() = unsafe { Retained::retain(img) };
            },
        );
        // SAFETY: 主线程调用；block 由框架 copy 持有
        unsafe { wk.takeSnapshotWithConfiguration_completionHandler(None, &block) };
    }
    let deadline = Instant::now() + SHOT_TIMEOUT;
    loop {
        if let Some(img) = cell.lock().unwrap().take() {
            return Some(img);
        }
        if Instant::now() > deadline {
            return None;
        }
        bg.timer(Duration::from_millis(15)).await;
    }
}

/// 采集一个 shader 条目的帧序列（串行队列消费；调用方已过 ThumbScheduler 去重）。
/// 返回成功写盘的帧数；完成经调用方 `ThumbsDone` 回灌 UI。
/// 采集窗口不归还时本函数负责 put_back（所有路径收敛到唯一出口）。
pub(crate) async fn capture_entry(
    bg: gpui_kit::gpui::BackgroundExecutor,
    url: String,
    dir: String,
) -> usize {
    let Some(mtm) = MainThreadMarker::new() else {
        println!("[thumbs] shader 采集必须在主线程");
        return 0;
    };
    let mut cap = take_window(mtm);
    let written = capture_with(&mut cap, &bg, url, &dir).await;
    put_window(cap);
    written
}

async fn capture_with(
    cap: &mut CaptureWindow,
    bg: &gpui_kit::gpui::BackgroundExecutor,
    url: String,
    dir: &str,
) -> usize {
    let _ = cap.webview.load_url(&url);

    // —— 就绪轮询：__gessoReady 由宿主页 shader 渲染器初始化成功后置位 ——
    let ready: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
    let deadline = Instant::now() + READY_TIMEOUT;
    let mut ok = false;
    while Instant::now() < deadline {
        {
            let ready = ready.clone();
            let _ = cap.webview.evaluate_script_with_callback(
                "window.__gessoReady === true",
                move |v: String| *ready.lock().unwrap() = Some(v == "true"),
            );
        }
        bg.timer(Duration::from_millis(120)).await;
        if ready.lock().unwrap().take() == Some(true) {
            ok = true;
            break;
        }
    }
    if !ok {
        println!("[thumbs] shader 页面未就绪（编译失败或加载超时）：{dir}");
        return 0;
    }
    // 首帧渲染后再等一拍，快照避免撞上合成空窗
    bg.timer(Duration::from_millis(150)).await;

    // RAF 活性探针：0 增量 = 窗口被遮挡/停摆（快照必然全黑的先兆）
    let _ = cap
        .webview
        .evaluate_script("window.__rafMark = window.__rafN;");
    bg.timer(Duration::from_millis(400)).await;
    {
        let r: Arc<Mutex<Option<i64>>> = Arc::new(Mutex::new(None));
        {
            let r = r.clone();
            let _ = cap.webview.evaluate_script_with_callback(
                "window.__rafN - (window.__rafMark || 0)",
                move |v: String| *r.lock().unwrap() = v.trim().parse().ok(),
            );
        }
        bg.timer(Duration::from_millis(100)).await;
        let delta = r.lock().unwrap().unwrap_or(-1);
        println!("[thumbs] RAF 活性（400ms 增量）：{delta}");
    }

    // —— 帧序列：t = i / SAMPLE_FPS（与视频抽帧同时刻表，轮播节奏一致）——
    let mut written = 0usize;
    for i in 0..=HOVER_FRAMES {
        let t = i as f64 / SAMPLE_FPS as f64;
        let _ = cap
            .webview
            .evaluate_script(&format!("window.__gessoSeek && window.__gessoSeek({t})"));
        bg.timer(FRAME_SETTLE).await;

        let mut image = snapshot(bg, &cap.wk).await;
        if image.is_none() {
            // 末帧偶发超时：等一拍重试一次
            bg.timer(Duration::from_millis(300)).await;
            image = snapshot(bg, &cap.wk).await;
        }
        let Some(image) = image else {
            continue;
        };
        // NSImage → CGImage → PNG（ImageIO）
        let Some(cg) = (unsafe {
            image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)
        }) else {
            continue;
        };
        let out: PathBuf = thumb::thumb_path(Path::new(dir), i);
        if thumb::write_png(&cg, &out) {
            written += 1;
        }
    }
    written
}
