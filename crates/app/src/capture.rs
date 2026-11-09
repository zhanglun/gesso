//! 缩略图采集（主线程）：隐藏窗口 + wry 宿主页渲染。
//! macOS：WKWebView 快照 → PNG（本文件）；Windows：webview 抽帧/PrintWindow（capture_win.rs）。
//!
//! 复用宿主页渲染器（shader 走 GLSL，html 走沙箱 iframe），产出与视频条目
//! 同构的 `thumb.png + thumb-1..15.png` —— 卡片静态预览与 hover 轮播零改动复用。
//! shader 经 `__gessoSeek(t)` 定格目标时刻；html 是活页面，按固定间隔截真时间帧。
//!
//! 线程与生命周期纪律（踩坑沉淀）：
//! 1. AppKit / wry / WKWebView 快照全部要求主线程——任务跑在 GPUI 前台执行器，
//!    等待一律用定时器挂起让出 runloop，不阻塞 UI。
//! 2. **采集窗口进程级持久、永不 close**：快照 completion 悬挂期间关闭窗口 =
//!    WebKit 异步回调摸已释放层 → autorelease 池排出时 over-release SIGSEGV
//!    （已由历史崩溃实证确认）。
//! 3. **全局串行**：同一时刻只有一个采集任务在跑（并发多 webview 是崩溃放大器）。

#[cfg(target_os = "macos")]
use std::rc::Rc;
#[cfg(target_os = "macos")]
use std::sync::Arc;
use std::sync::Mutex;

use gesso_core::WallpaperKind;

// 快照实现依赖 AppKit/WKWebView（Apple 专属）：以下导入与实现项全部 macOS 门控，
// Windows 采集使用 capture_win.rs 的 WebView2 抽帧实现。
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::Instant;

#[cfg(target_os = "macos")]
use block2::RcBlock;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSImage, NSView, NSWindow, NSWindowStyleMask};
#[cfg(target_os = "macos")]
use objc2_foundation::{MainThreadMarker, NSError, NSRect};
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;
#[cfg(target_os = "macos")]
use raw_window_handle::{AppKitWindowHandle, HasWindowHandle, RawWindowHandle, WindowHandle};

#[cfg(target_os = "macos")]
use crate::thumb::{self, HOVER_FRAMES, SAMPLE_FPS};

/// 采集分辨率：640×360 覆盖卡片预览与 hover 轮播（retina 快照 2x = 1280×720）
pub(crate) const CAP_W: f64 = 640.0;
pub(crate) const CAP_H: f64 = 360.0;
/// 页面就绪上限（含 shader 编译；坏 GLSL 时 `__gessoReady` 永不置位 → 放弃）
pub(crate) const READY_TIMEOUT: Duration = Duration::from_secs(5);
/// seek 后等待 RAF 重绘 + 渲染稳定（RAF 60fps 下一帧 ≤16ms，留足余量）
pub(crate) const FRAME_SETTLE: Duration = Duration::from_millis(140);
/// 单帧快照上限
pub(crate) const SHOT_TIMEOUT: Duration = Duration::from_secs(2);

/// 进程级采集队列（串行消费）；`false` = 工作任务未在跑
pub(crate) static CAPTURE_QUEUE: Mutex<Vec<(String, WallpaperKind)>> = Mutex::new(Vec::new());
pub(crate) static WORKER_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 封面帧采集任务（§4.8/§4.8.1）：入库时入队，主线程串行消化（同缩略图纪律）。
pub(crate) struct PosterJob {
    /// 条目缩略图家目录（库查找键，同 CAPTURE_QUEUE 的键语义）
    pub dir: String,
    /// 条目源目录（macOS 视频封面帧取 `index.mp4` 用；Windows 不读）
    pub source_dir: String,
    pub kind: WallpaperKind,
    /// 物理像素目标尺寸（主屏分辨率，上限 4K）
    pub size: (i32, i32),
}

pub(crate) static POSTER_QUEUE: Mutex<Vec<PosterJob>> = Mutex::new(Vec::new());
pub(crate) static POSTER_WORKER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "macos")]
struct CaptureWindow {
    /// 字段序 = drop 序：窗口最后释放（本设计中进程级存活，正常路径永不 drop）
    wk: Retained<WKWebView>,
    webview: lb_wry::WebView,
    #[allow(dead_code)] // 只为 drop 顺序占位（字段序 = 释放序），运行期不读
    window: Retained<NSWindow>,
}

