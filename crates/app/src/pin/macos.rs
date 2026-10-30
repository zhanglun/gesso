//! macOS 贴壁实现（可行性 spike 产品化）。
//!
//! 关键参数定稿（见 docs/archive/SPIKE-REPORT.md）：
//! - level = -2147483604（图标层下缘之下一档；勿用桌面图同级，排序不可控）
//! - Borderless + CanJoinAllSpaces|Stationary|FullScreenAuxiliary|IgnoresCycle
//! - ignoresMouseEvents + 不透明黑底（回退系统桌面 = 隐藏整窗，非藏 webview）

use std::sync::atomic::{AtomicU64, Ordering};

use objc2::rc::Retained;
use objc2_app_kit::{
    NSColor, NSImage, NSImageView, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::MainThreadMarker;
use raw_window_handle::{AppKitWindowHandle, HasWindowHandle, RawWindowHandle, WindowHandle};

use super::{MonitorFrame, MonitorInfo, WallpaperWindow};
use gesso_core::{GessoError, Result};

pub(crate) const PIN_LEVEL: isize = -2147483604;

// —— 拓扑纪元（topology epochs）——
// 显示器重配（连接/断开/移动/主屏切换/模式变更）必须事件驱动：轮询采样有
// 2s 级延迟（WindowServer 收容窗口的错误中间态肉眼可见），且可能采到重配
// 序列的半程状态。回调只做代数自增——投递期 WindowServer 正持锁，不得触碰
// AppKit/分配；去抖与消费在引擎心跳侧（main.rs）。
static TOPOLOGY_EPOCH: AtomicU64 = AtomicU64::new(0);

// SAFETY: CoreGraphics 公开 C ABI——CGDirectDisplayID = u32，
// CGDisplayChangeSummaryFlags = u32 位掩码，CGError = i32
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGDisplayRegisterReconfigurationCallback(
        callback: unsafe extern "C" fn(u32, u32, *mut core::ffi::c_void),
        userinfo: *mut core::ffi::c_void,
    ) -> i32;
}

unsafe extern "C" fn on_display_reconfig(_: u32, _: u32, _: *mut core::ffi::c_void) {
    TOPOLOGY_EPOCH.fetch_add(1, Ordering::Release);
}

/// 安装显示器重配监听（进程内一次；引擎心跳经 [`topology_epoch`] 消费）。
/// begin/end/逐屏 pass 全部计入——纪元只要「有变化」语义，去抖在心跳侧。
/// 注册失败只打日志：稳态巡检（≈5s 深收敛）兜底一切丢失的事件。
pub fn install_topology_watch() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: 进程级一次性注册；回调是静态 fn，生命周期覆盖全程
        let err = unsafe {
            CGDisplayRegisterReconfigurationCallback(on_display_reconfig, std::ptr::null_mut())
        };
        if err != 0 {
            println!("[pin] 显示器重配监听注册失败 err={err}（稳态巡检兜底）");
        }
    });
}

/// 拓扑纪元代数（只增不减；仅用于「有变化」判定，不区分事件种类）。
pub fn topology_epoch() -> u64 {
    TOPOLOGY_EPOCH.load(Ordering::Acquire)
}

/// wry 直挂所需的句柄包装（仅主线程使用）
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

/// 延迟诊断：窗口与 webview 的当前真值（由轮询调用）
pub fn diagnose(win: &MacWallpaperWindow, tag: &str) {
    let w = &win._window;
    let subs = w
        .contentView()
        .as_ref()
        .map(|c| c.subviews().len())
        .unwrap_or(0);
    println!(
        "[diag {tag}] level={} visible={} occluded={} frame=({:.0},{:.0},{:.0}×{:.0}) subviews={} url={}",
        w.level(),
        w.isVisible(),
        // NSWindowOcclusionStateVisible = 1<<1：不含该位 = 被遮挡
        w.occlusionState().0 & (1 << 1) == 0,
        w.frame().origin.x,
        w.frame().origin.y,
        w.frame().size.width,
        w.frame().size.height,
        subs,
        win.webview.url().map(|u| u.to_string()).unwrap_or_else(|e| format!("<{e}>")),
    );

    if let Some(c) = w.contentView().as_ref() {
        for (i, sub) in c.subviews().iter().enumerate().take(3) {
            let sv: &NSView = unsafe { &*(std::ptr::from_ref(&**sub) as *const NSView) };
            let f = sv.frame();
            println!(
                "[diag {tag}] sub[{i}] frame=({:.0},{:.0},{:.0}×{:.0}) hidden={} alpha={:.1}",
                f.origin.x,
                f.origin.y,
                f.size.width,
                f.size.height,
                sv.isHidden(),
                sv.alphaValue()
            );
        }
    }
}

