//! Windows 贴壁实现（M1）。
//!
//! 挂载阶梯（技术方案 §4.1，参考 Lively 思路，未复制代码）：
//! 1. `SendMessageTimeout(Progman, 0x052C)` → Explorer 在图标层之后生成一个 WorkerW
//!    （壁纸层；图标 SHELLDLL_DefView 仍在原 WorkerW/Progman 上）；
//! 2. 壁纸窗口 `SetParent` 到该 WorkerW——图标/右键菜单在其上，全屏应用盖住一切；
//! 3. 兜底：找不到图标层（用户关闭「显示桌面图标」）→ `SetParent` 到 Progman；
//!    Progman 也缺（explorer 未就绪）→ 保持顶层窗口压 HWND_BOTTOM，等待重钉。
//!
//! **创建顺序（实机验证 2026-10-05）**：必须**先 SetParent 挂载、后创建 WebView2**。
//! WebView2 的 DirectComposition 视觉树在 controller 创建时绑定宿主层级——
//! 反过来先创建后挂载，整窗内容不可见（窗口树全绿：visible/非 cloak/尺寸正确，
//! DWM 就是不合成）；脱离 WorkerW 后同一窗口立即正常渲染。explorer 重启重钉
//! 同理：SetParent 后必须重建 webview（`WinWallpaperWindow::remount`）。
//!
//! explorer.exe 重启会重建 Progman/WorkerW，本进程壁纸窗口沦为孤儿顶层：
//! 隐藏监听窗口收 `TaskbarCreated` 广播 → 置 REMOUNT_PENDING → 引擎轮询
//! （main.rs 每 ~2s）调 `SessionManager::remount_all` → 各窗口重走挂载 + 重建 webview。
//!
//! 点击穿透：不用 WS_EX_LAYERED（分层重定向面与 WebView2 子窗口有合成风险，
//! 实测旁证：剥离 layered 不影响可见性）——窗口过程直接回 `HTTRANSPARENT`。
//!
//! 坐标与 DPI：进程在 main() 顶部声明 PerMonitorV2（运行时调用，先于任何窗口创建），
//! 全程虚拟桌面**物理像素**；挂载后子窗口定位相对挂载点窗口原点换算（`child_offset`，
//! WorkerW/Progman 均为无边框 popup，窗口矩形 = 客户区）。
//!
//! webview：lb-wry 的 Windows 后端把自定义协议导航 `gesso://X/…` 翻译为
//! `http://gesso.X/…`（WebView2 workaround，回程在协议回调里还原，路由不分平台）。
//! wry 只翻译构建期 with_url，运行期 `load_url` 不翻——本层 `load()` 自行翻译；
//! 子资源 URL（spec.source）由 `protocol::entry_url` 引擎侧直接产出 workaround 形态。

use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use windows::core::{w, Interface as _, BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDeviceAndSwapChain, ID3D11Texture2D, D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_SCALING_UNSPECIFIED,
    DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGISwapChain1, DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW,
    HBRUSH, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::Media::MediaFoundation::{
    IMFSample, MFCreateMediaType, MFCreateSourceReaderFromURL, MFShutdown, MFStartup,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EnumWindows, FindWindowExW, FindWindowW,
    GetWindowRect, IsWindowVisible, RegisterClassW, RegisterWindowMessageW, SendMessageTimeoutW,
    SetLayeredWindowAttributes, SetParent, SetWindowPos, ShowWindow, HTTRANSPARENT, HWND_BOTTOM,
    LWA_ALPHA, MONITORINFOF_PRIMARY, SMTO_NORMAL, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNA,
    WINDOW_EX_STYLE, WM_NCHITTEST, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_OVERLAPPED, WS_POPUP,
};

use super::{MonitorInfo, WallpaperWindow};
use gesso_core::{GessoError, Result};

const WALLPAPER_CLASS: PCWSTR = w!("GessoWallpaper");
const LISTENER_CLASS: PCWSTR = w!("GessoTaskbarListener");

/// Progman 私有消息：让 Explorer 在图标层后生成壁纸 WorkerW（公开的民间事实，Lively 同款）。
const WM_PROGMAN_SPAWN_WORKERW: u32 = 0x052C;

/// 壁纸窗口挂载形态（诊断与重钉策略用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MountKind {
    /// 正常路径：Explorer 生成的壁纸 WorkerW（图标层之下）。
    WorkerW,
    /// 兜底：Progman 本身（桌面图标关闭时即壁纸层；旧系统图标也在其上）。
    Progman,
    /// 兜底：保持顶层窗口压最底（explorer 未就绪，等待重钉）。
    BottomMost,
}

static PIN_ENV: std::sync::Once = std::sync::Once::new();
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
/// TaskbarCreated 已到（explorer 重启）→ 引擎轮询取走并触发各会话 remount。
static REMOUNT_PENDING: AtomicBool = AtomicBool::new(false);
/// 存活的贴壁窗口句柄（仅 create() 的壁纸窗口，不含采集 overlay）。
/// panic 钩子的 emergency_teardown 按此清单尽力销毁。
static LIVE_WALLPAPER_HWNDS: std::sync::Mutex<Vec<isize>> = std::sync::Mutex::new(Vec::new());

// ---------------------------------------------------------------------------
// 纯逻辑（单测覆盖）：URL 翻译与子窗口定位换算
// ---------------------------------------------------------------------------

/// `gesso://X/…` → `http://gesso.X/…`（WebView2 workaround 导航形态，wry 同款变换）。
pub(crate) fn workaround_url(url: &str) -> String {
    url.replacen("gesso://", "http://gesso.", 1)
}

/// 逆向翻译（`current_url` 展示用，让两平台日志形态一致）。
fn unworkaround_url(url: &str) -> String {
    url.replacen("http://gesso.", "gesso://", 1)
}