#[cfg(target_os = "macos")]
thread_local! {
    /// 采集窗口：主线程独占；创建后整进程复用（见模块纪律第 2 条）
    static CAP_WINDOW: std::cell::RefCell<Option<CaptureWindow>> = const { std::cell::RefCell::new(None) };
}

/// wry 直挂所需的句柄包装（同 pin::macos::ViewHandle，仅主线程使用）
#[cfg(target_os = "macos")]
struct ViewHandle(*mut NSView);
// SAFETY: 指针仅在该句柄传给 wry（主线程）期间解引用
#[cfg(target_os = "macos")]
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
#[cfg(target_os = "macos")]
fn take_window(mtm: MainThreadMarker) -> CaptureWindow {
    CAP_WINDOW.with(|c| {
        if let Some(w) = c.borrow_mut().take() {
            return w;
        }
        build_capture_window(mtm, CAP_W, CAP_H)
    })
}

/// 构建采集窗：屏内 + 2% 透明 + 壁纸层之上一档——屏幕外或被壁纸完全盖住
/// （遮挡）都会让 WebKit 停摆 RAF/合成 → 快照全黑（两次实测）。几何上不被
/// 任何窗口盖住 WebKit 才全速渲染；2% 透明肉眼不可见。缩略图窗整进程常驻，
/// 封面帧窗一次性建毁（同 Windows capture_poster 语义）。
#[cfg(target_os = "macos")]
fn build_capture_window(mtm: MainThreadMarker, w: f64, h: f64) -> CaptureWindow {
    let window = unsafe { NSWindow::new(mtm) }; // SAFETY: 主线程标记保证
    window.setStyleMask(NSWindowStyleMask::Borderless);
    window.setOpaque(false);
    window.setAlphaValue(0.02);
    window.setLevel(crate::pin::macos::PIN_LEVEL + 1);
    window.setFrame_display(
        NSRect::new(objc2_foundation::NSPoint::new(0.0, 0.0), objc2_foundation::NSSize::new(w, h)),
        true,
    );
    window.orderFrontRegardless();

    let content = window.contentView().expect("contentView 缺失");
    let handle = ViewHandle(Retained::as_ptr(&content) as *mut NSView);
    // 首帧加载 about:blank；导航由调用方显式发起
    let webview = crate::protocol::create_webview(handle, "about:blank").expect("wry 创建失败");
    webview
        .set_bounds(lb_wry::Rect {
            size: lb_wry::dpi::Size::Logical(lb_wry::dpi::LogicalSize::new(w, h)),
            position: lb_wry::dpi::Position::Logical(lb_wry::dpi::LogicalPosition::new(0., 0.)),
        })
        .expect("bounds 失败");

    // WKWebView = contentView 的首个子视图（wry 挂载结构，pin 层同款）
    let wk = {
        let content = window.contentView().expect("contentView 缺失");
        let subs = content.subviews();
        let v = subs.iter().next().expect("WKWebView 未挂载").clone();
        Retained::downcast(v).expect("WKWebView 已挂载 contentView[0]")
    };

    CaptureWindow {
        wk,
        webview,
        window,
    }
}

#[cfg(target_os = "macos")]
fn put_window(w: CaptureWindow) {
    CAP_WINDOW.with(|c| *c.borrow_mut() = Some(w));
}