pub struct MacWallpaperWindow {
    /// 持有窗口所有权（Retained 保活；WebView 由 wry 内部 Rc 管理，存副本供操作）
    _window: Retained<NSWindow>,
    webview: lb_wry::WebView,
}

/// 贴壁窗口本体（webview / 原生视频两种内容共用同一组几何/层级/行为配置）。
fn make_pin_window(mtm: MainThreadMarker) -> Retained<NSWindow> {
    let window = unsafe { NSWindow::new(mtm) };
    window.setStyleMask(NSWindowStyleMask::Borderless);
    window.setLevel(PIN_LEVEL);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    window.setIgnoresMouseEvents(true);
    // 不透明：壁纸窗整屏被内容盖满，透明毫无收益，却让 WindowServer
    // 每帧对下层系统壁纸做全屏 alpha 混合（5K60 视频稳态实测 CPU 翻倍）。
    window.setOpaque(true);
    window.setBackgroundColor(Some(&NSColor::blackColor()));
    window
}

pub fn create(monitor: &MonitorInfo) -> Result<MacWallpaperWindow> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| GessoError::UnsupportedPlatform("非主线程".into()))?;

    let window = make_pin_window(mtm);

    let (x, y, w, h) = monitor.frame;
    // 几何在创建时施加，但**保持隐藏**——显示由会话收敛器统一复显
    //（先落位后可见；旧路径 create 即 orderFront，重建时会以未加载的
    // 黑底/错误几何先可见，正是唤醒期伪影的来源之一）
    window.setFrame_display(
        objc2_foundation::NSRect::new(
            objc2_foundation::NSPoint::new(x, y),
            objc2_foundation::NSSize::new(w, h),
        ),
        true,
    );

    let content = window
        .contentView()
        .ok_or_else(|| GessoError::UnsupportedPlatform("contentView 缺失".into()))?;
    let handle = ViewHandle(Retained::as_ptr(&content) as *mut NSView);

    // 初始 URL：自定义协议已可用（lb-wry ≥0.53），首帧直接走 gesso 宿主页。
    // build_session 稍后会 load 带 spec 的真实 URL。
    let webview = crate::protocol::create_webview(handle, "gesso://host/index.html")
        .map_err(|e| GessoError::UnsupportedPlatform(format!("wry: {e}")))?;
    webview
        .set_bounds(lb_wry::Rect {
            size: lb_wry::dpi::Size::Logical(lb_wry::dpi::LogicalSize::new(w, h)),
            position: lb_wry::dpi::Position::Logical(lb_wry::dpi::LogicalPosition::new(0., 0.)),
        })
        .map_err(|e| GessoError::UnsupportedPlatform(format!("bounds: {e}")))?;

    // —— 挂载诊断（沿用 m15 DIAG 模式）：窗口级 + subview 级真值 ——
    {
        let content = window.contentView();
        let subs = content.as_ref().map(|c| c.subviews().len()).unwrap_or(0);
        println!(
            "[diag] level={} visible={} frame=({:.0},{:.0},{:.0}×{:.0}) alpha={:.1} subviews={}",
            window.level(),
            window.isVisible(),
            window.frame().origin.x,
            window.frame().origin.y,
            window.frame().size.width,
            window.frame().size.height,
            window.alphaValue(),
            subs,
        );
        if let Some(c) = content.as_ref() {
            for (i, sub) in c.subviews().iter().enumerate().take(3) {
                let sv: &NSView = unsafe { &*(std::ptr::from_ref(&**sub) as *const NSView) };
                let f = sv.frame();
                println!(
                    "[diag] sub[{i}] frame=({:.0},{:.0},{:.0}×{:.0}) hidden={}",
                    f.origin.x,
                    f.origin.y,
                    f.size.width,
                    f.size.height,
                    sv.isHidden()
                );
            }
        }
    }

    Ok(MacWallpaperWindow {
        _window: window,
        webview,
    })
}

