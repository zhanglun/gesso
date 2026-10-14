//! Windows 桥实现：Win32 纯轮询，无需任何权限（对照 macOS 桥的两条纪律）。
//!
//! - 光标：GetCursorPos（顶左原点物理像素，与 MonitorInfo.frame 同系）+
//!   GetAsyncKeyState 按键 + GetLastInputInfo 空闲时长（含键盘活动——比 macOS
//!   的纯鼠标空闲更适合降帧判定：打字也算「有人在场」）。
//!   宿主页契约是左下原点（fragCoord/iMouse），翻转发生在 session::poll_mouse
//!   的契约边界（一次，勿在桥内预翻——见工程笔记「勿翻两次」）。
//! - 全屏：EnumWindows 找 rect 完整覆盖显示器 frame（±3px）的可见顶层窗口。
//!   排除：自身类 GessoWallpaper（壁纸/采集窗口本来就铺满全屏）、shell 窗口
//!   （Progman/WorkerW 恒为全屏 rect）、工具窗、DWM cloak（UWP 挂起幽灵窗口
//!   带全屏 rect 却不可见）。
//! - 电源：GetSystemPowerStatus（ACLineStatus 0=电池供电 / 1=交流；
//!   BatteryFlag 128 = 系统无电池 → None）。
//! - 锁屏：WTSQuerySessionInformationW(WTSSessionInfoEx) 读会话锁标志——
//!   纯轮询，与桥内其余信号同款纪律（无需 HWND + 消息泵）。

use std::collections::BTreeSet;

use windows::core::BOOL;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::RemoteDesktop::{
    WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW,
    WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetLastInputInfo, LASTINPUTINFO, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetCursorPos, GetWindowLongW, GetWindowRect, IsWindowVisible,
    GWL_EXSTYLE, WS_EX_TOOLWINDOW,
};

use crate::pin;

/// 枚举回调的共享状态（引用与数据不得混排在同一元组里强转——见函数内注释）。
struct FsState<'a> {
    monitors: Vec<crate::pin::MonitorInfo>,
    out: &'a mut BTreeSet<String>,
}

/// 正被全屏应用覆盖的显示器 ID，与 `MonitorInfo.id` 同源。
pub fn fullscreen_displays() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let monitors = pin::windows::enumerate_monitors();
    if monitors.is_empty() {
        return out;
    }
    let mut state = FsState {
        monitors,
        out: &mut out,
    };
    unsafe {
        // SAFETY: state 以裸指针经 LPARAM 传入枚举回调，枚举在调用线程同步完成
        let _ = EnumWindows(
            Some(fullscreen_enum_proc),
            LPARAM(&mut state as *mut FsState as isize),
        );
    }
    out
}

unsafe extern "system" fn fullscreen_enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam 指向调用栈上的 FsState，枚举期间存活
    let st = &mut *(lparam.0 as *mut FsState);
    let monitors = &st.monitors;

    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }
    // 自身窗口：壁纸/采集窗口（GessoWallpaper 类）恒为全屏 rect，必须排除
    if is_class(hwnd, w!("GessoWallpaper")) || is_shell_window(hwnd) {
        return BOOL(1);
    }
    // 工具窗（托盘浮层、调试悬浮条等）不算全屏主体
    let exstyle = GetWindowLongW(hwnd, GWL_EXSTYLE);
    if exstyle & (WS_EX_TOOLWINDOW.0 as i32) != 0 {
        return BOOL(1);
    }
    // DWM cloak：UWP/浏览器挂起的幽灵窗口保留着全屏 rect 但实际不可见
    let mut cloaked = 0u32;
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut _,
        std::mem::size_of::<u32>() as u32,
    )
    .is_ok()
        && cloaked != 0
    {
        return BOOL(1);
    }

    let mut r = RECT::default();
    if GetWindowRect(hwnd, &mut r).is_err() {
        return BOOL(1);
    }
    let (wx, wy, ww, wh) = (
        r.left as f64,
        r.top as f64,
        (r.right - r.left) as f64,
        (r.bottom - r.top) as f64,
    );
    if ww < 100.0 || wh < 100.0 {
        return BOOL(1); // 悬浮小窗与显示器 frame 的巧合重合无意义
    }

    for m in monitors.iter() {
        let (mx, my, mw, mh) = m.frame;
        // 与 macOS 同款判定：窗口 rect 与显示器 frame 完全重合（±3px；
        // 最大化窗口是 work-area 不含任务栏，天然不会误判为全屏）
        let hit = (ww - mw).abs() < 3.0
            && (wh - mh).abs() < 3.0
            && (wx - mx).abs() < 3.0
            && (wy - my).abs() < 3.0;
        if hit {
            st.out.insert(m.id.clone());
        }
    }
    BOOL(1)
}