/// 挂载后子窗口定位：挂载点（WorkerW/Progman）覆盖整个虚拟桌面，其原点即虚拟桌面
/// 坐标系原点；子窗口坐标 = 显示器虚拟桌面坐标 − 挂载点原点。
fn child_offset(parent_origin: (i32, i32), monitor_origin: (i32, i32)) -> (i32, i32) {
    (
        monitor_origin.0 - parent_origin.0,
        monitor_origin.1 - parent_origin.1,
    )
}

/// 设备名 → 稳定 ID：`\\.\DISPLAY1` → `win-DISPLAY1`（v1 限制同 macOS：
/// 接口重排会重编号，EDID 哈希是升级路径，技术方案 §4.3）。
fn monitor_id_from_device(device: &str) -> String {
    format!("win-{}", device.trim_start_matches(r"\\.\"))
}

/// 显示器硬件身份：该屏挂的 monitor PnP ID（EDID 派生，如 `dela0bc`），
/// 跨重启/驱动重装稳定——DISPLAYn 枚举序号则不是（v1 已知缺陷）。取不到回落。
fn monitor_pnp_id(device: &str) -> Option<String> {
    let mut dd = DISPLAY_DEVICEW {
        cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    let device_w: Vec<u16> = device.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: dd 为本栈帧局部，cbSize 按 API 约定填写；device_w 含 NUL 终止
    if unsafe { EnumDisplayDevicesW(PCWSTR(device_w.as_ptr()), 0, &mut dd, 0) }.as_bool() {
        let device_id = String::from_utf16_lossy(&dd.DeviceID)
            .trim_end_matches('\0')
            .to_string();
        monitor_pnp_id_parse(&device_id)
    } else {
        None
    }
}

/// DeviceID → PnP 身份段（纯函数，可测）。两种驱动形态，身份都是 EDID 派生段：
///   注册表形态  MONITOR\DELA0BC\<instance>
///   接口形态    \\?\DISPLAY#DELA0BC#<instance>#{guid}
/// 按分隔符切段后取第一个非空、非 MONITOR/DISPLAY/`?`（接口形态 \\?\ 前缀的
/// 段，真 Windows 实测曾解析成 Some("?")——CI 的 Windows job 不跑 app 测试
/// 所以漏网）的段。
fn monitor_pnp_id_parse(device_id: &str) -> Option<String> {
    let seg = device_id
        .split(['\\', '#'])
        .find(|s| !s.is_empty() && *s != "MONITOR" && *s != "DISPLAY" && *s != "?")
        .unwrap_or_default()
        .to_ascii_lowercase();
    (!seg.is_empty()).then_some(seg)
}

/// 身份 → 稳定 ID：`edid-<pnp>`；同 PnP 多屏（同型号多显示器）追加设备名消歧；
/// 无身份回落 `win-DISPLAYN`（v1 行为）。
fn assign_monitor_ids(rows: &[(String, Option<String>)]) -> Vec<String> {
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (_, pnp) in rows {
        if let Some(p) = pnp {
            *counts.entry(p.clone()).or_insert(0) += 1;
        }
    }
    rows.iter()
        .map(|(device, pnp)| match pnp {
            Some(p) if counts[p] > 1 => {
                format!("edid-{p}-{}", device.trim_start_matches(r"\\.\"))
            }
            Some(p) => format!("edid-{p}"),
            None => monitor_id_from_device(device),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 显示器枚举
// ---------------------------------------------------------------------------

/// 枚举回调的中间行：device 名 + PnP 身份 + 几何（id 在 enumerate 末尾统一生成，
/// 同型号消歧需要全量视图）。
struct EnumRow {
    device: String,
    pnp: Option<String>,
    frame: (f64, f64, f64, f64),
    is_main: bool,
}

/// 枚举显示器（虚拟桌面物理像素，top-left 原点）。顺序稳定：主屏优先，余按 (x, y)。
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mut rows: Vec<EnumRow> = Vec::new();
    unsafe {
        let lparam = LPARAM(&mut rows as *mut Vec<EnumRow> as isize);
        let _ = EnumDisplayMonitors(None, None, Some(enum_monitor_proc), lparam);
    }
    rows.sort_by(|a, b| {
        b.is_main
            .cmp(&a.is_main)
            .then(a.frame.0.total_cmp(&b.frame.0))
            .then(a.frame.1.total_cmp(&b.frame.1))
    });
    let ids = assign_monitor_ids(
        &rows
            .iter()
            .map(|r| (r.device.clone(), r.pnp.clone()))
            .collect::<Vec<_>>(),
    );
    rows.into_iter()
        .zip(ids)
        .enumerate()
        .map(|(i, (r, id))| MonitorInfo {
            id,
            name: format!("显示器 {}", i + 1),
            frame: r.frame,
            is_main: r.is_main,
        })
        .collect()
}

unsafe extern "system" fn enum_monitor_proc(
    _hmon: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let rows = &mut *(lparam.0 as *mut Vec<EnumRow>);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: info 为本栈帧局部，GetMonitorInfoW 按 cbSize 写入
    if !GetMonitorInfoW(_hmon, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
        return BOOL(1); // 单个显示器失败不中断枚举
    }
    let r = info.monitorInfo.rcMonitor;
    let device = String::from_utf16_lossy(&info.szDevice)
        .trim_end_matches('\0')
        .to_string();
    rows.push(EnumRow {
        pnp: monitor_pnp_id(&device),
        device,
        frame: (
            r.left as f64,
            r.top as f64,
            (r.right - r.left) as f64,
            (r.bottom - r.top) as f64,
        ),
        is_main: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
    });
    BOOL(1)
}

// ---------------------------------------------------------------------------
// WorkerW 挂载阶梯
// ---------------------------------------------------------------------------

/// 触发 Explorer 生成壁纸 WorkerW 并找到挂载点（挂载阶梯 1–3）。
fn find_wallpaper_parent() -> (MountKind, Option<HWND>) {
    unsafe {
        // SMTO_NORMAL 同步等 Explorer 处理（1s 上限）；跨进程 SendMessage 无死锁风险
        if let Ok(progman) = FindWindowW(w!("Progman"), PCWSTR::null()) {
            let mut result = 0usize;
            SendMessageTimeoutW(
                progman,
                WM_PROGMAN_SPAWN_WORKERW,
                WPARAM(0),
                LPARAM(0),
                SMTO_NORMAL,
                1000,
                Some(&mut result),
            );
        }
        // 图标层宿主（含 SHELLDLL_DefView 子窗口的顶层窗口）之后的第一个 WorkerW = 壁纸层
        let mut defview_host: Option<HWND> = None;
        let _ = EnumWindows(
            Some(find_defview_proc),
            LPARAM(&mut defview_host as *mut Option<HWND> as isize),
        );
        if let Some(host) = defview_host {
            if let Ok(worker) = FindWindowExW(None, Some(host), w!("WorkerW"), PCWSTR::null()) {
                return (MountKind::WorkerW, Some(worker));
            }
        }
        // 兜底：图标关闭（无 DefView）→ Progman 即壁纸层
        match FindWindowW(w!("Progman"), PCWSTR::null()) {
            Ok(p) => (MountKind::Progman, Some(p)),
            Err(_) => (MountKind::BottomMost, None),
        }
    }
}

/// 找承载桌面图标的顶层窗口（SHELLDLL_DefView 的父窗口）。
unsafe extern "system" fn find_defview_proc(top: HWND, lparam: LPARAM) -> BOOL {
    let slot = &mut *(lparam.0 as *mut Option<HWND>);
    if slot.is_some() {
        return BOOL(1);
    }
    if FindWindowExW(Some(top), None, w!("SHELLDLL_DefView"), PCWSTR::null()).is_ok() {
        *slot = Some(top);
        return BOOL(0); // 找到即停
    }
    BOOL(1)
}

/// 挂载 + 按显示器矩形定位（create 与 remount 共用）。幂等。
/// **必须在创建 WebView2 之前调用**（见模块注释「创建顺序」）。
fn mount(hwnd: HWND, frame: &[i32; 4]) -> MountKind {
    let (kind, parent) = find_wallpaper_parent();
    unsafe {
        match parent {
            Some(p) => {
                // 先 SetParent 再定位（子窗口坐标相对挂载点）
                if SetParent(hwnd, Some(p)).is_err() {
                    // 跨进程 SetParent 拒绝（UIPI 等）→ 顶层压底兜底
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_BOTTOM),
                        frame[0],
                        frame[1],
                        frame[2],
                        frame[3],
                        SWP_NOACTIVATE,
                    );
                    return MountKind::BottomMost;
                }
                let mut r = RECT::default();
                if GetWindowRect(p, &mut r).is_ok() {
                    let (x, y) = child_offset((r.left, r.top), (frame[0], frame[1]));
                    let _ = SetWindowPos(hwnd, None, x, y, frame[2], frame[3], SWP_NOACTIVATE);
                }
            }
            None => {
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_BOTTOM),
                    frame[0],
                    frame[1],
                    frame[2],
                    frame[3],
                    SWP_NOACTIVATE,
                );
            }
        }
    }
    kind
}

/// TaskbarCreated 是否已到（由引擎轮询取走；取走即清零）。主线程调用。
pub fn take_remount_pending() -> bool {
    REMOUNT_PENDING.swap(false, Ordering::Relaxed)
}

/// 重钉未落位（explorer 未就绪）→ 重新置位，下一轮轮询重试。主线程调用。
pub fn rearm_remount() {
    REMOUNT_PENDING.store(true, Ordering::Relaxed);
}

/// 退出还原桌面（EngineAction::Quit）：壁纸窗口已由会话拆除（Drop → DestroyWindow），
/// 但被盖住的原始壁纸属 explorer 绘制——子窗口消失不会自动触发父窗口重绘，
/// 直接退进程桌面会留黑底。对 Progman / 图标层 / 壁纸 WorkerW 各无效化一遍，
/// 谁持有壁纸谁重绘。主线程调用（teardown_all 之后）。
pub fn restore_desktop() {
    use windows::Win32::Graphics::Gdi::InvalidateRect;
    unsafe {
        let mut targets: Vec<HWND> = Vec::new();
        if let Ok(progman) = FindWindowW(w!("Progman"), PCWSTR::null()) {
            targets.push(progman);
            if let Ok(defview) =
                FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null())
            {
                targets.push(defview);
            }
        }
        // 不走 find_wallpaper_parent：那会向 Progman 发 0x052C 再生一个 WorkerW，
        // 退出时不应改变桌面结构。图标层宿主后的 WorkerW 此时仍在（挂载时创建），
        // 直接静默查找即可；找不到也不阻塞退出。
        let mut defview_host: Option<HWND> = None;
        let _ = EnumWindows(
            Some(find_defview_proc),
            LPARAM(&mut defview_host as *mut Option<HWND> as isize),
        );
        if let Some(host) = defview_host {
            if let Ok(worker) = FindWindowExW(None, Some(host), w!("WorkerW"), PCWSTR::null()) {
                targets.push(worker);
            }
        }
        for h in &targets {
            let _ = InvalidateRect(Some(*h), None, true);
        }
        // 兜底重锤：整个屏幕失效。定向失效可能不够——壁纸窗口销毁后暴露的
        // 区域若被 WorkerW 以黑底擦除，只有逼全部窗口重绘才能让原壁纸回来
        //（退出瞬间的一次全屏重绘，换桌面恢复正常）。
        let _ = InvalidateRect(None, None, true);
        println!(
            "[pin] 桌面还原：{} 个 shell 窗口已无效化 + 全屏失效",
            targets.len()
        );
    }
}

/// 异常退出（panic 钩子调用）的尽力清场：销毁全部存活的贴壁窗口并触发桌面
/// 重绘。DestroyWindow 仅在创建线程合法——panic 发生在其他线程时失败，
/// 静默忽略（进程随后死亡，OS 兜底销毁窗口）。任何路径都不 panic。
pub fn emergency_teardown() {
    let live = LIVE_WALLPAPER_HWNDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain(..)
        .collect::<Vec<isize>>();
    for h in live {
        // SAFETY: 句柄来自本模块登记；跨线程时 DestroyWindow 返回失败，忽略
        unsafe {
            let _ = DestroyWindow(HWND(h as *mut core::ffi::c_void));
        }
    }
    restore_desktop();
}

/// 采集覆盖窗（M4-W，capture.rs 用）：TOPMOST + 3/255 alpha 的近隐形窗口，
/// 采集期显身、空闲期由 capture_win 隐藏（隐藏窗不参与命中测试与合成）。
/// 顶层窗口不被普通应用窗口遮挡 → WebView2 全速渲染（macOS「壁纸层之上一档 +
/// 2% 透明」的 Windows 等价机制；遮挡会让 Chromium 停摆 RAF/合成 → 快照全黑）。
/// TOOLWINDOW 不进任务栏/Alt-Tab，NOACTIVATE + SW_SHOWNA 不抢焦点。
/// 点击穿透必须走 `WS_EX_TRANSPARENT`（文档保证的跨进程路径）：类过程
/// HTTRANSPARENT 按文档仅对同线程下方窗口生效，低 alpha 的命中行为无文档
/// 保证且实测不稳——两者都不足为凭。explorer 重启不影响顶层窗口。
/// 必须主线程调用（采集任务跑 GPUI 前台执行器）。
pub(crate) fn create_overlay_window(logical: (i32, i32)) -> Result<HWND> {
    ensure_pin_env();
    // SAFETY: 查询系统 DPI（PMv2 下 = 主屏 DPI）
    let dpi = unsafe { GetDpiForSystem() } as i32;
    let (w, h) = ((logical.0.max(1) * dpi) / 96, (logical.1.max(1) * dpi) / 96);
    // SAFETY: 主线程；模块句柄查询无特殊前提
    let hwnd = unsafe {
        let hinstance = GetModuleHandleW(PCWSTR::null())
            .map(|m| HINSTANCE(m.0))
            .map_err(|e| GessoError::UnsupportedPlatform(format!("GetModuleHandleW: {e}")))?;
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            WALLPAPER_CLASS,
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            w,
            h,
            None,
            None,
            Some(hinstance),
            None,
        )
        .map_err(|e| GessoError::UnsupportedPlatform(format!("CreateWindowExW(采集): {e}")))?
    };
    // SAFETY: 同线程刚创建的窗口；alpha 3/255 ≈ 1.2%，肉眼不可见
    unsafe {
        if let Err(e) = SetLayeredWindowAttributes(hwnd, COLORREF(0), 3, LWA_ALPHA) {
            eprintln!("[pin] 采集窗口 alpha 设置失败：{e}");
        }
        let _ = ShowWindow(hwnd, SW_SHOWNA);
    }
    Ok(hwnd)
}

// ---------------------------------------------------------------------------
// 壁纸窗口
// ---------------------------------------------------------------------------

/// wry 直挂所需的句柄包装（仅主线程使用）。
struct HwndHandle {
    hwnd: HWND,
    hinstance: isize,
}

// SAFETY: 指针仅在该句柄传给 wry（主线程）期间解引用
impl raw_window_handle::HasWindowHandle for HwndHandle {
    fn window_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError>
    {
        // raw-window-handle 0.6：hwnd/hinstance 为 NonZero<isize>（NULL 句柄无意义）
        let hwnd = NonZero::<isize>::new(self.hwnd.0 as isize).expect("壁纸窗口 HWND 非空");
        let mut handle = raw_window_handle::Win32WindowHandle::new(hwnd);
        handle.hinstance = NonZero::<isize>::new(self.hinstance);
        Ok(unsafe {
            raw_window_handle::WindowHandle::borrow_raw(raw_window_handle::RawWindowHandle::Win32(
                handle,
            ))
        })
    }
}

pub struct WinWallpaperWindow {
    hwnd: HWND,
    /// webview 可随 remount 重建（explorer 重启后合成失效，须重挂 + 重建）
    webview: Option<lb_wry::WebView>,
    /// 当前宿主页 URL（gesso 形态；remount 重建后重载）
    url: Option<String>,
    /// 诊断显示用挂载形态（真值以实际父窗口为准）
    mount: std::cell::Cell<MountKind>,
}

/// 进程级一次性初始化：窗口类注册 + TaskbarCreated 监听窗口。
/// （DPI awareness 不在此——必须先于任何窗口创建，main() 顶部处理。）
fn ensure_pin_env() {
    PIN_ENV.call_once(|| unsafe {
        let hinstance = match GetModuleHandleW(PCWSTR::null()) {
            Ok(m) => HINSTANCE(m.0),
            Err(e) => {
                eprintln!("[pin] GetModuleHandleW 失败：{e}");
                return;
            }
        };
        let brush: HBRUSH = CreateSolidBrush(COLORREF(0)); // 加载期黑底（进程级持有，不释放）

        let wc = WNDCLASSW {
            style: Default::default(),
            lpfnWndProc: Some(wallpaper_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: Default::default(),
            hCursor: Default::default(),
            hbrBackground: brush,
            lpszMenuName: PCWSTR::null(),
            lpszClassName: WALLPAPER_CLASS,
        };
        if RegisterClassW(&wc) == 0 {
            eprintln!(
                "[pin] RegisterClassW(壁纸) 失败：{}",
                windows::core::Error::from_thread()
            );
        }
        let wl = WNDCLASSW {
            style: Default::default(),
            lpfnWndProc: Some(listener_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: Default::default(),
            hCursor: Default::default(),
            hbrBackground: brush,
            lpszMenuName: PCWSTR::null(),
            lpszClassName: LISTENER_CLASS,
        };
        if RegisterClassW(&wl) == 0 {
            eprintln!(
                "[pin] RegisterClassW(监听) 失败：{}",
                windows::core::Error::from_thread()
            );
        }
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::Relaxed,
        );
        // 隐藏监听窗口：必须是普通顶层窗口——message-only（HWND_MESSAGE）收不到广播
        if let Err(e) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            LISTENER_CLASS,
            PCWSTR::null(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance),
            None,
        ) {
            eprintln!("[pin] 监听窗口创建失败：{e}");
        }
    });
}

/// 监听窗口过程：只认 TaskbarCreated 广播（explorer 重启信号）。
unsafe extern "system" fn listener_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let taskbar = TASKBAR_CREATED.load(Ordering::Relaxed);
    if taskbar != 0 && msg == taskbar {
        println!("[pin] TaskbarCreated 广播：explorer 重启，各会话重钉");
        REMOUNT_PENDING.store(true, Ordering::Relaxed);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

/// 壁纸窗口过程：命中测试一律穿透（v1 非交互语义，对齐 macOS ignoresMouseEvents）；
/// crate 的 DefWindowProcW 是 Rust 包装 fn，不能直接作 WNDPROC，需 extern "system" 转发。
unsafe extern "system" fn wallpaper_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCHITTEST {
        return LRESULT(HTTRANSPARENT as isize);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

pub fn create(monitor: &MonitorInfo) -> Result<WinWallpaperWindow> {
    ensure_pin_env();

    let (mx, my, mw, mh) = (
        monitor.frame.0 as i32,
        monitor.frame.1 as i32,
        monitor.frame.2.max(1.0) as i32,
        monitor.frame.3.max(1.0) as i32,
    );

    // SAFETY: 模块句柄查询，无特殊前提
    let hinstance = unsafe { GetModuleHandleW(PCWSTR::null()) }
        .map(|m| HINSTANCE(m.0))
        .map_err(|e| GessoError::UnsupportedPlatform(format!("GetModuleHandleW: {e}")))?;

    // 1. 自有窗口。toolwindow 不进任务栏/Alt-Tab；NOACTIVATE 不抢焦点；
    //    点击穿透走 WM_NCHITTEST（模块注释），不用 WS_EX_LAYERED。
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            WALLPAPER_CLASS,
            PCWSTR::null(),
            WS_POPUP,
            mx,
            my,
            mw,
            mh,
            None,
            None,
            Some(hinstance),
            None,
        )
        .map_err(|e| GessoError::UnsupportedPlatform(format!("CreateWindowExW: {e}")))?
    };

    // 2. 先挂载后创建 webview（创建顺序 = 可见性生死线，见模块注释）
    let frame = [mx, my, mw, mh];
    let kind = mount(hwnd, &frame);

    // 3. webview 直挂本窗口（物理像素铺满）
    let webview = crate::protocol::create_webview(
        HwndHandle {
            hwnd,
            hinstance: hinstance.0 as isize,
        },
        "about:blank",
    )
    .map_err(|e| GessoError::UnsupportedPlatform(format!("wry: {e}")))?;
    let _ = webview.set_bounds(lb_wry::Rect {
        size: lb_wry::dpi::Size::Physical(lb_wry::dpi::PhysicalSize::new(mw as u32, mh as u32)),
        position: lb_wry::dpi::Position::Physical(lb_wry::dpi::PhysicalPosition::new(0, 0)),
    });

    // 4. 显示（不抢焦点）
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNA);
    }
    // 登记：panic 钩子的 emergency_teardown 按此清单尽力销毁
    LIVE_WALLPAPER_HWNDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(hwnd.0 as isize);
    println!(
        "[pin] 壁纸窗口 {mx},{my} {mw}×{mh} → {kind:?}（{}）",
        monitor.id
    );

    Ok(WinWallpaperWindow {
        hwnd,
        webview: Some(webview),
        url: None,
        mount: std::cell::Cell::new(kind),
    })
}