impl WallpaperWindow for MacWallpaperWindow {
    fn diag(&self, tag: &str) {
        diagnose(self, tag);
    }

    fn load(&mut self, url: &str) {
        let _ = self.webview.load_url(url);
    }

    fn set_paused(&mut self, paused: bool) {
        // 复用类型化命令契约（§5.3：暂停 = 停 RAF / video.pause，窗口常驻）
        let cmd = if paused {
            crate::host_cmd::HostCommand::Pause
        } else {
            crate::host_cmd::HostCommand::Resume
        };
        let _ = self.webview.evaluate_script(&cmd.to_js());
    }

    fn evaluate(&mut self, js: &str) {
        let _ = self.webview.evaluate_script(js);
    }

    fn evaluate_with_callback(&mut self, js: &str, cb: Box<dyn Fn(String) + Send>) {
        let _ = self
            .webview
            .evaluate_script_with_callback(js, move |v: String| cb(v));
    }

    fn set_visible(&mut self, visible: bool) {
        // 窗口级隐藏：窗口已不透明，藏 webview 只会露黑底；藏窗才露出
        // 系统桌面（与 MacVideoWindow 一致的回退静态壁纸语义）
        if visible {
            self._window.orderFrontRegardless();
        } else {
            self._window.orderOut(None);
        }
    }

    fn current_url(&self) -> String {
        // lb-wry 的 url() 在页面未提交首帧前对 nil URL unwrap 会 panic
        //（WKWebView.URL() 返回 nil）——启动诊断/竞态路径必须容忍
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.webview
                .url()
                .map(|u| u.to_string())
                .unwrap_or_else(|e| format!("<{e}>"))
        }))
        .unwrap_or_else(|_| "<unavailable>".into())
    }

    fn set_frame(&mut self, (x, y, w, h): (f64, f64, f64, f64)) {
        self._window.setFrame_display(
            objc2_foundation::NSRect::new(
                objc2_foundation::NSPoint::new(x, y),
                objc2_foundation::NSSize::new(w, h),
            ),
            true,
        );
        let _ = self.webview.set_bounds(lb_wry::Rect {
            size: lb_wry::dpi::Size::Logical(lb_wry::dpi::LogicalSize::new(w, h)),
            position: lb_wry::dpi::Position::Logical(lb_wry::dpi::LogicalPosition::new(0., 0.)),
        });
    }

    fn actual_frame(&self) -> Option<MonitorFrame> {
        let f = self._window.frame();
        Some((f.origin.x, f.origin.y, f.size.width, f.size.height))
    }

    fn reassert_pinning(&mut self, frame: (f64, f64, f64, f64)) {
        // WindowServer 在显示器重配后可能重置层级/空间行为（实测：合盖再开盖，
        // 壁纸窗口从图标层之下浮成普通窗口）——与 create() 保持同一组贴壁参数
        self._window.setLevel(PIN_LEVEL);
        self._window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        self.set_frame(frame);
    }
}

/// 原生视频壁纸窗：AVPlayerLayer 直挂 contentView，不经 webview。
/// 原因（2026-10-10 实测）：webview 的远端层树对视频上屏节奏不均——60fps
/// 片 rVFC 实测 53~56/s 波动（页面节拍却稳 60Hz），肉眼卡顿；原生 AVPlayer
/// 管线有显示锁相，视频 URL 优先使用回环 HTTP，服务不可用时才回落本地文件。
pub struct MacVideoWindow {
    _window: Retained<NSWindow>,
    _looper: Retained<objc2_av_foundation::AVPlayerLooper>,
    player: Retained<objc2_av_foundation::AVQueuePlayer>,
    layer: Retained<objc2_av_foundation::AVPlayerLayer>,
}

