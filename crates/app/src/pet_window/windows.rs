//! Windows 宠物窗口：独立 Win32 分层 overlay，不经 GPUI/SessionManager。
//!
//! 与 macos 模块同一行为契约：
//! - 置顶（HWND_TOPMOST）、非激活（WS_EX_NOACTIVATE）、工具窗（不进任务栏）；
//! - WS_EX_LAYERED + UpdateLayeredWindow 逐像素 alpha——alpha=0 区域由系统直接
//!   放行鼠标（分层窗口文档语义），透明像素逐点穿透天然成立；
//! - 光标不在蜡身命中区（圆角矩形，与 macOS hitTest 同一判定）时整窗加
//!   WS_EX_TRANSPARENT——跨进程穿透唯一可靠路径（pin/windows.rs 实测：窗口
//!   过程返回 HTTRANSPARENT 只对同线程下方窗口生效）；命中区内移除，让点击
//!   落到宠物。150ms 同步节拍判定，与 macOS ignoresMouseEvents 切换同构；
//! - 软件合成上屏：25×50 点阵最近邻逆映射放大（保持像素硬边，等价 macOS
//!   contentsGravity + nearest）+ 状态变换（缩放/旋转/走动颠簸）+ GDI 文本
//!   气泡；视觉键不变则不重绘；
//! - PMv2 纪律：全程物理像素，窗口 DPI 每拍经 GetDpiForWindow 校正，跨屏
//!   拖动自动换算布局与窗口尺寸。
//!
//! 事件与 macos 模块同构：单击延迟确认为戳、300ms 内连点为摸、超过 6pt 为
//! 拖动；全部经 EngineAction 入队，窗口层不直接写存档。

use std::time::{Duration, Instant};

use gesso_core::{
    PetActivity, PetBehavior, PetBehaviorEvent, PetInteraction, PetNotice, PetPosition, PetSave,
    PetSize,
};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GdiFlush,
    SelectObject, SetBkMode, SetTextColor, ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO,
    BITMAPINFOHEADER, BLENDFUNCTION, AC_SRC_ALPHA, AC_SRC_OVER, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_END_ELLIPSIS, DT_SINGLELINE,
    DT_VCENTER, HBITMAP, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DestroyWindow, GetCursorPos, GetWindowLongPtrW, GetWindowRect, HMENU,
    IsWindowVisible, LoadCursorW, PostMessageW, RegisterClassW, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW, TrackPopupMenu,
    UpdateLayeredWindow, WNDCLASSW, GWL_EXSTYLE, GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW,
    MF_BYCOMMAND, MF_POPUP, MF_SEPARATOR, MF_STRING, SPI_GETCLIENTAREAANIMATION, SW_HIDE,
    SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, TPM_BOTTOMALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, ULW_ALPHA, WM_CAPTURECHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE,
    WM_NULL, WM_RBUTTONUP, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    WS_POPUP,
};

use crate::engine::{self, EngineAction};
use crate::pet_template::{BehaviorParams, FrameKind, PetTemplate};
use crate::pet_template_candle;
use crate::pin::MonitorInfo;
use crate::ui::strings;

const DRAG_THRESHOLD: f64 = 6.0; // pt；换算像素见 PetWinState::threshold_px
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(300);

const BODY_W: f64 = 92.0;
const BODY_H: f64 = 100.0;
const MOTION_PADDING: f64 = 24.0;
const BUBBLE_HEADROOM: f64 = 40.0;
const BUBBLE_HEIGHT: f64 = 24.0;

const PET_CLASS: PCWSTR = w!("GessoPetWindow");

// 菜单命令 ID（TrackPopupMenu 直接返回，不经 WM_COMMAND）
const CMD_SETTINGS: i32 = 1;
const CMD_HIDE: i32 = 2;
const CMD_QUIT: i32 = 3;
const CMD_ACTIVITY_QUIET: i32 = 10;
const CMD_ACTIVITY_NORMAL: i32 = 11;
const CMD_ACTIVITY_LIVELY: i32 = 12;
const CMD_SIZE_SMALL: i32 = 20;
const CMD_SIZE_MEDIUM: i32 = 21;
const CMD_SIZE_LARGE: i32 = 22;
const CMD_NOTICE_NEW: i32 = 30;
const CMD_NOTICE_ATTENTION: i32 = 31;
const CMD_NOTICE_WARNING: i32 = 32;

static PET_ENV: std::sync::Once = std::sync::Once::new();

// ---------------------------------------------------------------------------
// 纯逻辑（单测覆盖）：命中/点击分类与几何
// ---------------------------------------------------------------------------

/// 圆角矩形命中（左上原点坐标语义与 macOS 版一致，仅轴方向不同）。
fn rounded_rect_contains(point: (f64, f64), rect: (f64, f64, f64, f64), radius: f64) -> bool {
    let (rx, ry, rw, rh) = rect;
    if point.0 < rx || point.0 > rx + rw || point.1 < ry || point.1 > ry + rh {
        return false;
    }
    let radius = radius.min(rw / 2.0).min(rh / 2.0);
    let left = rx + radius;
    let right = rx + rw - radius;
    let top = ry + radius;
    let bottom = ry + rh - radius;
    if (left..=right).contains(&point.0) || (top..=bottom).contains(&point.1) {
        return true;
    }
    let corner_x = if point.0 < left { left } else { right };
    let corner_y = if point.1 < top { top } else { bottom };
    let dx = point.0 - corner_x;
    let dy = point.1 - corner_y;
    dx * dx + dy * dy <= radius * radius
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClickOutcome {
    Poke,
    Touch,
    Drag,
}

fn classify_click(distance: f64, elapsed_since_last_click: Option<Duration>) -> ClickOutcome {
    if distance > DRAG_THRESHOLD {
        ClickOutcome::Drag
    } else if elapsed_since_last_click.is_some_and(|elapsed| elapsed <= DOUBLE_CLICK_WINDOW) {
        ClickOutcome::Touch
    } else {
        ClickOutcome::Poke
    }
}

fn size_scale(size: PetSize) -> f64 {
    match size {
        PetSize::Small => 0.8,
        PetSize::Medium => 1.0,
        PetSize::Large => 1.25,
    }
}

/// 窗口尺寸（pt）。pt → 物理像素由调用方乘 DPI 系数。
fn window_dimensions(size: PetSize) -> (f64, f64) {
    let scale = size_scale(size);
    (
        BODY_W * scale + MOTION_PADDING,
        BODY_H * scale + MOTION_PADDING + BUBBLE_HEADROOM,
    )
}

/// 默认落点：主屏右下角。Windows 左上原点，y=1 为屏幕底部（macOS y 翻转）。
const DEFAULT_POSITION: (f64, f64) = (0.88, 0.92);

fn window_origin(
    (x, y, w, h): (f64, f64, f64, f64),
    rx: f64,
    ry: f64,
    window_w: f64,
    window_h: f64,
) -> (f64, f64) {
    (
        x + ((w - window_w).max(0.0) * rx),
        y + ((h - window_h).max(0.0) * ry),
    )
}

fn walking_origin(
    base: (f64, f64),
    monitor: (f64, f64, f64, f64),
    window: (f64, f64),
    behavior: PetBehavior,
    params: &BehaviorParams,
) -> (f64, f64) {
    if behavior.state() != gesso_core::PetBehaviorState::Walking {
        return base;
    }
    let progress = (behavior.elapsed().as_secs_f64() / params.walk_seconds).clamp(0.0, 1.0);
    let excursion = (std::f64::consts::PI * progress).sin();
    let left = (base.0 - monitor.0).max(0.0);
    let right = (monitor.0 + monitor.2 - base.0 - window.0).max(0.0);
    let direction = if right >= left { 1.0 } else { -1.0 };
    let room = if direction > 0.0 { right } else { left };
    let distance = room.min(params.walk_excursion);
    (base.0 + direction * distance * excursion, base.1)
}

fn distance_sq(point: (f64, f64), frame: (f64, f64, f64, f64)) -> f64 {
    let x = point.0.clamp(frame.0, frame.0 + frame.2);
    let y = point.1.clamp(frame.1, frame.1 + frame.3);
    (point.0 - x).powi(2) + (point.1 - y).powi(2)
}

/// 窗口矩形 → 显示器相对位置（0..1，钳制到屏内）。
fn position_for_frame(
    frame: (f64, f64, f64, f64),
    monitors: &[MonitorInfo],
    window: (f64, f64),
) -> Option<PetPosition> {
    let center = (frame.0 + frame.2 / 2.0, frame.1 + frame.3 / 2.0);
    let monitor = monitors
        .iter()
        .find(|m| {
            center.0 >= m.frame.0
                && center.0 <= m.frame.0 + m.frame.2
                && center.1 >= m.frame.1
                && center.1 <= m.frame.1 + m.frame.3
        })
        .or_else(|| {
            monitors.iter().min_by(|a, b| {
                distance_sq(center, a.frame).total_cmp(&distance_sq(center, b.frame))
            })
        })?;
    let x = ((frame.0 - monitor.frame.0) / (monitor.frame.2 - window.0).max(1.0)).clamp(0.0, 1.0);
    let y = ((frame.1 - monitor.frame.1) / (monitor.frame.3 - window.1).max(1.0)).clamp(0.0, 1.0);
    Some(PetPosition {
        monitor_id: monitor.id.clone(),
        x: x as f32,
        y: y as f32,
    })
}

fn choose_monitor<'a>(save: &PetSave, monitors: &'a [MonitorInfo]) -> Option<&'a MonitorInfo> {
    save.position
        .as_ref()
        .and_then(|position| monitors.iter().find(|m| m.id == position.monitor_id))
        .or_else(|| monitors.iter().find(|m| m.is_main))
        .or_else(|| monitors.first())
}