impl WallpaperWindow for WinWallpaperWindow {
    fn load(&mut self, url: &str) {
        // wry 只翻译构建期导航 URL；运行期 load_url 需自行翻译成 WebView2 workaround 形态
        let url = workaround_url(url);
        self.url = Some(unworkaround_url(&url));
        if let Some(w) = self.webview.as_ref() {
            let _ = w.load_url(&url);
        }
    }

    fn set_paused(&mut self, paused: bool) {
        // 宿主页契约（类型化命令见 host_cmd.rs）：暂停 = JS 层停帧，窗口常驻
        let cmd = if paused {
            crate::host_cmd::HostCommand::Pause
        } else {
            crate::host_cmd::HostCommand::Resume
        };
        if let Some(w) = self.webview.as_ref() {
            let _ = w.evaluate_script(&cmd.to_js());
        }
    }

    fn evaluate(&mut self, js: &str) {
        if let Some(w) = self.webview.as_ref() {
            let _ = w.evaluate_script(js);
        }
    }

    fn evaluate_with_callback(&mut self, js: &str, cb: Box<dyn Fn(String) + Send>) {
        // WebView2 ExecuteScript 结果经完成回调回传（JSON 编码——采集窗 eval_cb
        // 同一管线）；与 macOS 侧同款透传
        if let Some(w) = self.webview.as_ref() {
            let _ = w.evaluate_script_with_callback(js, move |v: String| cb(v));
        }
    }