/// 填充模式 → AVLayerVideoGravity（§4.4 填充下拉，2026-10-12）。
/// Fill = 无视比例铺满（变形）；Cover = 等比裁切铺满；Contain = 信箱完整显示。
pub fn video_gravity(fit: gesso_core::Fit) -> &'static str {
    match fit {
        gesso_core::Fit::Cover => "AVLayerVideoGravityResizeAspectFill",
        gesso_core::Fit::Contain => "AVLayerVideoGravityResizeAspect",
        gesso_core::Fit::Fill => "AVLayerVideoGravityResize",
    }
}

pub fn create_video_window(
    monitor: &MonitorInfo,
    url: &str,
    fit: gesso_core::Fit,
) -> gesso_core::Result<MacVideoWindow> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| GessoError::UnsupportedPlatform("非主线程".into()))?;
    let window = make_pin_window(mtm);
    let (x, y, w, h) = monitor.frame;
    window.setFrame_display(video_pin_frame((x, y, w, h)), true);

    let content = window.contentView().expect("contentView 缺失");
    // SAFETY: 主线程；AVFoundation 家族方法按文档语义调用
    let (player, looper, layer) = unsafe {
        use objc2_av_foundation::{AVPlayerItem, AVPlayerLayer, AVPlayerLooper, AVQueuePlayer};
        use objc2_foundation::{NSArray, NSString};
        let player = AVQueuePlayer::queuePlayerWithItems(&NSArray::new(), mtm);
        let layer = AVPlayerLayer::playerLayerWithPlayer(Some(&player));
        // 适配语义 = 填充下拉（§4.4，默认 Cover 裁切铺满；2026-10-12 前恒为
        // contain——比例吻合的素材各模式渲染相同，不吻合时按用户选择裁切/信箱/变形）
        layer.setVideoGravity(&objc2_foundation::NSString::from_str(video_gravity(fit)));
        content.setWantsLayer(true);
        content.setLayer(Some(&layer));
        // 媒体 URL：优先使用 http:// 回环内存直供，失败时回落本地文件。
        let nsurl = objc2_foundation::NSURL::URLWithString(&NSString::from_str(url))
            .ok_or_else(|| GessoError::UnsupportedPlatform(format!("媒体 URL 非法：{url}")))?;
        let item = AVPlayerItem::playerItemWithURL(&nsurl, mtm);
        let looper = AVPlayerLooper::playerLooperWithPlayer_templateItem(&player, &item);
        player.setMuted(true); // AudioPolicy::Muted
        player.play();
        (player, looper, layer)
    };
    // 与 webview 窗同一纪律：创建保持隐藏，几何落位由会话收敛器统一复显
    //（AVPlayer 在不可见窗口中照常解码，点亮即有画面）
    println!("[session] 原生视频窗 {url}");
    Ok(MacVideoWindow {
        _window: window,
        _looper: looper,
        player,
        layer,
    })
}

impl WallpaperWindow for MacVideoWindow {
    /// 内容在创建时定死（file 路径）；换壁纸 = 整窗重建（assign 同路径）。
    /// set_fps 热重载传进来的宿主页 URL 在此静默忽略（视频无帧率杠杆）。
    fn load(&mut self, _url: &str) {}

    fn set_paused(&mut self, paused: bool) {
        // SAFETY: 主线程（会话层在主循环调用）；play/pause 幂等
        unsafe {
            if paused {
                self.player.pause();
            } else {
                self.player.play();
            }
        }
    }

    fn set_fit(&mut self, fit: gesso_core::Fit) {
        // SAFETY: 主线程（会话层在主循环调用）；gravity 切换即时生效不重建窗
        unsafe {
            self.layer
                .setVideoGravity(&objc2_foundation::NSString::from_str(video_gravity(fit)));
        }
    }