unsafe fn is_class(hwnd: HWND, class: PCWSTR) -> bool {
    let mut buf = [0u16; 64];
    let n = GetClassNameW(hwnd, &mut buf);
    if n <= 0 {
        return false;
    }
    // class 是形如 w!("GessoWallpaper") 的 NUL 结尾字面量，逐位比较
    let want: Vec<u16> = class.as_wide().to_vec();
    buf[..want.len()] == want[..]
}

/// shell 窗口（恒为全屏 rect，但不是全屏应用）：Progman / WorkerW / 任务栏。
unsafe fn is_shell_window(hwnd: HWND) -> bool {
    is_class(hwnd, w!("Progman"))
        || is_class(hwnd, w!("WorkerW"))
        || is_class(hwnd, w!("Shell_TrayWnd"))
}

/// 电源态：Some(true)=电池供电。台式机（无电池）返回 None。
pub fn on_battery() -> Option<bool> {
    unsafe {
        let mut sps = SYSTEM_POWER_STATUS::default();
        GetSystemPowerStatus(&mut sps).ok()?;
        if sps.BatteryFlag == 128 {
            return None; // 无系统电池
        }
        match sps.ACLineStatus {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        }
    }
}

/// 锁屏态（会话级）：WTSSessionInfoEx.SessionFlags，随 bridge::sample 一拍轮询。
/// 已知语义坑：LOCK/UNLOCK 两值在 Win7 及以前文档化互换（Win8 修复）——WebView2
/// 运行时本身要求 Win10+，按文档语义直读。查询失败（无活动会话/Secure 桌面）
/// 按未锁，与 macOS 桥「无 GUI 会话返回 NULL 按未锁」同纪律。
pub fn is_locked() -> bool {
    let mut buf = PWSTR::null();
    let mut len = 0u32;
    // SAFETY: buf/len 为出参；成功时 buf 指向 WTS 分配的 WTSINFOEXW，下方读取并释放
    let ok = unsafe {
        WTSQuerySessionInformationW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buf,
            &mut len,
        )
    };
    if ok.is_err() || buf.0.is_null() {
        return false;
    }
    // SAFETY: WTSSessionInfoEx 契约——成功返回的缓冲即 WTSINFOEXW
    let info = unsafe { &*(buf.0 as *const WTSINFOEXW) };
    // SessionFlags 在 Level 1 联合体分支；读联合体字段是 unsafe
    let locked = info.Level == 1
        && unsafe { info.Data.WTSInfoExLevel1.SessionFlags } as u32 == WTS_SESSIONSTATE_LOCK;
    // SAFETY: 与查询配对释放，杜绝泄漏
    unsafe { WTSFreeMemory(buf.0 as *mut core::ffi::c_void) };
    locked
}

/// 光标位置（顶左原点物理像素，与 MonitorInfo.frame 同系，供后台高频比对）。
pub fn mouse_location() -> (f64, f64) {
    let mut p = POINT::default();
    // SAFETY: 局部 POINT；失败（如 Secure 桌面）保持 (0,0)，调用方按离屏处理
    let _ = unsafe { GetCursorPos(&mut p) };
    (p.x as f64, p.y as f64)
}

/// 按键位与空闲秒数（GetAsyncKeyState + GetLastInputInfo，纯轮询）。
pub fn mouse_buttons_idle() -> (u8, f64) {
    unsafe {
        let down = |vk: i32| GetAsyncKeyState(vk) as u16 & 0x8000 != 0;
        let buttons = (down(VK_LBUTTON.0 as i32)) as u8
            | ((down(VK_RBUTTON.0 as i32)) as u8) << 1
            | ((down(VK_MBUTTON.0 as i32)) as u8) << 2;

        let mut li = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        let idle_secs = if GetLastInputInfo(&mut li).as_bool() {
            // GetTickCount 49.7 天回绕：wrapping_sub 对回绕场景同样正确
            (GetTickCount().wrapping_sub(li.dwTime)) as f64 / 1000.0
        } else {
            0.0
        };
        (buttons, idle_secs)
    }
}