    fn set_visible(&mut self, visible: bool) {
        // 整窗隐藏 = 露出系统壁纸（本窗之下由 Explorer 绘制桌面）
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNA } else { SW_HIDE });
        }
        if let Some(w) = self.webview.as_ref() {
            let _ = w.set_visible(visible);
        }
    }

    fn current_url(&self) -> String {
        self.webview
            .as_ref()
            .and_then(|w| w.url().ok())
            .map(|u| unworkaround_url(&u.to_string()))
            .or_else(|| self.url.clone())
            .unwrap_or_else(|| "<no webview>".into())
    }

    fn set_frame(&mut self, (x, y, w, h): (f64, f64, f64, f64)) {
        let frame = [x as i32, y as i32, w as i32, h as i32];
        let kind = mount(self.hwnd, &frame);
        self.mount.set(kind);
    }

    fn reassert_pinning(&mut self, frame: (f64, f64, f64, f64)) {
        // WorkerW 挂载层级由 mount() 保证，重配后重走一次挂载即重申
        self.set_frame(frame);
    }

    /// 挂载是否落位（BottomMost = explorer 未就绪，调用方重试）。
    fn mount_ok(&self) -> bool {
        self.mount.get() != MountKind::BottomMost
    }

    fn diag(&self, tag: &str) {
        unsafe {
            let mut r = RECT::default();
            let rect = if GetWindowRect(self.hwnd, &mut r).is_ok() {
                format!("({},{},{},{})", r.left, r.top, r.right, r.bottom)
            } else {
                "<bad>".into()
            };
            println!(
                "[diag {tag}] win hwnd={:x} mount={:?} visible={} rect={} url={}",
                self.hwnd.0 as isize,
                self.mount.get(),
                IsWindowVisible(self.hwnd).as_bool(),
                rect,
                self.current_url(),
            );
        }
    }
}