    fn set_visible(&mut self, visible: bool) {
        if visible {
            self._window.orderFrontRegardless();
        } else {
            self._window.orderOut(None);
        }
    }

    fn set_frame(&mut self, (x, y, w, h): (f64, f64, f64, f64)) {
        self._window
            .setFrame_display(video_pin_frame((x, y, w, h)), true);
    }

    fn actual_frame(&self) -> Option<MonitorFrame> {
        // 读回还原 video_pin_frame 的 2pt 外扩，保持显示器坐标语义可比
        let f = self._window.frame();
        Some((
            f.origin.x + 2.0,
            f.origin.y + 2.0,
            f.size.width - 4.0,
            f.size.height - 4.0,
        ))
    }

    fn reassert_pinning(&mut self, frame: (f64, f64, f64, f64)) {
        self._window.setLevel(PIN_LEVEL);
        self._window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        self.set_frame(frame);
    }

    fn current_url(&self) -> String {
        "avplayer://native-video".into()
    }
}

/// 视频窗几何：四周外扩 2pt（超出屏幕的部分被 WindowServer 裁掉）。
/// 原因：AspectFill 非整数缩放时 AVPlayerLayer 边缘有 ~2 设备像素的纹理
/// 采样劣化带（双线性钳制采样到帧外，无 API 可关）——外扩即裁剪。
/// 实测 1pt 只盖住一半（边框变细变淡），2pt 在 2x 屏 = 4px 完整吞掉
/// （2026-10-10 双屏实测定位，见会话排障）。
fn video_pin_frame((x, y, w, h): (f64, f64, f64, f64)) -> objc2_foundation::NSRect {
    objc2_foundation::NSRect::new(
        objc2_foundation::NSPoint::new(x - 2.0, y - 2.0),
        objc2_foundation::NSSize::new(w + 4.0, h + 4.0),
    )
}

// CGDisplay 身份三元组（EDID 的 vendor/product/serial）。合盖、重连和唤醒可能重编
// CGDirectDisplayID，但硬件身份通常不变——用三元组做稳定 key；同型号双屏
// serial=0 会碰撞，后面追加 CG ID 消歧。没有可用身份时回落 CG ID。
// macOS 26+ 可能没有这三个 CoreGraphics 符号，因此 identity_fns() 会回落。
type CgIdentityFn = unsafe extern "C" fn(u32) -> u32;

fn identity_fns() -> Option<[CgIdentityFn; 3]> {
    static FNS: std::sync::OnceLock<Option<[CgIdentityFn; 3]>> = std::sync::OnceLock::new();
    *FNS.get_or_init(|| {
        unsafe extern "C" {
            fn dlsym(
                handle: *mut std::ffi::c_void,
                symbol: *const std::ffi::c_char,
            ) -> *mut std::ffi::c_void;
        }
        // SAFETY: RTLD_DEFAULT = (void*)-2，在默认全局句柄中按名字查符号；
        // 三个符号查齐才算可用（缺任何一个都无法构成稳定身份）
        let resolve = |name: &str| -> Option<CgIdentityFn> {
            let c = std::ffi::CString::new(name).ok()?;
            let p = unsafe { dlsym(-2isize as *mut std::ffi::c_void, c.as_ptr()) };
            if p.is_null() {
                None
            } else {
                // SAFETY: 符号即 C 函数地址，签名按 CoreGraphics 文档
                Some(unsafe { std::mem::transmute::<*mut std::ffi::c_void, CgIdentityFn>(p) })
            }
        };
        Some([
            resolve("CGDisplayGetVendorNumber")?,
            resolve("CGDisplayGetModelNumber")?,
            resolve("CGDisplayGetSerialNumber")?,
        ])
    })
}