/// 系统减少动态效果偏好：SPI_GETCLIENTAREAANIMATION 是 Windows 上最接近
/// macOS accessibilityDisplayShouldReduceMotion 的开关。
fn reduced_motion() -> bool {
    // SAFETY: 只读系统参数查询；pvParams 指向本栈帧 BOOL
    unsafe {
        let mut animations = windows::core::BOOL::default();
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut animations as *mut _ as *mut core::ffi::c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
            && !animations.as_bool()
    }
}

// ---------------------------------------------------------------------------
// 窗口状态（wndproc 与 sync 共享；全部访问都在主线程）
// ---------------------------------------------------------------------------

struct PressState {
    start: (f64, f64),
    start_origin: (f64, f64),
    dragged: bool,
}

struct PetWinState {
    press: Option<PressState>,
    pending_click: Option<Instant>,
    last_mouse_moved: Option<Instant>,
    /// 光标在窗口内的水平比例（0=左，1=右），驱动注视帧。
    gaze_x: f64,
    /// 眨眼时钟基准（窗口创建时刻）。
    blink_clock: Instant,
    /// 唤醒提亮的截止时刻。
    wake_until: Option<Instant>,
    /// 上一帧是否处于睡眠（检测唤醒瞬间）。
    prev_sleeping: bool,
    activity: PetActivity,
    size: PetSize,
    k: f64,
    /// 右键菜单打开中（TrackPopupMenu 模态期）：冻结窗口几何操作——
    /// 每拍的 TOPMOST 重申/位置校正会把菜单抖掉，走动也会把菜单甩在原地。
    menu_open: bool,
    /// 蜡身命中区（窗口内物理像素，左上原点）与半径。
    hit_rect: (f64, f64, f64, f64),
    hit_radius: f64,
}

impl PetWinState {
    fn new(size: PetSize, k: f64) -> Self {
        Self {
            press: None,
            pending_click: None,
            last_mouse_moved: None,
            gaze_x: 0.5,
            blink_clock: Instant::now(),
            wake_until: None,
            prev_sleeping: false,
            activity: PetActivity::Normal,
            size,
            k,
            menu_open: false,
            hit_rect: (0.0, 0.0, 0.0, 0.0),
            hit_radius: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// 窗口过程
// ---------------------------------------------------------------------------

/// 取窗口状态指针指向的堆对象（主线程；创建时登记、销毁前清零）。
unsafe fn state_of(hwnd: HWND) -> Option<&'static mut PetWinState> {
    // SAFETY: 指针由 PetWindow 登记，Drop 前先清 userdata 再回收
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PetWinState };
    (!ptr.is_null()).then(|| unsafe { &mut *ptr })
}

unsafe extern "system" fn pet_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            on_left_down(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            on_mouse_move(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            on_left_up(hwnd);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            on_right_up(hwnd);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            on_capture_changed(hwnd);
            LRESULT(0)
        }
        WM_NULL => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// 光标与窗口几何快照（屏幕物理像素，f64 便于距离运算）。
struct CursorCtx {
    cursor: (f64, f64),
    origin: (f64, f64),
    size: (f64, f64),
}

unsafe fn cursor_and_origin(hwnd: HWND) -> Option<CursorCtx> {
    let mut pt = POINT::default();
    let mut rc = RECT::default();
    // SAFETY: hwnd 属本模块创建的窗口；出参为本栈帧局部
    if unsafe { GetCursorPos(&mut pt) }.is_err() || unsafe { GetWindowRect(hwnd, &mut rc) }.is_err()
    {
        return None;
    }
    Some(CursorCtx {
        cursor: (pt.x as f64, pt.y as f64),
        origin: (rc.left as f64, rc.top as f64),
        size: ((rc.right - rc.left) as f64, (rc.bottom - rc.top) as f64),
    })
}

fn on_left_down(hwnd: HWND) {
    let Some(state) = (unsafe { state_of(hwnd) }) else { return };
    let Some(ctx) = (unsafe { cursor_and_origin(hwnd) }) else {
        return;
    };
    state.press = Some(PressState {
        start: ctx.cursor,
        start_origin: ctx.origin,
        dragged: false,
    });
    // SAFETY: 本窗口捕获；拖动跟随须跨出窗口/透明区域继续收消息
    unsafe {
        let _ = SetCapture(hwnd);
    }
}

fn on_mouse_move(hwnd: HWND) {
    let Some(state) = (unsafe { state_of(hwnd) }) else { return };
    let Some(ctx) = (unsafe { cursor_and_origin(hwnd) }) else {
        return;
    };
    let Some(mut press) = state.press.take() else {
        // 接近观察：注视方向 + 200ms 节流的 MouseMoved（macOS mouseMoved: 同款）
        state.gaze_x =
            ((ctx.cursor.0 - ctx.origin.0) / ctx.size.0.max(1.0)).clamp(0.0, 1.0);
        let now = Instant::now();
        if state
            .last_mouse_moved
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(200))
        {
            return;
        }
        state.last_mouse_moved = Some(now);
        engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::MouseMoved));
        return;
    };
    let dx = ctx.cursor.0 - press.start.0;
    let dy = ctx.cursor.1 - press.start.1;
    // 距离换算回 pt 再分类（阈值常量与 macOS 同源）
    if !press.dragged
        && classify_click((dx * dx + dy * dy).sqrt() / state.k, None) == ClickOutcome::Drag
    {
        press.dragged = true;
        state.pending_click = None;
        engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::DragStarted));
    }
    if press.dragged {
        let (sx, sy) = press.start_origin;
        // SAFETY: 仅更新原点；不动尺寸与层级，避免拖动中丢失 TOPMOST
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                (sx + dx).round() as i32,
                (sy + dy).round() as i32,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
    }
    state.press = Some(press);
}

fn on_left_up(hwnd: HWND) {
    let Some(state) = (unsafe { state_of(hwnd) }) else { return };
    let Some(press) = state.press.take() else {
        // 无按下也就没有捕获；ReleaseCapture 无副作用，直接收尾
        unsafe {
            let _ = ReleaseCapture();
        }
        return;
    };
    // SAFETY: press 已取走——ReleaseCapture 同步派发的 WM_CAPTURECHANGED
    // 在 on_capture_changed 里无事可做，点击分类不会被抢跑
    unsafe {
        let _ = ReleaseCapture();
    }
    if press.dragged {
        engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::DragEnded));
        return;
    }
    let now = Instant::now();
    let elapsed = state.pending_click.map(|last| now.duration_since(last));
    match classify_click(0.0, elapsed) {
        ClickOutcome::Touch => {
            state.pending_click = None;
            engine::enqueue(EngineAction::PetInteraction(PetInteraction::Touch));
            engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::Touch));
        }
        _ => state.pending_click = Some(now),
    }
}