impl Drop for WinWallpaperWindow {
    fn drop(&mut self) {
        // 主线程约束：会话创建/销毁全在引擎轮询（GPUI 主线程）；跨线程 DestroyWindow 会失败
        // webview 字段先于 DestroyWindow 结束（drop 顺序：Option 先取走丢弃）
        self.webview = None;
        // SAFETY: 窗口由本模块在同线程创建
        unsafe {
            if let Err(e) = DestroyWindow(self.hwnd) {
                eprintln!("[pin] DestroyWindow 失败：{e}");
            }
        }
        LIVE_WALLPAPER_HWNDS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|h| *h != self.hwnd.0 as isize);
        println!("[pin] 壁纸窗口已销毁（{:x}）", self.hwnd.0 as isize);
    }
}

// ---------------------------------------------------------------------------
// 原生视频壁纸窗（2026-10-11 立项，对标 macOS MacVideoWindow）
//
// 背景：WebView2 windowed hosting 的呈现路径（渲染进程合成器 → 跨进程纹理 →
// DComp 子窗口 → DWM）实测所有构建/传输/素材全卡（同文件本地播放器与 Edge
// 丝滑，页侧探针全绿）——病灶在呈现交接层，与 macOS 远端层树无锁相同病。
// 解法同构：MF SourceReader 解码（BGRA 软件输出，内置转换器缩放到屏）→
// flip-model swapchain 直挂 WorkerW 子窗口（Present(1) 锁 vsync，PTS 对齐
// 墙钟）→ DWM 合成。与本地播放器同构（2 跳 vs 5 跳），对精简系统免疫
// （win15 spike 2026-10-11 实测丝滑；不依赖被精简工具删除的 WinRT UI 运行时）。
// ---------------------------------------------------------------------------

