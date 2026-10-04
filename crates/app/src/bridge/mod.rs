//! 系统数据桥（M5）：只读系统真值，不含策略。
//!
//! 策略解析（PausePolicy → 暂停/降帧/忽略）在 `session::suspend_effect`
//! （纯函数可单测）；本模块只回答两个问题：哪些显示器正被全屏应用覆盖、
//! 现在是不是电池供电。光标/空闲检测需要辅助功能权限（Input Monitoring），
//! 留待独立待办（见 ROADMAP M5）。

#[cfg(target_os = "macos")]
pub mod macos;

/// 一次桥采样。
pub struct BridgeSnapshot {
    /// 正被全屏应用覆盖的显示器 ID 集合。
    pub fullscreen: std::collections::BTreeSet<String>,
    /// 电源态：Some(true)=电池供电、Some(false)=接通电源、None=无电池（台式机）。
    pub on_battery: Option<bool>,
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