/// 捕获被夺（全屏开始隐藏窗口/系统接管等，鼠标不会再来 WM_LBUTTONUP）：
/// press 不许悬挂——悬挂会永久卡死 sync 的拖拽守卫（位置不再同步）和
/// 穿透切换（命中区外也吞点击）。按拖动与否收尾，位置回报交回下一拍 sync。
/// macOS 无显式信号（mouseUp 丢失时同样悬挂），Windows 靠本消息根治。
fn on_capture_changed(hwnd: HWND) {
    let Some(state) = (unsafe { state_of(hwnd) }) else { return };
    let Some(press) = state.press.take() else { return };
    if press.dragged {
        engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::DragEnded));
    }
}

/// 单击延迟确认：300ms 内没有第二击则判定为戳（由 sync 节拍调用）。
fn flush_pending_click(state: &mut PetWinState) {
    if state
        .pending_click
        .is_some_and(|at| at.elapsed() > DOUBLE_CLICK_WINDOW)
    {
        state.pending_click = None;
        engine::enqueue(EngineAction::PetInteraction(PetInteraction::Poke));
        engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::Poke));
    }
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn on_right_up(hwnd: HWND) {
    let Some(state) = (unsafe { state_of(hwnd) }) else { return };
    if state.press.is_some() {
        return; // 按住左键拖动中不开菜单
    }
    let activity = state.activity;
    let size = state.size;
    let Some(ctx) = (unsafe { cursor_and_origin(hwnd) }) else {
        return;
    };
    state.menu_open = true;
    let cmd = build_and_show_menu(hwnd, activity, size, ctx.cursor);
    if let Some(state) = (unsafe { state_of(hwnd) }) {
        state.menu_open = false;
    }
    dispatch_menu_command(cmd);
}

unsafe fn append_popup(menu: HMENU, sub: HMENU, text: &str) -> bool {
    let wide = to_wide(text);
    // SAFETY: MF_POPUP 的 uIDNewItem 语义为子菜单句柄
    unsafe { AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(wide.as_ptr())) }.is_ok()
}

unsafe fn append_item(menu: HMENU, id: i32, text: &str) -> bool {
    let wide = to_wide(text);
    // SAFETY: 同 append_text；id 为自定义命令值
    unsafe { AppendMenuW(menu, MF_STRING, id as usize, PCWSTR(wide.as_ptr())) }.is_ok()
}

/// 右键菜单（结构对齐 macOS NSMenu 版本）：
/// 设置 / 活跃度(单选) / 尺寸(单选) / 模拟事件 / 统计 / 隐藏 / 退出。
/// 返回 TrackPopupMenu 选中的命令 ID（取消 = 0）。
fn build_and_show_menu(hwnd: HWND, activity: PetActivity, size: PetSize, cursor: (f64, f64)) -> i32 {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return 0;
        };
        let Ok(activity_menu) = CreatePopupMenu() else {
            let _ = DestroyMenu(menu);
            return 0;
        };
        let Ok(sizes) = CreatePopupMenu() else {
            let _ = DestroyMenu(menu);
            let _ = DestroyMenu(activity_menu);
            return 0;
        };
        let Ok(notices) = CreatePopupMenu() else {
            let _ = DestroyMenu(menu);
            let _ = DestroyMenu(activity_menu);
            let _ = DestroyMenu(sizes);
            return 0;
        };

        let mut ok = append_item(menu, CMD_SETTINGS, strings::TRAY_PET_SETTINGS());

        let activity_current = match activity {
            PetActivity::Quiet => CMD_ACTIVITY_QUIET,
            PetActivity::Normal => CMD_ACTIVITY_NORMAL,
            PetActivity::Lively => CMD_ACTIVITY_LIVELY,
        };
        ok &= append_item(activity_menu, CMD_ACTIVITY_QUIET, strings::PET_ACTIVITY_QUIET());
        ok &= append_item(activity_menu, CMD_ACTIVITY_NORMAL, strings::PET_ACTIVITY_NORMAL());
        ok &= append_item(activity_menu, CMD_ACTIVITY_LIVELY, strings::PET_ACTIVITY_LIVELY());
        // SAFETY: 组内单选；ID 连续（MF_BYCOMMAND）
        ok &= CheckMenuRadioItem(
            activity_menu,
            CMD_ACTIVITY_QUIET as u32,
            CMD_ACTIVITY_LIVELY as u32,
            activity_current as u32,
            MF_BYCOMMAND.0,
        )
        .is_ok();
        ok &= append_popup(menu, activity_menu, strings::SET_PET_ACTIVITY());

        let size_current = match size {
            PetSize::Small => CMD_SIZE_SMALL,
            PetSize::Medium => CMD_SIZE_MEDIUM,
            PetSize::Large => CMD_SIZE_LARGE,
        };
        ok &= append_item(sizes, CMD_SIZE_SMALL, strings::PET_SIZE_SMALL());
        ok &= append_item(sizes, CMD_SIZE_MEDIUM, strings::PET_SIZE_MEDIUM());
        ok &= append_item(sizes, CMD_SIZE_LARGE, strings::PET_SIZE_LARGE());
        ok &= CheckMenuRadioItem(
            sizes,
            CMD_SIZE_SMALL as u32,
            CMD_SIZE_LARGE as u32,
            size_current as u32,
            MF_BYCOMMAND.0,
        )
        .is_ok();
        ok &= append_popup(menu, sizes, strings::SET_PET_SIZE());

        ok &= append_item(notices, CMD_NOTICE_NEW, strings::PET_NOTICE_NEW());
        ok &= append_item(notices, CMD_NOTICE_ATTENTION, strings::PET_NOTICE_ATTENTION());
        ok &= append_item(notices, CMD_NOTICE_WARNING, strings::PET_NOTICE_WARNING());
        ok &= append_popup(menu, notices, strings::PET_NOTICE_GROUP());

        // 统计与设置共用入口（macOS 版同款 petSettings: 动作）
        ok &= append_item(menu, CMD_SETTINGS, strings::PET_STATS());
        ok &= append_item(menu, CMD_HIDE, strings::TRAY_PET_HIDE());
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        ok &= append_item(menu, CMD_QUIT, strings::TRAY_QUIT());

        if !ok {
            let _ = DestroyMenu(menu);
            return 0;
        }

        // TrackPopupMenu 的经典收尾纪律：先 SetForegroundWindow 才能点击外部
        // 关闭菜单；结束后补发 WM_NULL 让菜单状态复位。
        // SAFETY: hwnd 属本线程；坐标为屏幕物理像素
        let fg_ok = SetForegroundWindow(hwnd);
        if std::env::var("GESSO_PET_HIT_DIAG").is_ok() {
            eprintln!("[pet] 打开菜单：fg={}", fg_ok.as_bool());
        }
        let cmd = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_BOTTOMALIGN,
            cursor.0.round() as i32,
            cursor.1.round() as i32,
            None,
            hwnd,
            None,
        )
        .0 as i32;
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        cmd
    }
}