pub struct WinVideoWindow {
    hwnd: HWND,
    mount: std::cell::Cell<MountKind>,
    /// 渲染线程停止旗（Drop 置位 + join）
    stop: Arc<AtomicBool>,
    /// 暂停旗（全屏/电池/锁屏自动暂停 → 冻结当前帧，窗口常驻）
    paused: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

pub fn create_video_window(monitor: &MonitorInfo, url: &str) -> Result<WinVideoWindow> {
    ensure_pin_env();

    let (mx, my, mw, mh) = (
        monitor.frame.0 as i32,
        monitor.frame.1 as i32,
        monitor.frame.2.max(1.0) as i32,
        monitor.frame.3.max(1.0) as i32,
    );

    // SAFETY: 模块句柄查询，无特殊前提
    let hinstance = unsafe { GetModuleHandleW(PCWSTR::null()) }
        .map(|m| HINSTANCE(m.0))
        .map_err(|e| GessoError::UnsupportedPlatform(format!("GetModuleHandleW: {e}")))?;

    // 与 webview 壁纸窗同一个类：HTTRANSPARENT 点击穿透白拿
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            WALLPAPER_CLASS,
            PCWSTR::null(),
            WS_POPUP,
            mx,
            my,
            mw,
            mh,
            None,
            None,
            Some(hinstance),
            None,
        )
        .map_err(|e| GessoError::UnsupportedPlatform(format!("CreateWindowExW(视频): {e}")))?
    };

    // 挂载在 swapchain 创建前（窗口层级是 DComp/DXGI 呈现的前提）
    let frame = [mx, my, mw, mh];
    let kind = mount(hwnd, &frame);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNA);
    }
    LIVE_WALLPAPER_HWNDS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(hwnd.0 as isize);
    println!(
        "[pin] 视频壁纸窗 {mx},{my} {mw}×{mh} → {kind:?}（{}）",
        monitor.id
    );

    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let thread = {
        let (stop, paused) = (stop.clone(), paused.clone());
        let url = url.to_string();
        // HWND 原始句柄值跨线程传递（重建于渲染线程内）
        let hwnd_raw = hwnd.0 as isize;
        Some(
            std::thread::Builder::new()
                .name("win-video".into())
                .spawn(move || {
                    video_render_thread(
                        HWND(hwnd_raw as *mut core::ffi::c_void),
                        url,
                        mw,
                        mh,
                        stop,
                        paused,
                    )
                })
                .map_err(|e| GessoError::UnsupportedPlatform(format!("渲染线程: {e}")))?,
        )
    };

    Ok(WinVideoWindow {
        hwnd,
        mount: std::cell::Cell::new(kind),
        stop,
        paused,
        thread,
    })
}