// SAFETY: 纯查询函数，无全局状态与线程约束
fn display_edid_identity(cgid: u32) -> Option<[u32; 3]> {
    let fns = identity_fns()?;
    let v = unsafe { (fns[0])(cgid) };
    let m = unsafe { (fns[1])(cgid) };
    let s = unsafe { (fns[2])(cgid) };
    // vendor/product 全零 = 无 EDID 身份（AirPlay/虚拟屏）→ 回落 cg-id
    (v != 0 || m != 0).then_some([v, m, s])
}

/// 由 (EDID 身份?, CGDirectDisplayID) 序列生成稳定 ID 列表：
/// - 有身份：`edid-{vendor:04x}{model:04x}{serial:08x}`——跨合盖/重连/重启稳定
/// - 无身份：`cg-{cgid}`（虚拟屏回落，行为同 v1）
/// - 身份重复（同型号多屏且 serial 全零）：追加 `-{cgid}` 消歧，连着时稳定
fn identity_ids(entries: &[(Option<[u32; 3]>, u32)]) -> Vec<String> {
    let mut counts: std::collections::BTreeMap<[u32; 3], usize> = std::collections::BTreeMap::new();
    for (idty, _) in entries {
        if let Some(t) = idty {
            *counts.entry(*t).or_insert(0) += 1;
        }
    }
    entries
        .iter()
        .map(|(idty, cgid)| match idty {
            None => format!("cg-{cgid}"),
            Some(t) if counts[t] > 1 => format!("edid-{:04x}{:04x}{:08x}-{cgid}", t[0], t[1], t[2]),
            Some(t) => format!("edid-{:04x}{:04x}{:08x}", t[0], t[1], t[2]),
        })
        .collect()
}

/// 枚举 (MonitorInfo, NSScreen) 对——desktop_bridge 按显示器 ID 定位系统屏
/// （设/读桌面图）用。必须主线程。
pub fn screens() -> Vec<(MonitorInfo, Retained<NSScreen>)> {
    let mtm = match MainThreadMarker::new() {
        Some(m) => m,
        None => return vec![],
    };
    let mut screens: Vec<(objc2_foundation::NSRect, u32, Retained<NSScreen>)> = Vec::new();
    // SAFETY: 主线程标记保证
    for screen in NSScreen::screens(mtm).iter() {
        // SAFETY: NSArray<NSScreen> 元素类型保证，指针级 cast 绕过 NSObject 外观
        let screen: &NSScreen = unsafe { &*(std::ptr::from_ref(&**screen) as *const NSScreen) };
        let frame = screen.frame();
        // 设备描述里的 NSScreenNumber 即 CGDirectDisplayID
        let key = objc2_foundation::NSString::from_str("NSScreenNumber");
        let cgid = screen
            .deviceDescription()
            .objectForKey(&key)
            .map(|v| unsafe {
                // SAFETY: NSScreenNumber 值为 NSNumber；指针级转 NSNumber 后取值
                let nsnumber: &objc2_foundation::NSNumber =
                    &*(std::ptr::from_ref(&*v) as *const objc2_foundation::NSNumber);
                nsnumber.unsignedIntValue()
            })
            .unwrap_or(0);
        // SAFETY: 同上指针级 cast；Retained 接管不转移所有权（数组保活）
        let retained: Retained<NSScreen> =
            unsafe { Retained::retain(screen as *const NSScreen as *mut NSScreen) }
                .expect("NSScreen 保活");
        screens.push((frame, cgid, retained));
    }

    let identities: Vec<(Option<[u32; 3]>, u32)> = screens
        .iter()
        .map(|(_, cgid, _)| (display_edid_identity(*cgid), *cgid))
        .collect();
    let ids = identity_ids(&identities);

    screens
        .into_iter()
        .zip(ids)
        .enumerate()
        .map(|(i, ((frame, _, screen), id))| {
            let info = MonitorInfo {
                id,
                name: format!("显示器 {}", i + 1),
                frame: (
                    frame.origin.x,
                    frame.origin.y,
                    frame.size.width,
                    frame.size.height,
                ),
                is_main: frame.origin.x == 0.0 && frame.origin.y == 0.0,
            };
            (info, screen)
        })
        .collect()
}

