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

const PIN_LEVEL: isize = -2147483604;

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
        for (i, sub) in unsafe { c.subviews() }.iter().enumerate().take(3) {
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

pub fn create(monitor: &MonitorInfo) -> Result<MacWallpaperWindow> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| GessoError::UnsupportedPlatform("非主线程".into()))?;

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
    window.setOpaque(false);
    let clear = unsafe { NSColor::clearColor() };
    window.setBackgroundColor(Some(&clear));

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

    // 初始 URL 必须是一个必然成功的中立页：早期版本误用已废弃的 gesso:// URL 作为
    // 首帧导航，失败后 WKWebView 进入"URL 会更新但永不绘制"的死状态（M2 排查记录）
    let webview = crate::protocol::create_webview(handle, "about:blank")
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
            for (i, sub) in unsafe { c.subviews() }.iter().enumerate().take(3) {
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
        // 宿主页契约：window.__gesso.pause()/resume()（技术方案 §5.1/§5.3）
        let js = if paused {
            "__gesso&&__gesso.pause()"
        } else {
            "__gesso&&__gesso.resume()"
        };
        let _ = self.webview.evaluate_script(js);
    }

    fn set_visible(&mut self, visible: bool) {
        let _ = self.webview.set_visible(visible);
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
}

/// 枚举显示器（稳定 ID = CGDirectDisplayID）。
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mtm = match MainThreadMarker::new() {
        Some(m) => m,
        None => return vec![],
    };
    let mut out = Vec::new();
    // SAFETY: 主线程标记保证
    for screen in unsafe { NSScreen::screens(mtm) }.iter() {
        // SAFETY: NSArray<NSScreen> 元素类型保证，指针级 cast 绕过 NSObject 外观
        let screen: &NSScreen = unsafe { &*(std::ptr::from_ref(&**screen) as *const NSScreen) };
        let frame = screen.frame();
        // 设备描述里的 NSScreenNumber 即 CGDirectDisplayID（v1 稳定 ID）
        let key = objc2_foundation::NSString::from_str("NSScreenNumber");
        let id = screen
            .deviceDescription()
            .objectForKey(&key)
            .and_then(|v| unsafe {
                // SAFETY: NSScreenNumber 值为 NSNumber；指针级转 NSNumber 后取值
                let nsnumber: &objc2_foundation::NSNumber =
                    &*(std::ptr::from_ref(&*v) as *const objc2_foundation::NSNumber);
                Some(nsnumber.unsignedIntValue())
            })
            .map(|n| format!("cg-{n}"))
            .unwrap_or_else(|| format!("anon-{:?}", (frame.origin.x, frame.size.width)));
        out.push(MonitorInfo {
            id,
            name: format!("显示器 {}", out.len() + 1),
            frame: (
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height,
            ),
            is_main: frame.origin.x == 0.0 && frame.origin.y == 0.0,
        });
    }
    out
}