fn dispatch_menu_command(cmd: i32) {
    if std::env::var("GESSO_PET_HIT_DIAG").is_ok() {
        eprintln!("[pet] 菜单命令：{cmd}");
    }
    match cmd {
        CMD_SETTINGS => engine::enqueue(EngineAction::OpenPetSettings),
        CMD_HIDE => engine::enqueue(EngineAction::TogglePetVisibility),
        CMD_QUIT => engine::enqueue(EngineAction::Quit),
        CMD_ACTIVITY_QUIET => {
            engine::enqueue(EngineAction::SetPetActivity(PetActivity::Quiet));
        }
        CMD_ACTIVITY_NORMAL => {
            engine::enqueue(EngineAction::SetPetActivity(PetActivity::Normal));
        }
        CMD_ACTIVITY_LIVELY => {
            engine::enqueue(EngineAction::SetPetActivity(PetActivity::Lively));
        }
        CMD_SIZE_SMALL => engine::enqueue(EngineAction::SetPetSize(PetSize::Small)),
        CMD_SIZE_MEDIUM => engine::enqueue(EngineAction::SetPetSize(PetSize::Medium)),
        CMD_SIZE_LARGE => engine::enqueue(EngineAction::SetPetSize(PetSize::Large)),
        CMD_NOTICE_NEW => engine::enqueue(EngineAction::PetBehavior(
            PetBehaviorEvent::Notify(PetNotice::NewResult),
        )),
        CMD_NOTICE_ATTENTION => engine::enqueue(EngineAction::PetBehavior(
            PetBehaviorEvent::Notify(PetNotice::NeedsAttention),
        )),
        CMD_NOTICE_WARNING => engine::enqueue(EngineAction::PetBehavior(
            PetBehaviorEvent::Notify(PetNotice::Warning),
        )),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// 窗口本体：DIB 合成 + UpdateLayeredWindow 上屏
// ---------------------------------------------------------------------------

/// 窗口布局（物理像素）：身体区、点阵、命中区与气泡基线。
struct Layout {
    w: f64,
    h: f64,
    #[allow(dead_code)] // 身体区供后续正式轮廓命中使用
    body: (f64, f64, f64, f64),
    cell: f64,
    sprite: (f64, f64),
    hit: (f64, f64, f64, f64),
    hit_radius: f64,
    bubble_bottom: f64,
}

fn layout(size: PetSize, k: f64, template: &PetTemplate) -> Layout {
    let scale = size_scale(size);
    let (w_pt, h_pt) = window_dimensions(size);
    let w = (w_pt * k).round().max(1.0);
    let h = (h_pt * k).round().max(1.0);
    let body_w = BODY_W * scale * k;
    let body_h = BODY_H * scale * k;
    let pad = MOTION_PADDING / 2.0 * k;
    // 身体贴窗口底部（顶部留给气泡余量）——与 macOS 底部原点版镜像
    let body_x = pad;
    let body_y = h - pad - body_h;
    let cell = (body_w / template.grid_w as f64).min(body_h / template.grid_h as f64);
    let sprite_x = body_x + (body_w - cell * template.grid_w as f64) / 2.0;
    let sprite_y = body_y + (body_h - cell * template.grid_h as f64) / 2.0;
    // hit 行号自顶部起；Windows 左上原点无需翻转
    let (hc, hr, hcw, hrh) = template.params.hit;
    let hit = (
        sprite_x + hc as f64 * cell,
        sprite_y + hr as f64 * cell,
        hcw as f64 * cell,
        hrh as f64 * cell,
    );
    Layout {
        w,
        h,
        body: (body_x, body_y, body_w, body_h),
        cell,
        sprite: (sprite_x, sprite_y),
        hit,
        hit_radius: template.params.hit_radius * scale * k,
        bubble_bottom: body_y - 5.0 * k,
    }
}

/// 视觉键：任何成分变化才重新合成 + 上屏。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VisualKey {
    frame: u8,
    boost: u8,
    scale_q: u16,
    angle_q: i16,
    tx_q: i16,
    ty_q: i16,
    opacity_q: u16,
    bubble: Option<PetNotice>,
    w: i32,
    h: i32,
}

/// 一帧的合成参数（点阵帧/强化档/变换/透明度/气泡）。
struct FrameVisual {
    frame: u8,
    boost: u8,
    scale: f64,
    angle: f64,
    tx: f64,
    ty: f64,
    opacity: f64,
    bubble: Option<PetNotice>,
}

/// 气泡覆盖层：未变换窗口空间里预合成的位图（RGBA 预乘，已含全局透明度）。
/// 采样时与精灵同经状态变换——等价 macOS 气泡挂 root 层继承 transform。
struct BubbleOverlay {
    x: f64,
    y: f64,
    /// 覆盖层像素尺寸（stride），ceil 取整杜绝采样越界。
    w: usize,
    h: usize,
    data: Vec<u8>,
}

struct PetWindow {
    hwnd: HWND,
    state_ptr: *mut PetWinState,
    memdc: HDC,
    stock_bmp: HGDIOBJ,
    dib: HBITMAP,
    dib_bits: *mut u8,
    dib_w: i32,
    dib_h: i32,
    k: f64,
    size: PetSize,
    grid: (usize, usize),
    lay: Layout,
    /// 帧类别 × 3 档火苗强化的点阵 RGBA（非预乘，grid_w*grid_h*4）。
    sprites: Vec<Vec<u8>>,
    last_visual: Option<VisualKey>,
    /// 气泡文本掩码缓存（GDI 光栅化代价一次性付清）。
    bubble_mask: Option<(String, i32, i32, Vec<u8>)>,
    placed: bool,
}

impl PetWindow {
    /// 经 raw 指针取状态（堆上独立对象，非 self 结构体字段）。
    fn state(&mut self) -> &mut PetWinState {
        // SAFETY: 指针由创建路径登记、Drop 前回收；单线程访问
        unsafe { &mut *self.state_ptr }
    }

    fn create(
        template: &PetTemplate,
        appearance: &gesso_core::AppearanceParams,
        size: PetSize,
    ) -> Option<PetWindow> {
        ensure_pet_env();
        // SAFETY: 模块句柄查询无特殊前提
        let hinstance = unsafe { GetModuleHandleW(PCWSTR::null()) }
            .map(|m| HINSTANCE(m.0))
            .ok()?;
        // 初始按系统 DPI 建窗（隐藏态）；首拍 sync 先落位到目标屏再按
        // GetDpiForWindow 校正缩放，混合 DPI 下不会闪错尺寸。
        let k = unsafe { GetDpiForSystem() } as f64 / 96.0;
        // SAFETY: 类已注册；创建为 1×1 隐藏窗，尺寸由 sync/ULW 接管
        let hwnd = match unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT,
                PET_CLASS,
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(hinstance),
                None,
            )
        } {
            Ok(hwnd) => hwnd,
            Err(error) => {
                // 持续失败不至于静默无迹：sync 每拍重试，日志可见即宠物缺失原因
                eprintln!("[pet] CreateWindowExW 失败：{error}");
                return None;
            }
        };
        // SAFETY: Box 进入堆后地址稳定；窗口销毁前由 Drop 取回
        let state_ptr = Box::into_raw(Box::new(PetWinState::new(size, k)));
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        }

        // 精灵预渲染：6 帧类别 × 3 档通知强化（等价 macOS create_window 的
        // CGImage 预渲染，运行时只做数组索引切换）。
        let boost_count = template.notice_palettes.len() + 1;
        let mut sprites = Vec::with_capacity(template.frames.kind_count() * boost_count);
        for frame_idx in 0..template.frames.kind_count() {
            let kind = match frame_idx {
                0 => FrameKind::Idle,
                1 => FrameKind::Flicker,
                2 => FrameKind::Sleep,
                3 => FrameKind::Blink,
                4 => FrameKind::GazeLeft,
                _ => FrameKind::GazeRight,
            };
            let cells = template.compose(kind, appearance.marking, appearance.body);
            for boost in 0..boost_count as u8 {
                let palette = template.palette(appearance.coat_color, appearance.accent, boost);
                let mut rgba = vec![0u8; template.grid_w * template.grid_h * 4];
                template.render_rgba(&cells, &palette, &mut rgba);
                sprites.push(rgba);
            }
        }

        // SAFETY: 与屏幕兼容的内存 DC；DIB 稍后按布局尺寸创建并选入
        let memdc = unsafe { CreateCompatibleDC(None) };
        let mut win = PetWindow {
            hwnd,
            state_ptr,
            memdc,
            stock_bmp: HGDIOBJ::default(),
            dib: HBITMAP::default(),
            dib_bits: std::ptr::null_mut(),
            dib_w: 0,
            dib_h: 0,
            k,
            size,
            grid: (template.grid_w, template.grid_h),
            lay: layout(size, k, template),
            sprites,
            last_visual: None,
            bubble_mask: None,
            placed: false,
        };
        win.ensure_dib(win.lay.w as i32, win.lay.h as i32);
        win.sync_state_layout();
        Some(win)
    }

    /// DPI/尺寸档位变化时重建布局与 DIB；点阵数据不随缩放变化。
    fn rescale(&mut self, k: f64, size: PetSize, template: &PetTemplate) {
        if (self.k - k).abs() < f64::EPSILON && self.size == size {
            return;
        }
        self.k = k;
        self.size = size;
        self.lay = layout(size, k, template);
        self.ensure_dib(self.lay.w as i32, self.lay.h as i32);
        self.sync_state_layout();
        self.last_visual = None;
    }

    fn sync_state_layout(&mut self) {
        let (k, size, hit, radius) = (self.k, self.size, self.lay.hit, self.lay.hit_radius);
        let state = self.state();
        state.k = k;
        state.size = size;
        state.hit_rect = hit;
        state.hit_radius = radius;
    }

    /// 重建窗口大小的 32bpp 顶朝下 DIB（BGRA；预乘 alpha 由写入方保证）。
    fn ensure_dib(&mut self, w: i32, h: i32) {
        if self.dib_w == w && self.dib_h == h && !self.dib_bits.is_null() {
            return;
        }
        let old = if self.dib_bits.is_null() {
            None
        } else {
            Some(self.dib)
        };
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // 顶朝下：首行即窗口顶行
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: bmi 为本栈帧局部；bits 由系统写入
        let dib = unsafe { CreateDIBSection(Some(self.memdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) };
        let Ok(dib) = dib else {
            eprintln!("[pet] CreateDIBSection 失败：{}", windows::core::Error::from_thread());
            return;
        };
        // SAFETY: 新 DIB 选入 memdc，换下旧 DIB 后再删旧（选中态不可删）
        unsafe {
            let prev = SelectObject(self.memdc, HGDIOBJ(dib.0));
            if self.stock_bmp.is_invalid() {
                self.stock_bmp = prev;
            }
            if let Some(old) = old {
                let _ = DeleteObject(HGDIOBJ(old.0));
            }
        }
        self.dib = dib;
        self.dib_bits = bits as *mut u8;
        self.dib_w = w;
        self.dib_h = h;
    }

    /// 光标是否在蜡身命中区（屏幕坐标）。
    fn cursor_on_body(&mut self) -> Option<bool> {
        let (hit, radius) = {
            let state = self.state();
            (state.hit_rect, state.hit_radius)
        };
        let mut pt = POINT::default();
        let mut rc = RECT::default();
        // SAFETY: 只读查询；出参为栈上局部
        unsafe {
            if GetCursorPos(&mut pt).is_err() || GetWindowRect(self.hwnd, &mut rc).is_err() {
                return None;
            }
        }
        let screen_hit = (rc.left as f64 + hit.0, rc.top as f64 + hit.1, hit.2, hit.3);
        let inside = rounded_rect_contains((pt.x as f64, pt.y as f64), screen_hit, radius);
        if std::env::var("GESSO_PET_HIT_DIAG").as_deref() == Ok("1") {
            eprintln!(
                "[pet-hit] point=({:.0},{:.0}) rect=({:.0},{:.0},{:.0},{:.0}) inside={}",
                pt.x, pt.y, screen_hit.0, screen_hit.1, screen_hit.2, screen_hit.3, inside
            );
        }
        Some(inside)
    }

    /// 命中区外整窗 WS_EX_TRANSPARENT（跨进程穿透唯一可靠路径）；
    /// 按住/拖动期间强制可交互，保证捕获链路稳定。
    fn update_hit_through(&mut self) {
        let pressing = self.state().press.is_some();
        let Some(inside) = self.cursor_on_body() else { return };
        let want_transparent = !inside && !pressing;
        // SAFETY: 本进程窗口样式读写；仅翻转 WS_EX_TRANSPARENT 位
        unsafe {
            let current = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let has = (current as u32 & WS_EX_TRANSPARENT.0) != 0;
            if has != want_transparent {
                let next = if want_transparent {
                    current | WS_EX_TRANSPARENT.0 as isize
                } else {
                    current & !(WS_EX_TRANSPARENT.0 as isize)
                };
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, next);
            }
        }
    }

    fn window_origin_now(&self) -> Option<(f64, f64)> {
        let mut rc = RECT::default();
        // SAFETY: 只读查询
        unsafe {
            if GetWindowRect(self.hwnd, &mut rc).is_err() {
                return None;
            }
        }
        Some((rc.left as f64, rc.top as f64))
    }

    /// 合成一帧并按需上屏。全部输入量化进视觉键，不变则零开销。
    fn compose(&mut self, v: FrameVisual) {
        let key = VisualKey {
            frame: v.frame,
            boost: v.boost,
            scale_q: (v.scale * 200.0) as u16,
            angle_q: (v.angle * 100.0) as i16,
            tx_q: v.tx.round() as i16,
            ty_q: v.ty.round() as i16,
            opacity_q: (v.opacity * 100.0) as u16,
            bubble: v.bubble,
            w: self.dib_w,
            h: self.dib_h,
        };
        if self.last_visual == Some(key) {
            return;
        }
        let w = self.dib_w;
        let h = self.dib_h;
        if w <= 0 || h <= 0 || self.dib_bits.is_null() {
            return;
        }
        // SAFETY: DIB 由本窗口独占；切片长度与分配一致
        let buf = unsafe { std::slice::from_raw_parts_mut(self.dib_bits, (w * h * 4) as usize) };
        buf.fill(0);

        // 预合成气泡覆盖层，与精灵同经状态变换采样（macOS 气泡挂 root 层）
        let bubble = self.prepare_bubble(v.bubble, v.opacity);
        self.paint_frame(&v, bubble.as_ref(), buf, w, h);

        // SAFETY: memdc 已选入当前 DIB；ULW_ALPHA 采用 DIB 的预乘 alpha，
        // psize 同步窗口尺寸（src 原点恒为 0,0）
        unsafe {
            let size = SIZE { cx: w, cy: h };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let result = UpdateLayeredWindow(
                self.hwnd,
                None,
                None,
                Some(&size),
                Some(self.memdc),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            if let Err(error) = result {
                eprintln!("[pet] UpdateLayeredWindow 失败：{error}");
            }
        }
        self.last_visual = Some(key);
    }

    /// 单趟逆映射绘制：输出像素 → 逆变换 → 精灵格（最近邻，保持硬边）
    /// + 气泡覆盖（气泡是 root 层子层，与精灵同受缩放/旋转/颠簸变换）。
    fn paint_frame(
        &self,
        v: &FrameVisual,
        bubble: Option<&BubbleOverlay>,
        buf: &mut [u8],
        w: i32,
        h: i32,
    ) {
        let sprite = &self.sprites[v.frame as usize * 3 + v.boost as usize];
        let (grid_w, grid_h) = self.grid;
        let (sp_x, sp_y) = self.lay.sprite;
        let cell = self.lay.cell;
        let (s, c) = v.angle.sin_cos();
        let inv = 1.0 / v.scale;
        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;
        let gw = grid_w as f64;
        let gh = grid_h as f64;
        for py in 0..h {
            let fy = py as f64 + 0.5 - v.ty;
            let dy = fy - cy;
            for px in 0..w {
                let fx = px as f64 + 0.5 - v.tx;
                let dx = fx - cx;
                // 逆变换：先平移后旋转缩放（对应 macOS setAffineTransform）
                let rx = (c * dx + s * dy) * inv;
                let ry = (-s * dx + c * dy) * inv;
                let bx = rx + cx;
                let by = ry + cy;
                // 精灵：sprites 是 RGBA 字节流，格号 ×4 才是字节偏移
                let gx = (bx - sp_x) / cell;
                let gy = (by - sp_y) / cell;
                if gx >= 0.0 && gx < gw && gy >= 0.0 && gy < gh {
                    let o = (gy as usize * grid_w + gx as usize) * 4;
                    if sprite[o + 3] != 0 {
                        let out = (py * w + px) as usize * 4;
                        buf[out] = (sprite[o + 2] as f64 * v.opacity) as u8;
                        buf[out + 1] = (sprite[o + 1] as f64 * v.opacity) as u8;
                        buf[out + 2] = (sprite[o] as f64 * v.opacity) as u8;
                        buf[out + 3] = (sprite[o + 3] as f64 * v.opacity) as u8;
                    }
                }
                // 气泡在最上层，alpha>0 即整像素覆盖（睡眠半透明与气泡按行为机
                // 互斥——重要通知打断睡眠、普通通知睡眠中丢弃，覆盖即精确）
                if let Some(b) = bubble {
                    let lx = bx - b.x;
                    let ly = by - b.y;
                    if lx >= 0.0 && ly >= 0.0 {
                        let (lx, ly) = (lx as usize, ly as usize);
                        if lx < b.w && ly < b.h {
                            let o = (ly * b.w + lx) * 4;
                            if b.data[o + 3] != 0 {
                                let out = (py * w + px) as usize * 4;
                                buf[out] = b.data[o + 2];
                                buf[out + 1] = b.data[o + 1];
                                buf[out + 2] = b.data[o];
                                buf[out + 3] = b.data[o + 3];
                            }
                        }
                    }
                }
            }
        }
    }

    /// 预合成气泡覆盖层：圆角底板 + 描边 + GDI 文本掩码（RGBA 预乘，
    /// 已含全局透明度）。布局常量与 macOS sync_bubble 同式。
    fn prepare_bubble(&mut self, notice: Option<PetNotice>, opacity: f64) -> Option<BubbleOverlay> {
        let kind = notice?;
        let text = match kind {
            PetNotice::NewResult => strings::PET_NOTICE_NEW_TEXT(),
            PetNotice::NeedsAttention => strings::PET_NOTICE_ATTENTION_TEXT(),
            PetNotice::Warning => strings::PET_NOTICE_WARNING_TEXT(),
        };
        let k = self.k;
        let win_w = self.dib_w as f64;
        // 宽度估算与 macOS 版同式（接入正式美术时改用真实文本度量）
        let est: f64 = text
            .chars()
            .map(|ch| if ch.is_ascii() { 7.4 } else { 12.0 })
            .sum::<f64>()
            * k;
        let bubble_w = (est + 24.0 * k).min(win_w - 8.0 * k).max(4.0);
        let bubble_h = BUBBLE_HEIGHT * k;
        let bubble_x = (win_w - bubble_w) / 2.0;
        let bubble_y = self.lay.bubble_bottom - bubble_h;
        let radius = (8.0 * k).min(bubble_w / 2.0).min(bubble_h / 2.0);
        // ceil 取整：覆盖层尺寸与采样上界严格一致，杜绝小数边缘越界
        let bw = bubble_w.ceil().max(1.0) as usize;
        let bh = bubble_h.ceil().max(1.0) as usize;
        let mut data = vec![0u8; bw * bh * 4];

        let bg = (250.0, 250.0, 252.0); // macOS 版 (0.98,0.98,0.99)
        let border = (204.0, 207.0, 217.0); // (0.80,0.81,0.85)
        let inner = (1.0, 1.0, bubble_w - 2.0, bubble_h - 2.0); // 覆盖层局部坐标
        let inner_radius = (radius - 1.0).max(0.0);
        for py in 0..bh {
            for px in 0..bw {
                let (pfx, pfy) = (px as f64 + 0.5, py as f64 + 0.5);
                let (rr, gg, bb) = if !rounded_rect_contains((pfx, pfy), inner, inner_radius) {
                    border
                } else {
                    bg
                };
                let o = (py * bw + px) * 4;
                data[o] = (rr * opacity) as u8;
                data[o + 1] = (gg * opacity) as u8;
                data[o + 2] = (bb * opacity) as u8;
                data[o + 3] = (255.0 * opacity) as u8;
            }
        }

        // 文本区（macOS 版 inset 6/5，高度 -10）；墨色 (0.11,0.11,0.12)，
        // 掩码缓存在窗口上，GDI 光栅化只付一次代价
        let tw = (bubble_w - 12.0 * k).max(1.0) as i32;
        let th = (bubble_h - 10.0 * k).max(1.0) as i32;
        let font_h = (12.0 * k).round().max(8.0) as i32;
        let need_mask = self
            .bubble_mask
            .as_ref()
            .is_none_or(|(t, mw, mh, _)| *t != text || *mw != tw || *mh != th);
        if need_mask {
            self.bubble_mask = text_mask(text, tw, th, font_h)
                .map(|mask| (text.to_string(), tw, th, mask));
        }
        // 掩码缺失时气泡整体退场（失败原因已在 text_mask 内打日志）；
        // 火苗强化档不受影响，通知语义仍可感知
        let (_, _, _, mask) = self.bubble_mask.as_ref()?;
        let ink = (28.0, 28.0, 31.0);
        let tx = (6.0 * k) as i32;
        let ty = (5.0 * k) as i32;
        for row in 0..th {
            for col in 0..tw {
                let a = mask[(row * tw + col) as usize] as f64 / 255.0;
                if a <= 0.0 {
                    continue;
                }
                let dx = tx + col;
                let dy = ty + row;
                if dx < 0 || dx >= bw as i32 || dy < 0 || dy >= bh as i32 {
                    continue;
                }
                let o = (dy as usize * bw + dx as usize) * 4;
                // 底板已是预乘值：out = 底*(1-a) + 墨*op*a（alpha 不变）
                data[o] = (data[o] as f64 * (1.0 - a) + ink.0 * opacity * a) as u8;
                data[o + 1] = (data[o + 1] as f64 * (1.0 - a) + ink.1 * opacity * a) as u8;
                data[o + 2] = (data[o + 2] as f64 * (1.0 - a) + ink.2 * opacity * a) as u8;
            }
        }
        Some(BubbleOverlay {
            x: bubble_x,
            y: bubble_y,
            w: bw,
            h: bh,
            data,
        })
    }
}