/// 枚举显示器（稳定 ID = EDID 身份三元组；无身份回落 CGDirectDisplayID）。
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    screens().into_iter().map(|(m, _)| m).collect()
}

// —— 过渡窗（桌面桥接新路线：重配/建窗间隙的静帧遮盖）——

/// 纯 AppKit 静帧窗：同 PIN_LEVEL，建窗即显示。晚于它创建/复显的真壁纸
/// 窗自然盖在其上，点亮后即可销毁过渡窗（Drop 先 orderOut，幽灵窗纪律）。
/// 毫秒级建窗，不碰系统桌面/壁纸代理。
pub struct MacCoverWindow {
    window: Retained<NSWindow>,
    _image_view: Retained<NSImageView>,
}

impl Drop for MacCoverWindow {
    fn drop(&mut self) {
        self.window.orderOut(None);
    }
}

/// RGBA 像素 → NSImage（主线程；位图包装是内存拷贝级，毫秒）。
/// SAFETY: objc2 声明的安全方法，按文档语义调用；bitmapData 写入不超过
/// 分配尺寸（w×h×4，由 rep 初始化参数保证）。
pub fn nsimage_from_rgba(
    rgba: &[u8],
    w: usize,
    h: usize,
    mtm: MainThreadMarker,
) -> Retained<NSImage> {
    use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSImageScaling};
    use objc2_foundation::{NSRect, NSSize};

    unsafe {
        let rep = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            mtm.alloc(),
            std::ptr::null_mut(),
            w as isize,
            h as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            (w * 4) as isize,
            32,
        )
        .expect("位图初始化");
        std::ptr::copy_nonoverlapping(rgba.as_ptr(), rep.bitmapData(), rgba.len());
        let img = NSImage::initWithSize(mtm.alloc(), NSSize::new(w as f64, h as f64));
        img.addRepresentation(&rep);
        let _ = NSImageScaling::ScaleProportionallyUpOrDown; // 供 cover 窗用
        let _ = NSRect::ZERO;
        img
    }
}

/// 创建并立即显示过渡窗（主线程）。frame 为显示器坐标系全屏帧。
pub fn create_cover_window(
    frame: MonitorFrame,
    image: Retained<NSImage>,
) -> Result<MacCoverWindow> {
    use objc2_app_kit::{NSImageScaling, NSImageView};
    use objc2_foundation::NSRect;

    let mtm = MainThreadMarker::new()
        .ok_or_else(|| GessoError::UnsupportedPlatform("非主线程".into()))?;
    let window = make_pin_window(mtm);
    let (x, y, w, h) = frame;
    let rect = NSRect::new(
        objc2_foundation::NSPoint::new(x, y),
        objc2_foundation::NSSize::new(w, h),
    );
    window.setFrame_display(rect, true);
    let view = NSImageView::initWithFrame(mtm.alloc(), rect);
    view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    view.setImage(Some(&image));
    let content = window.contentView().expect("contentView 缺失");
    content.addSubview(&view);
    window.orderFrontRegardless();
    Ok(MacCoverWindow {
        window,
        _image_view: view,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ids_edid_fallback_and_dedup() {
        let a = Some([0x0610, 0x9cd2, 0]);
        let b = Some([0x0610, 0x9cd2, 0]); // 同型号多屏、serial 全零 → 消歧
        let c = Some([0x1e6d, 0x5763, 0x0101_0202]);
        // 唯一身份 → edid- 前缀
        assert_eq!(
            identity_ids(&[(c, 777)]),
            vec!["edid-1e6d576301010202".to_string()]
        );
        // 无 EDID（虚拟屏）→ 回落 cg-
        assert_eq!(identity_ids(&[(None, 42)]), vec!["cg-42".to_string()]);
        // 同身份两屏 → 追加 cgid 消歧
        assert_eq!(
            identity_ids(&[(a, 500), (b, 501)]),
            vec![
                "edid-06109cd200000000-500".to_string(),
                "edid-06109cd200000000-501".to_string(),
            ]
        );
    }
}