/// 快照一帧（completion 经共享 cell 回传，主线程轮询收割）
#[cfg(target_os = "macos")]
async fn snapshot(
    bg: &gpui_kit::gpui::BackgroundExecutor,
    wk: &WKWebView,
) -> Option<Retained<NSImage>> {
    let cell: Rc<Mutex<Option<Retained<NSImage>>>> = Rc::new(Mutex::new(None));
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
#[cfg(target_os = "macos")]
pub(crate) async fn capture_entry(
    bg: gpui_kit::gpui::BackgroundExecutor,
    url: String,
    dir: String,
    kind: WallpaperKind,
) -> usize {
    let Some(mtm) = MainThreadMarker::new() else {
        println!("[thumbs] shader 采集必须在主线程");
        return 0;
    };
    let mut cap = take_window(mtm);
    let written = capture_with(&mut cap, &bg, url, &dir, kind).await;
    // 回中立页：宿主页 shader 的 RAF/WebGL 循环不会自停，不导航走就等于隐形窗
    // 常驻 60fps 白渲染（GPU process + WindowServer 持续买单，壁纸播放被拖累）
    let _ = cap.webview.load_url("about:blank");
    put_window(cap);
    written
}

/// Windows 走 webview 抽帧管线（capture_win.rs）。
#[cfg(target_os = "windows")]
pub(crate) async fn capture_entry(
    bg: gpui_kit::gpui::BackgroundExecutor,
    url: String,
    dir: String,
    kind: WallpaperKind,
) -> usize {
    crate::capture_win::capture_entry(bg, url, dir, kind).await
}

// ── 封面帧采集（§4.8/§4.8.1）─────────────────────────────────────────────

/// 封面帧单帧采集（跨平台入口，spawn_poster_job 调用）：jpg/png 静图不进此
/// 函数（poster_job_for 直接引用主文件）。video → 首帧；shader/html/动图静图
/// → 宿主页快照；尺寸 = 物理像素上限 4K，JPEG 输出。
#[cfg(target_os = "windows")]
pub(crate) async fn capture_poster(
    bg: gpui_kit::gpui::BackgroundExecutor,
    url: String,
    source_dir: String,
    kind: WallpaperKind,
    out: &Path,
    size: (i32, i32),
) -> bool {
    let _ = source_dir; // Windows 视频封面走宿主页画布，不用源文件
    crate::capture_win::capture_poster(&bg, url, kind, out, size).await
}

/// macOS 封面帧：video 走 AVAssetImageGenerator 抽首帧（后台线程，同
/// extract_frames 管线）；其余走一次性 WKWebView 采集窗（主线程，建毁语义
/// 同 Windows capture_poster）。快照 completion 收割完成后才拆窗。
#[cfg(target_os = "macos")]
pub(crate) async fn capture_poster(
    bg: gpui_kit::gpui::BackgroundExecutor,
    url: String,
    source_dir: String,
    kind: WallpaperKind,
    out: &Path,
    size: (i32, i32),
) -> bool {
    if kind == WallpaperKind::Video {
        let out = out.to_path_buf();
        return bg
            .spawn(async move { poster_from_video(&source_dir, &out, size) })
            .await;
    }
    poster_from_web(&bg, url, out, size).await
}

/// 视频封面帧：`index.mp4` 首帧 → JPEG（缩略图同源同刻，锁屏形象与卡片一致）。
#[cfg(target_os = "macos")]
fn poster_from_video(source_dir: &str, out: &Path, size: (i32, i32)) -> bool {
    use objc2_av_foundation::{AVAssetImageGenerator, AVURLAsset};
    use objc2_core_media::CMTime;
    use objc2_foundation::{NSString, NSURL};

    let _ = size; // 封面按视频原生分辨率出（常见 ≤4K）；上层 4K 上限由源约束
    let src = Path::new(source_dir).join("index.mp4");
    if !src.exists() {
        println!("[poster] 视频源缺失：{source_dir}");
        return false;
    }
    objc2::rc::autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(src.to_string_lossy().as_ref()));
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
        let gen = unsafe { AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset) };
        // 与缩略图同款参数（帧精度 + 免裁剪 + 方向修正）
        unsafe {
            gen.setRequestedTimeToleranceBefore(CMTime::new(0, 600));
            gen.setRequestedTimeToleranceAfter(CMTime::new(0, 600));
            gen.setAppliesPreferredTrackTransform(true);
        }
        let image = {
            // 同 extract_frames：后台有限次同步调用，deprecated 形态最简且正确
            #[allow(deprecated)]
            match unsafe {
                gen.copyCGImageAtTime_actualTime_error(CMTime::with_seconds(0.0, 600), std::ptr::null_mut())
            } {
                Ok(image) => image,
                Err(e) => {
                    println!("[poster] 视频抽帧失败：{e:?}");
                    return false;
                }
            }
        };
        thumb::write_jpeg(&image, out)
    })
}