impl WallpaperWindow for WinVideoWindow {
    /// 内容在创建时定死；换壁纸 = 整窗重建（assign 同路径）。
    /// set_fps 热重载的宿主页 URL 在此静默忽略（视频无帧率杠杆）。
    fn load(&mut self, _url: &str) {}

    fn set_paused(&mut self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    fn set_visible(&mut self, visible: bool) {
        // 整窗隐藏 = 露出系统壁纸（本窗之下由 Explorer 绘制桌面）
        unsafe {
            let _ = ShowWindow(self.hwnd, if visible { SW_SHOWNA } else { SW_HIDE });
        }
    }

    fn set_frame(&mut self, (x, y, w, h): (f64, f64, f64, f64)) {
        let frame = [x as i32, y as i32, w as i32, h as i32];
        let kind = mount(self.hwnd, &frame);
        self.mount.set(kind);
    }

    fn reassert_pinning(&mut self, frame: (f64, f64, f64, f64)) {
        // WorkerW 挂载层级由 mount() 保证，重配后重走一次挂载即重申
        self.set_frame(frame);
    }

    /// 挂载是否落位（BottomMost = explorer 未就绪，调用方重试）
    fn mount_ok(&self) -> bool {
        self.mount.get() != MountKind::BottomMost
    }

    fn current_url(&self) -> String {
        "mf://native-video".into()
    }

    fn diag(&self, tag: &str) {
        println!(
            "[diag {tag}] video hwnd={:x} paused={} mount={:?}",
            self.hwnd.0 as isize,
            self.paused.load(Ordering::Relaxed),
            self.mount.get()
        );
    }
}

impl Drop for WinVideoWindow {
    fn drop(&mut self) {
        // 先停渲染线程再销毁窗口（Present 到已销毁窗口的 swapchain 会出错）
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // SAFETY: 窗口由本模块在同线程创建（会话创建/销毁全在引擎轮询线程）
        unsafe {
            if let Err(e) = DestroyWindow(self.hwnd) {
                eprintln!("[pin] DestroyWindow(视频) 失败：{e}");
            }
        }
        LIVE_WALLPAPER_HWNDS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|h| *h != self.hwnd.0 as isize);
        println!("[pin] 视频壁纸窗已销毁（{:x}）", self.hwnd.0 as isize);
    }
}

/// 渲染线程：MF 解码（独占 MTA 套间）→ UpdateSubresource → flip-model Present。
/// D3D11 即时上下文非线程安全——只在生命期内由本线程独占使用；
/// 窗口位置/显隐在主线程（会话层），只动窗口不动 D3D 对象。
fn video_render_thread(
    hwnd: HWND,
    url: String,
    sw: i32,
    sh: i32,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
) {
    unsafe {
        // MF 媒体管线偏好 MTA；D3D11/swapchain 的 Present 都在本线程
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if hr.is_err() {
            eprintln!("[pin-video] CoInitializeEx 失败：0x{:08X}", hr.0);
            return;
        }
        if let Err(e) = MFStartup(MF_VERSION, 0) {
            eprintln!("[pin-video] MFStartup 失败：{e}");
            return;
        }
        let r = video_render_loop(hwnd, &url, sw, sh, &stop, &paused);
        if let Err(e) = r {
            eprintln!("[pin-video] 渲染线程退出：{e} (0x{:08X})", e.code().0);
        }
        let _ = MFShutdown();
    }
}

