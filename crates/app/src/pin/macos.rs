//! macOS 贴壁实现（M1.5 spike 产品化）。
//!
//! 关键参数定稿（SPIKE-REPORT M1.5）：
//! - level = -2147483604（图标层下缘之下一档；勿用桌面图同级，排序不可控）
//! - Borderless + CanJoinAllSpaces|Stationary|FullScreenAuxiliary|IgnoresCycle
//! - ignoresMouseEvents + 透明底（webview 隐藏 = 露系统壁纸）

use objc2::rc::Retained;
use objc2_app_kit::{
    NSColor, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::MainThreadMarker;
use raw_window_handle::{AppKitWindowHandle, HasWindowHandle, RawWindowHandle, WindowHandle};

use super::{MonitorInfo, WallpaperWindow};
use gesso_core::{GessoError, Result};

pub(crate) const PIN_LEVEL: isize = -2147483604;

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
    window.setFrame_display(
        objc2_foundation::NSRect::new(
            objc2_foundation::NSPoint::new(x, y),
            objc2_foundation::NSSize::new(w, h),
        ),
        true,
    );
    window.orderFrontRegardless();

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
        self.webview
            .url()
            .map(|u| u.to_string())
            .unwrap_or_else(|e| format!("<{e}>"))
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
/// 管线有显示锁相，且直接 file:// 读盘，媒体回环 HTTP/gesso:// 均不参与。
pub struct MacVideoWindow {
    _window: Retained<NSWindow>,
    _looper: Retained<objc2_av_foundation::AVPlayerLooper>,
    player: Retained<objc2_av_foundation::AVQueuePlayer>,
}

pub fn create_video_window(
    monitor: &MonitorInfo,
    url: &str,
) -> gesso_core::Result<MacVideoWindow> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| GessoError::UnsupportedPlatform("非主线程".into()))?;
    let window = make_pin_window(mtm);
    let (x, y, w, h) = monitor.frame;
    window.setFrame_display(video_pin_frame((x, y, w, h)), true);

    let content = window.contentView().expect("contentView 缺失");
    // SAFETY: 主线程；AVFoundation 家族方法按文档语义调用
    let (player, looper) = unsafe {
        use objc2_av_foundation::{AVPlayerItem, AVPlayerLayer, AVPlayerLooper, AVQueuePlayer};
        use objc2_foundation::{NSArray, NSString};
        let player = AVQueuePlayer::queuePlayerWithItems(&NSArray::new(), mtm);
        let layer = AVPlayerLayer::playerLayerWithPlayer(Some(&player));
        // cover 语义：与宿主页 object-fit:cover 一致（常量底层就是这三个字符串）
        layer.setVideoGravity(&objc2_foundation::NSString::from_str(
            "AVLayerVideoGravityResizeAspectFill",
        ));
        content.setWantsLayer(true);
        content.setLayer(Some(&layer));
        // 媒体 URL：http:// 回环（内存直供）或 file:// 回落，均为完整 URL 形态
        let nsurl = objc2_foundation::NSURL::URLWithString(&NSString::from_str(url))
            .ok_or_else(|| GessoError::UnsupportedPlatform(format!("媒体 URL 非法：{url}")))?;
        let item = AVPlayerItem::playerItemWithURL(&nsurl, mtm);
        let looper = AVPlayerLooper::playerLooperWithPlayer_templateItem(&player, &item);
        player.setMuted(true); // AudioPolicy::Muted（技术方案 §5.1）
        player.play();
        (player, looper)
    };
    window.orderFrontRegardless();
    println!("[session] 原生视频窗 {url}");
    Ok(MacVideoWindow {
        _window: window,
        _looper: looper,
        player,
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

    fn set_visible(&mut self, visible: bool) {
        if visible {
            self._window.orderFrontRegardless();
        } else {
            self._window.orderOut(None);
        }
    }

    fn set_frame(&mut self, (x, y, w, h): (f64, f64, f64, f64)) {
        self._window.setFrame_display(video_pin_frame((x, y, w, h)), true);
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

// CGDisplay 身份三元组（EDID 的 vendor/product/serial）。合盖/重连/唤醒会重编
// CGDirectDisplayID（v1 用它当 ID 的已知缺陷，见 session.rs），但硬件身份不变——
// 它就是跨重配的稳定 ID 来源。
//
// 可用性（2026-10-09 实测）：macOS 26 把这三个符号从 CoreGraphics 的 SDK 接口
// **和运行时 dylib** 里都删除了（dlsym 探测 MISSING）——26+ 上 identity_fns()
// 返回 None，自动回落 cg-id（v1 行为）。macOS 13–15 运行时符号仍在，dlsym 有效；
// 26+ 的完整 EDID 身份需走 IOKit IODisplayConnect（后续工作，MonitorControl 同路）。
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

/// 枚举显示器（稳定 ID = EDID 身份三元组；无身份回落 CGDirectDisplayID）。
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mtm = match MainThreadMarker::new() {
        Some(m) => m,
        None => return vec![],
    };
    let mut screens: Vec<(objc2_foundation::NSRect, u32)> = Vec::new();
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
            .and_then(|v| unsafe {
                // SAFETY: NSScreenNumber 值为 NSNumber；指针级转 NSNumber 后取值
                let nsnumber: &objc2_foundation::NSNumber =
                    &*(std::ptr::from_ref(&*v) as *const objc2_foundation::NSNumber);
                Some(nsnumber.unsignedIntValue())
            })
            .unwrap_or(0);
        screens.push((frame, cgid));
    }

    let identities: Vec<(Option<[u32; 3]>, u32)> = screens
        .iter()
        .map(|(_, cgid)| (*cgid, display_edid_identity(*cgid)))
        .map(|(cgid, idty)| (idty, cgid))
        .collect();
    let ids = identity_ids(&identities);

    screens
        .iter()
        .zip(ids)
        .enumerate()
        .map(|(i, ((frame, _), id))| MonitorInfo {
            id,
            name: format!("显示器 {}", i + 1),
            frame: (
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height,
            ),
            is_main: frame.origin.x == 0.0 && frame.origin.y == 0.0,
        })
        .collect()
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