/// Web 类封面帧：一次性采集窗 → 宿主页就绪 → 单帧快照 → JPEG。
#[cfg(target_os = "macos")]
async fn poster_from_web(
    bg: &gpui_kit::gpui::BackgroundExecutor,
    url: String,
    out: &Path,
    size: (i32, i32),
) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        println!("[poster] 封面采集必须在主线程");
        return false;
    };
    // 复用常驻采集窗（改尺寸 → 采集 → 还原尺寸 → 归还），**不做一次性建毁**：
    // 快照 completion 超时后仍可能迟到，关闭窗口会让迟到的回调摸已释放层——
    // 正是本模块纪律第 2 条记录的 SIGSEGV 模式；常驻窗永不关闭，天然免疫。
    let cap = take_window(mtm);
    // 物理/2 逻辑尺寸：retina 快照 2x = 目标物理分辨率
    let w = (size.0.max(1) as f64) / 2.0;
    let h = (size.1.max(1) as f64) / 2.0;
    resize_capture_window(&cap, w, h);
    let ok = poster_capture_with(&cap, bg, url, out).await;
    // 回中立页（同 capture_entry：宿主页 RAF/WebGL 循环不自停，常驻窗不能留活页）
    let _ = cap.webview.load_url("about:blank");
    resize_capture_window(&cap, CAP_W, CAP_H); // 缩略图任务按常量尺寸工作
    put_window(cap);
    ok
}

/// 调整常驻采集窗与 webview 的逻辑尺寸（封面帧与缩略图共用一窗的换挡）。
#[cfg(target_os = "macos")]
fn resize_capture_window(cap: &CaptureWindow, w: f64, h: f64) {
    cap.window.setFrame_display(
        NSRect::new(objc2_foundation::NSPoint::new(0.0, 0.0), objc2_foundation::NSSize::new(w, h)),
        true,
    );
    let _ = cap.webview.set_bounds(lb_wry::Rect {
        size: lb_wry::dpi::Size::Logical(lb_wry::dpi::LogicalSize::new(w, h)),
        position: lb_wry::dpi::Position::Logical(lb_wry::dpi::LogicalPosition::new(0., 0.)),
    });
}

#[cfg(target_os = "macos")]
async fn poster_capture_with(
    cap: &CaptureWindow,
    bg: &gpui_kit::gpui::BackgroundExecutor,
    url: String,
    out: &Path,
) -> bool {
    let _ = cap.webview.load_url(&url);

    // 就绪轮询（同 capture_with 契约：shader 编译成功 / html iframe 装载即置位）
    let deadline = Instant::now() + READY_TIMEOUT;
    let mut ok = false;
    while Instant::now() < deadline {
        let ready: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
        {
            let ready = ready.clone();
            let _ = cap
                .webview
                .evaluate_script_with_callback("window.__gessoReady === true", move |v: String| {
                    *ready.lock().unwrap() = Some(v == "true")
                });
        }
        bg.timer(Duration::from_millis(120)).await;
        if ready.lock().unwrap().take() == Some(true) {
            ok = true;
            break;
        }
    }
    if !ok {
        println!("[poster] 页面未就绪（编译失败或加载超时）：{url}");
        return false;
    }
    bg.timer(Duration::from_millis(150)).await;

    let mut image = snapshot(bg, &cap.wk).await;
    if image.is_none() {
        // 末帧偶发超时：等一拍重试一次
        bg.timer(Duration::from_millis(300)).await;
        image = snapshot(bg, &cap.wk).await;
    }
    let Some(image) = image else {
        return false;
    };
    let Some(cg) = (unsafe {
        image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)
    }) else {
        return false;
    };
    thumb::write_jpeg(&cg, out)
}

#[cfg(target_os = "macos")]
async fn capture_with(
    cap: &mut CaptureWindow,
    bg: &gpui_kit::gpui::BackgroundExecutor,
    url: String,
    dir: &str,
    kind: WallpaperKind,
) -> usize {
    let _ = cap.webview.load_url(&url);

    // —— 就绪轮询：shader 初始化成功 / html iframe 开始装载即置 __gessoReady ——
    let ready: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
    let deadline = Instant::now() + READY_TIMEOUT;
    let mut ok = false;
    while Instant::now() < deadline {
        {
            let ready = ready.clone();
            let _ = cap
                .webview
                .evaluate_script_with_callback("window.__gessoReady === true", move |v: String| {
                    *ready.lock().unwrap() = Some(v == "true")
                });
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

    // —— 帧序列：shader t = i/SAMPLE_FPS 定格；html 活页面按间隔截真时间帧 ——
    let mut written = 0usize;
    for i in 0..=HOVER_FRAMES {
        if kind == WallpaperKind::Shader {
            let t = i as f64 / SAMPLE_FPS as f64;
            let _ = cap
                .webview
                .evaluate_script(&format!("window.__gessoSeek && window.__gessoSeek({t})"));
        }
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
