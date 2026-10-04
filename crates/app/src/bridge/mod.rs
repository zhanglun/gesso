//! 系统数据桥（M5）：只读系统真值，不含策略。
//!
//! 策略解析（PausePolicy → 暂停/降帧/忽略）在 `session::suspend_effect`
//! （纯函数可单测）；本模块只回答三件事：哪些显示器正被全屏应用覆盖、
//! 现在是不是电池供电、光标此刻在哪/哪个键按下/空闲多久。
//! 光标走 CoreGraphics 纯 C 轮询（位置/按键瞬时态/空闲时长均无需 Input
//! Monitoring 权限——只有事件 tap 才要），频率与合并由调用方控制。

#[cfg(target_os = "macos")]
pub mod macos;

/// 一次桥采样。
pub struct BridgeSnapshot {
    /// 正被全屏应用覆盖的显示器 ID 集合。
    pub fullscreen: std::collections::BTreeSet<String>,
    /// 电源态：Some(true)=电池供电、Some(false)=接通电源、None=无电池（台式机）。
    pub on_battery: Option<bool>,
}

/// 光标瞬时态：全局 AppKit 坐标（原点左下，单位 pt）。
#[derive(Debug, Clone, Copy)]
pub struct MouseSample {
    pub x: f64,
    pub y: f64,
    /// bit0=左键 bit1=右键 bit2=中键
    pub buttons: u8,
    /// 距上次鼠标活动的秒数
    pub idle_secs: f64,
}

/// 只采样光标（轻量，无全屏窗口列表拷贝；供高频光标轮询）。
#[cfg(target_os = "macos")]
pub fn sample_mouse() -> MouseSample {
    let (x, y) = macos::mouse_location();
    let (buttons, idle_secs) = macos::mouse_buttons_idle();
    MouseSample { x, y, buttons, idle_secs }
}

pub fn sample() -> BridgeSnapshot {
    #[cfg(target_os = "macos")]
    {
        BridgeSnapshot {
            fullscreen: macos::fullscreen_displays(),
            on_battery: macos::on_battery(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        BridgeSnapshot {
            fullscreen: Default::default(),
            on_battery: None,
        }
    }
}