impl Drop for PetWindow {
    fn drop(&mut self) {
        // SAFETY: userdata 先清零再销毁窗口，杜绝销毁期 wndproc 悬垂访问
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
            if !self.state_ptr.is_null() {
                drop(Box::from_raw(self.state_ptr));
            }
            if !self.stock_bmp.is_invalid() {
                let _ = SelectObject(self.memdc, self.stock_bmp);
            }
            if !self.dib.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(self.dib.0));
            }
            let _ = DeleteDC(self.memdc);
        }
    }
}

/// GDI 文本 → 灰度覆盖掩码：黑字白底光栅化，alpha = 255 - 亮度。
/// （GDI 不写 alpha 通道；掩码法是分层窗上文本抗锯齿的常规路径。）
fn text_mask(text: &str, w: i32, h: i32, font_h: i32) -> Option<Vec<u8>> {
    unsafe {
        let hdc = CreateCompatibleDC(None);
        if hdc.is_invalid() {
            eprintln!("[pet] 气泡文本 CreateCompatibleDC 失败");
            return None;
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: bmi/bits 为本函数栈上对象（外层 fn 已是 unsafe 块）
        let Ok(dib) = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
            // 失败路径必须回收 DC（GDI 句柄进程级限额，泄漏会拖垮后续绘制）
            eprintln!("[pet] 气泡文本 CreateDIBSection 失败：{}", windows::core::Error::from_thread());
            let _ = DeleteDC(hdc);
            return None;
        };
        std::ptr::write_bytes(bits as *mut u8, 0xFF, (w * h * 4) as usize); // 白底
        let old_bmp = SelectObject(hdc, HGDIOBJ(dib.0));
        let face: Vec<u16> = "Segoe UI".encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: 常规 UI 字形参数；负高度 = 字符高（重量 400 = FW_NORMAL）
        let font: HFONT = CreateFontW(
            -font_h,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            DEFAULT_PITCH.0 as u32,
            PCWSTR(face.as_ptr()),
        );
        let old_font = SelectObject(hdc, HGDIOBJ(font.0));
        SetTextColor(hdc, COLORREF(0)); // 黑字
        SetBkMode(hdc, TRANSPARENT);
        let mut wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
        };
        let _ = DrawTextW(
            hdc,
            &mut wide,
            &mut rc,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        let _ = GdiFlush();
        // 必须先拷出掩码再销毁 GDI 对象——bits 指向 DIB 内存，
        // DeleteObject(dib) 之后即悬垂（实机单测曾在此 ACCESS_VIOLATION）
        let mut mask = vec![0u8; (w * h) as usize];
        for (i, slot) in mask.iter_mut().enumerate() {
            let p = (bits as *const u8).add(i * 4);
            let b = *p as u32;
            let g = *p.add(1) as u32;
            let r = *p.add(2) as u32;
            *slot = (255u32.saturating_sub((r + g + b) / 3)).min(255) as u8;
        }
        let _ = SelectObject(hdc, old_font);
        let _ = SelectObject(hdc, old_bmp);
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteObject(HGDIOBJ(dib.0));
        let _ = DeleteDC(hdc);
        Some(mask)
    }
}