unsafe fn video_render_loop(
    hwnd: HWND,
    url: &str,
    sw: i32,
    sh: i32,
    stop: &Arc<AtomicBool>,
    paused: &Arc<AtomicBool>,
) -> windows::core::Result<()> {
    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_MODE_DESC {
            Width: sw as u32,
            Height: sh as u32,
            RefreshRate: DXGI_RATIONAL {
                Numerator: 0,
                Denominator: 0,
            },
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
            Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        OutputWindow: hwnd,
        Windowed: true.into(),
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        Flags: 0,
    };
    let mut device = None;
    let mut context = None;
    let mut swapchain = None;
    D3D11CreateDeviceAndSwapChain(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        windows::Win32::Foundation::HMODULE::default(),
        D3D11_CREATE_DEVICE_FLAG(0),
        None,
        D3D11_SDK_VERSION,
        Some(&desc),
        Some(&mut swapchain),
        Some(&mut device),
        None,
        Some(&mut context),
    )?;
    let (device, context, swapchain) = (
        device.unwrap(),
        context.unwrap(),
        swapchain.unwrap().cast::<IDXGISwapChain1>()?,
    );
    let _ = &device;

    MFStartup(MF_VERSION, 0)?;
    let wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    // 缩放到主屏分辨率需要高级视频处理（基础模式只允许解码器直出转换）
    let mut attrs_opt: Option<windows::Win32::Media::MediaFoundation::IMFAttributes> = None;
    windows::Win32::Media::MediaFoundation::MFCreateAttributes(&mut attrs_opt, 1)?;
    let attrs = attrs_opt.unwrap();
    attrs.SetUINT32(
        &windows::Win32::Media::MediaFoundation::MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING,
        1,
    )?;
    let reader = MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), &attrs)?;
    reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
    reader.SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)?;
    let mt = MFCreateMediaType()?;
    mt.SetGUID(
        &MF_MT_MAJOR_TYPE,
        &windows::Win32::Media::MediaFoundation::MFMediaType_Video,
    )?;
    mt.SetGUID(
        &MF_MT_SUBTYPE,
        &windows::Win32::Media::MediaFoundation::MFVideoFormat_ARGB32,
    )?;
    mt.SetUINT64(&MF_MT_FRAME_SIZE, ((sw as u64) << 32) | sh as u64)?;
    reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &mt)?;

    let backbuffer: ID3D11Texture2D = swapchain.GetBuffer(0)?;
    let mut qpf = 0i64;
    let _ = QueryPerformanceFrequency(&mut qpf);
    let mut first_ts: Option<i64> = None;
    let mut first_qpc = 0i64;
    let pv = PROPVARIANT::default();

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        // 暂停 = 冻结当前帧（flip-model 保留最后呈现帧），只挂起解码
        if paused.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let mut flags = 0u32;
        let mut ts = 0i64;
        let mut sample: Option<IMFSample> = None;
        reader.ReadSample(
            MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
            0,
            None,
            Some(&mut flags),
            Some(&mut ts),
            Some(&mut sample),
        )?;
        if (flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 || sample.is_none() {
            // 循环：回到起点并重定基时间戳（OS 页缓存命中，循环零磁盘读）
            reader.SetCurrentPosition(&windows::core::GUID::zeroed(), &pv)?;
            first_ts = None;
            continue;
        }
        let sample = sample.unwrap();
        let buffer = sample.ConvertToContiguousBuffer()?;
        let mut data = std::ptr::null_mut::<u8>();
        let mut max_len = 0u32;
        let mut len = 0u32;
        buffer.Lock(&mut data, Some(&mut max_len), Some(&mut len))?;

        // PTS 节拍：内容时间轴对齐墙钟（QPC），早到才睡，迟到此帧即 present
        let mut qpc = 0i64;
        let _ = QueryPerformanceCounter(&mut qpc);
        if first_ts.is_none() {
            first_ts = Some(ts);
            first_qpc = qpc;
        }
        let due = first_qpc + ((ts - first_ts.unwrap()) * qpf) / 10_000_000;
        if due - qpc > qpf / 1000 {
            let sleep_ms = ((due - qpc) * 1000 / qpf - 1).max(0);
            std::thread::sleep(Duration::from_millis(sleep_ms as u64));
            loop {
                let mut n = 0i64;
                let _ = QueryPerformanceCounter(&mut n);
                if n >= due {
                    break;
                }
            }
        }

        context.UpdateSubresource(&backbuffer, 0, None, data as *const _, sw as u32 * 4, 0);
        let _ = buffer.Unlock();
        swapchain.Present(1, DXGI_PRESENT(0)).ok()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assign_monitor_ids_edid_fallback_and_dedup() {
        let device1 = r"\\.\DISPLAY1".to_string();
        let device2 = r"\\.\DISPLAY2".to_string();
        // 唯一 PnP → edid- 前缀
        assert_eq!(
            assign_monitor_ids(&[(device1.clone(), Some("dela0bc".into()))]),
            vec!["edid-dela0bc".to_string()]
        );
        // 同型号多屏 → 追加设备名消歧
        assert_eq!(
            assign_monitor_ids(&[
                (device1.clone(), Some("dela0bc".into())),
                (device2.clone(), Some("dela0bc".into())),
            ]),
            vec![
                "edid-dela0bc-DISPLAY1".to_string(),
                "edid-dela0bc-DISPLAY2".to_string(),
            ]
        );
        // 无身份（虚拟屏等）→ 回落 win-DISPLAYN
        assert_eq!(
            assign_monitor_ids(&[(device1, None)]),
            vec!["win-DISPLAY1".to_string()]
        );
    }

    #[test]
    fn monitor_pnp_id_parses_both_deviceid_forms() {
        // 注册表形态 MONITOR\<pnp>\<instance>
        assert_eq!(
            monitor_pnp_id_parse("MONITOR\\DELA0BC\\5&2f3acdbc&0&UID8194"),
            Some("dela0bc".to_string())
        );
        // 接口形态 \\?\DISPLAY#<pnp>#<instance>#{guid}
        assert_eq!(
            monitor_pnp_id_parse(
                r"\\?\DISPLAY#DELA0BC#5&2f3acdbc&0&UID8194#{e6f07b5f-97c8-4631-a4cf-07749b5c6c5b}"
            ),
            Some("dela0bc".to_string())
        );
        // 空/异常输入 → None
        assert_eq!(monitor_pnp_id_parse(""), None);
        assert_eq!(monitor_pnp_id_parse("MONITOR\\"), None);
    }

    #[test]
    fn workaround_url_translates_only_prefix() {
        assert_eq!(
            workaround_url("gesso://host/index.html?spec=%7B%7D"),
            "http://gesso.host/index.html?spec=%7B%7D"
        );
        assert_eq!(
            workaround_url("gesso://library/ab12/index.mp4"),
            "http://gesso.library/ab12/index.mp4"
        );
        assert_eq!(
            workaround_url("gesso://steam/workshop/content/431960/7/x.mp4"),
            "http://gesso.steam/workshop/content/431960/7/x.mp4"
        );
        // 非 gesso URL 原样返回
        assert_eq!(workaround_url("about:blank"), "about:blank");
        assert_eq!(
            workaround_url("http://gesso.host/already"),
            "http://gesso.host/already"
        );
    }

    #[test]
    fn unworkaround_roundtrips() {
        let url = "gesso://host/index.html?spec=x";
        assert_eq!(unworkaround_url(&workaround_url(url)), url);
        assert_eq!(unworkaround_url("gesso://host/keep"), "gesso://host/keep");
    }

    #[test]
    fn child_offset_relative_to_parent_origin() {
        // 双屏：副屏在主屏右侧；负坐标（左侧屏）同样成立
        assert_eq!(child_offset((-1920, 0), (0, 0)), (1920, 0));
        assert_eq!(child_offset((-1920, 0), (-1920, 0)), (0, 0));
        assert_eq!(child_offset((0, 0), (2560, -1080)), (2560, -1080));
    }

    #[test]
    fn monitor_id_strips_device_prefix() {
        assert_eq!(monitor_id_from_device(r"\\.\DISPLAY1"), "win-DISPLAY1");
        assert_eq!(monitor_id_from_device(r"\\.\DISPLAY3"), "win-DISPLAY3");
    }
}