fn ensure_pet_env() {
    PET_ENV.call_once(|| unsafe {
        let Ok(module) = GetModuleHandleW(PCWSTR::null()) else {
            eprintln!("[pet] GetModuleHandleW 失败");
            return;
        };
        let wc = WNDCLASSW {
            style: Default::default(),
            lpfnWndProc: Some(pet_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: HINSTANCE(module.0),
            hIcon: Default::default(),
            // SAFETY: 系统共享光标，无需释放
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: Default::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: PET_CLASS,
        };
        if RegisterClassW(&wc) == 0 {
            eprintln!(
                "[pet] RegisterClassW 失败：{}",
                windows::core::Error::from_thread()
            );
        }
    });
}

// ---------------------------------------------------------------------------
// 反馈合成（对齐 macos::apply_feedback）
// ---------------------------------------------------------------------------

/// 清醒时的摇曳帧选择：reduced-motion 或未到交替点用清醒帧。
fn flicker_kind(behavior: PetBehavior, reduced: bool, template: &PetTemplate) -> FrameKind {
    if reduced {
        return FrameKind::Idle;
    }
    let phase = (behavior.elapsed().as_millis() / template.params.flicker_interval.as_millis()) % 2;
    if phase == 0 {
        FrameKind::Idle
    } else {
        FrameKind::Flicker
    }
}

fn apply_feedback(win: &mut PetWindow, behavior: PetBehavior, template: &PetTemplate) {
    let reduced = reduced_motion();
    let now = Instant::now();
    let sleeping = behavior.state() == gesso_core::PetBehaviorState::Sleeping;

    // reduced-motion 静态替代：脉冲改固定微放大、走动颠簸停用（同 macOS）
    let scale = match behavior.state() {
        gesso_core::PetBehaviorState::Poked => {
            if reduced {
                1.06
            } else {
                let phase = (behavior.elapsed().as_secs_f64() / 0.45).min(1.0);
                1.0 + 0.08 * (std::f64::consts::PI * phase).sin()
            }
        }
        gesso_core::PetBehaviorState::Touched => {
            if reduced {
                1.03
            } else {
                let phase = (behavior.elapsed().as_secs_f64() / 0.65).min(1.0);
                1.0 + 0.04 * (std::f64::consts::PI * phase).sin()
            }
        }
        gesso_core::PetBehaviorState::Dragging => 1.03,
        _ => 1.0,
    };
    let angle = if !reduced && behavior.state() == gesso_core::PetBehaviorState::Touched {
        let phase = (behavior.elapsed().as_secs_f64() / 0.65).min(1.0);
        0.06 * (std::f64::consts::TAU * phase).sin()
    } else {
        0.0
    };
    let k = win.k;
    let (tx, ty) = if reduced {
        (0.0, 0.0)
    } else {
        match behavior.state() {
            gesso_core::PetBehaviorState::Walking => {
                let phase = (behavior.elapsed().as_secs_f64() / 3.0) * std::f64::consts::TAU;
                // y 轴向下：颠簸向上为负
                (phase.sin() * 10.0 * k, -(phase.cos().abs() * 3.0 * k))
            }
            _ => (0.0, 0.0),
        }
    };
    let opacity = if sleeping { 0.72 } else { 1.0 };

    let (awake, gaze, wake_lit) = {
        let state = win.state();
        // 唤醒提亮：从睡眠醒来的 1 秒内火苗保持注意档亮度
        if state.prev_sleeping && !sleeping {
            state.wake_until = Some(now + Duration::from_secs(1));
        }
        state.prev_sleeping = sleeping;
        (
            now.duration_since(state.blink_clock),
            state.gaze_x,
            state.wake_until.is_some_and(|t| now < t),
        )
    };

    // 通知火苗强化档位：0 无 / 1 注意档 / 2 警告档；唤醒提亮取注意档
    let boost = match behavior.notice() {
        Some(PetNotice::Warning) => 2u8,
        Some(_) => 1,
        None if wake_lit => 1,
        None => 0,
    };

    // 帧选择：睡眠 > 眨眼 > 注视（Watching 按光标偏移）> 摇曳
    let kind = if sleeping {
        FrameKind::Sleep
    } else if template.blink_active(awake) {
        FrameKind::Blink
    } else if behavior.state() == gesso_core::PetBehaviorState::Watching {
        if gaze < 0.4 {
            FrameKind::GazeLeft
        } else if gaze > 0.6 {
            FrameKind::GazeRight
        } else {
            flicker_kind(behavior, reduced, template)
        }
    } else {
        flicker_kind(behavior, reduced, template)
    };

    win.compose(FrameVisual {
        frame: kind as u8,
        boost,
        scale,
        angle,
        tx,
        ty,
        opacity,
        bubble: behavior.notice(),
    });
}

// ---------------------------------------------------------------------------
// 管理器（GPUI Global；主循环 150ms 驱动）
// ---------------------------------------------------------------------------

pub struct PetWindowManager {
    template: &'static PetTemplate,
    window: Option<PetWindow>,
    pet_id: Option<String>,
    pet_size: Option<PetSize>,
    last_reported_position: Option<(String, f32, f32)>,
    last_applied_origin: Option<(f64, f64)>,
}

impl Default for PetWindowManager {
    fn default() -> Self {
        Self {
            template: pet_template_candle::candle(),
            window: None,
            pet_id: None,
            pet_size: None,
            last_reported_position: None,
            last_applied_origin: None,
        }
    }
}

impl gpui_kit::Global for PetWindowManager {}

impl PetWindowManager {
    pub fn sync(
        &mut self,
        save: &PetSave,
        monitors: &[MonitorInfo],
        behavior: PetBehavior,
    ) -> Option<PetPosition> {
        let Some(pet) = save.adoption.as_ref() else {
            self.hide();
            return None;
        };
        if !save.visible {
            self.hide();
            return None;
        }
        let Some(monitor) = choose_monitor(save, monitors) else {
            self.hide();
            return None;
        };

        // 身份/尺寸档位变化 → 整窗重建（贴 macOS 语义）。创建失败不记录身份，
        // 下一拍重试（类注册失败等瞬态场景宠物不至于永久消失）。
        if self.pet_id.as_deref() != Some(&pet.pet_id) || self.pet_size != Some(save.size) {
            self.window = None;
            self.pet_id = None;
            self.pet_size = None;
            self.last_reported_position = None;
            self.last_applied_origin = None;
            let win = PetWindow::create(self.template, &pet.appearance, save.size)?;
            self.pet_id = Some(pet.pet_id.clone());
            self.pet_size = Some(save.size);
            self.window = Some(win);
        }
        let win = self.window.as_mut()?;

        // 混合 DPI 修正：先落位到目标屏再读窗口 DPI，避免首拍按主屏缩放
        if !win.placed {
            let cx = (monitor.frame.0 + monitor.frame.2 / 2.0) as i32;
            let cy = (monitor.frame.1 + monitor.frame.3 / 2.0) as i32;
            // SAFETY: 只挪位置；尺寸/层级由后续流程接管
            unsafe {
                let _ = SetWindowPos(
                    win.hwnd,
                    None,
                    cx,
                    cy,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
            win.placed = true;
        }
        // SAFETY: GetDpiForWindow 只读查询
        let dpi = unsafe { GetDpiForWindow(win.hwnd) };
        win.rescale(dpi as f64 / 96.0, save.size, self.template);
        win.state().activity = save.activity;

        apply_feedback(win, behavior, self.template);
        flush_pending_click(win.state());
        if !win.state().menu_open {
            win.update_hit_through();
        }

        // 拖动期间禁止同步旧存档位置（否则拖放会在手指位置与旧目标间抢写）；
        // 菜单打开期间同样冻结——宿主窗口的任何几何操作都会拆掉模态菜单。
        if win.state().press.is_some() || win.state().menu_open {
            return None;
        }

        // 位置推导：基准（pt 布局 × DPI → 物理像素）+ 走动偏移
        let k = win.k;
        let (ww_pt, wh_pt) = window_dimensions(save.size);
        let ww = ww_pt * k;
        let wh = wh_pt * k;
        let (rx, ry) = save
            .position
            .as_ref()
            .filter(|position| position.monitor_id == monitor.id)
            .map(|position| {
                (
                    position.x.clamp(0.0, 1.0) as f64,
                    position.y.clamp(0.0, 1.0) as f64,
                )
            })
            .unwrap_or(DEFAULT_POSITION);
        let base = window_origin(monitor.frame, rx, ry, ww, wh);
        let mut params = self.template.params.clone();
        params.walk_excursion *= k; // 散步距离 pt → 物理像素
        let (px, py) = if reduced_motion() {
            base
        } else {
            walking_origin(base, monitor.frame, (ww, wh), behavior, &params)
        };

        let tol = 2.0 * k;
        let actual = win.window_origin_now()?;
        let at_target = (actual.0 - px).abs() <= tol && (actual.1 - py).abs() <= tol;
        if !at_target {
            let target_changed = self
                .last_applied_origin
                .is_some_and(|applied| (applied.0 - px).abs() > tol || (applied.1 - py).abs() > tol);
            if !target_changed {
                if let Some(applied) = self.last_applied_origin {
                    let moved = (actual.0 - applied.0).abs() > tol
                        || (actual.1 - applied.1).abs() > tol;
                    if moved {
                        // 用户拖放后的窗口位置回报一次；等引擎写回存档再施加目标
                        if let Some(position) =
                            position_for_frame((actual.0, actual.1, ww, wh), monitors, (ww, wh))
                        {
                            let key = (position.monitor_id.clone(), position.x, position.y);
                            if self.last_reported_position.as_ref() != Some(&key) {
                                self.last_reported_position = Some(key);
                                return Some(position);
                            }
                        }
                        return None;
                    }
                }
            }
            // SAFETY: 仅校正原点；窗口尺寸由 ULW 管理
            unsafe {
                let _ = SetWindowPos(
                    win.hwnd,
                    None,
                    px.round() as i32,
                    py.round() as i32,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }
        self.last_reported_position = None;
        self.last_applied_origin = Some((px, py));

        // 置顶重申 + 可见性恢复（对齐 setLevel + orderFrontRegardless）
        // SAFETY: TOPMOST 幂等；SW_SHOWNA 不抢焦点
        unsafe {
            if !IsWindowVisible(win.hwnd).as_bool() {
                let _ = ShowWindow(win.hwnd, SW_SHOWNA);
            }
            let _ = SetWindowPos(
                win.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        None
    }

    fn hide(&self) {
        if let Some(win) = &self.window {
            // SAFETY: 本进程窗口
            unsafe {
                let _ = ShowWindow(win.hwnd, SW_HIDE);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 单测（纯逻辑；Win32 路径靠实机验证）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        classify_click, position_for_frame, size_scale, walking_origin, window_dimensions,
        window_origin, ClickOutcome, DEFAULT_POSITION, DOUBLE_CLICK_WINDOW, DRAG_THRESHOLD,
    };
    use crate::pin::MonitorInfo;
    use crate::pet_template::BehaviorParams;
    use gesso_core::{PetBehavior, PetBehaviorEvent, PetSize};
    use std::time::Duration;

    /// 气泡文本掩码：GDI 光栅化必须产出「白底 + 字形笔画」双值掩码。
    /// （app 测试只在开发机跑，GDI 必可用；CI Windows job 不跑 app 测试。）
    #[test]
    fn text_mask_rasterizes_glyphs() {
        let mask = super::text_mask("新结果 Done", 140, 24, 16)
            .expect("GDI 文本光栅化不应失败");
        let ink = mask.iter().filter(|&&v| v > 128).count();
        let bg = mask.iter().filter(|&&v| v == 0).count();
        assert!(ink > 30, "应有可观的字形笔画像素，实际 {ink}");
        assert!(
            bg > mask.len() / 2,
            "白底（alpha=0）应占多数，实际 {bg}/{}",
            mask.len()
        );
    }

    #[test]
    fn click_classifier_distinguishes_poke_touch_and_drag() {
        assert_eq!(classify_click(0.0, None), ClickOutcome::Poke);
        assert_eq!(
            classify_click(0.0, Some(DOUBLE_CLICK_WINDOW)),
            ClickOutcome::Touch
        );
        assert_eq!(
            classify_click(0.0, Some(DOUBLE_CLICK_WINDOW + Duration::from_millis(1))),
            ClickOutcome::Poke
        );
        assert_eq!(classify_click(DRAG_THRESHOLD, None), ClickOutcome::Poke);
        assert_eq!(
            classify_click(DRAG_THRESHOLD + 0.001, None),
            ClickOutcome::Drag
        );
    }

    #[test]
    fn pet_size_maps_to_window_dimensions() {
        assert_eq!(size_scale(PetSize::Small), 0.8);
        assert_close(window_dimensions(PetSize::Small), (97.6, 144.0));
        assert_close(window_dimensions(PetSize::Medium), (116.0, 164.0));
        assert_close(window_dimensions(PetSize::Large), (139.0, 189.0));
    }

    #[test]
    fn default_position_lands_bottom_right_in_top_left_coords() {
        let (w, h) = window_dimensions(PetSize::Medium);
        // 左上原点：y=0.92 → 窗口贴近屏幕底部
        let (x, y) = window_origin((0.0, 0.0, 1440.0, 900.0), DEFAULT_POSITION.0, DEFAULT_POSITION.1, w, h);
        assert!((x - 1165.12).abs() < 1e-9);
        assert!((y - 677.12).abs() < 1e-9);
        assert!(y > 900.0 - h - 200.0);
    }

    #[test]
    fn pet_window_origin_stays_inside_monitor_when_clamped() {
        let (w, h) = window_dimensions(PetSize::Medium);
        assert_close(
            window_origin((0.0, 0.0, 1440.0, 900.0), 0.88, 0.92, w, h),
            (1165.12, 677.12),
        );
        // 窗口大于屏（rx/ry=1）→ 锚定显示器原点
        assert_close(
            window_origin((10.0, 20.0, 100.0, 100.0), 1.0, 1.0, w, h),
            (10.0, 20.0),
        );
    }

    #[test]
    fn walking_origin_moves_without_leaving_monitor() {
        let params = BehaviorParams {
            flicker_interval: Duration::from_millis(550),
            blink_interval: Duration::from_millis(4000),
            blink_duration: Duration::from_millis(180),
            walk_excursion: 56.0,
            walk_seconds: 3.0,
            hit: (2, 17, 21, 25),
            hit_radius: 22.0,
        };
        let monitor = (0.0, 0.0, 1440.0, 900.0);
        let window = (116.0, 164.0);
        // 右下角基点：左侧空间大 → 走动向左
        let base = (1165.0, 736.0);
        let mut behavior = PetBehavior::new(gesso_core::PetActivity::Normal);
        behavior.handle(PetBehaviorEvent::StartWalking);
        behavior.tick(Duration::from_millis(1500));
        let middle = walking_origin(base, monitor, window, behavior, &params);
        assert!(middle.0 < base.0);
        assert!(middle.0 >= monitor.0);
        assert!(middle.0 + window.0 <= monitor.0 + monitor.2);
        // 非走动状态保持基点
        assert_eq!(
            walking_origin(
                base,
                monitor,
                window,
                PetBehavior::new(gesso_core::PetActivity::Normal),
                &params,
            ),
            base
        );
    }

    #[test]
    fn position_is_clamped_to_monitor_bounds() {
        let monitors = [MonitorInfo {
            id: "main".into(),
            name: "Main".into(),
            frame: (0.0, 0.0, 1440.0, 900.0),
            is_main: true,
        }];
        // 拖出左下角 → 钳制进屏内（左上原点：底部对应 y=1）
        let position =
            position_for_frame((-100.0, 850.0, 116.0, 164.0), &monitors, (116.0, 164.0)).unwrap();
        assert_eq!(position.monitor_id, "main");
        assert_eq!(position.x, 0.0);
        assert_eq!(position.y, 1.0);
    }

    fn assert_close(actual: (f64, f64), expected: (f64, f64)) {
        assert!((actual.0 - expected.0).abs() < 1e-9);
        assert!((actual.1 - expected.1).abs() < 1e-9);
    }
}
