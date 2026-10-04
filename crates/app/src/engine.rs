//! 引擎接线层（API.md §4 —— UI 与引擎之间唯一通道）。
//!
//! - 读：引擎持有 `AppState`（含 SessionManager），UI 经快照（main.rs `snapshot_ui`）
//!   拿渲染数据；UI 不直接触碰 pin/protocol/PinSession。
//! - 写：UI 只把 `EngineAction` 入队；引擎 150ms 轮询执行（与托盘同一通道）。
//!   禁止 UI 直接创建/销毁壁纸窗口——窗口生命周期由 `sync_monitors` 独占。

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use gesso_core::Settings;
use gpui_kit::Global;

use crate::session::SessionManager;

/// 引擎全局（API.md §4：cx.set_global(AppState::new(sm))）。
pub struct AppState {
    pub sm: SessionManager,
    /// 缩略图抽帧调度簿记（主线程独占，见 `ThumbScheduler`）。
    pub thumbs: ThumbScheduler,
}

impl AppState {
    pub fn new(sm: SessionManager) -> Self {
        AppState {
            sm,
            thumbs: ThumbScheduler::default(),
        }
    }
}

impl Global for AppState {}

/// 抽帧调度簿记：在途去重 + 每目录重试上限（主线程独占，无锁）。
///
/// 背景：v1 对缺帧条目每 30s 无限重试——持续失败的条目（坏文件/磁盘满/不支持的
/// 编码）变成无止境的 FFI 空转和崩溃放大器。有上限后自然退化为静态首帧/渐变
/// 占位。会话级计数（不持久化）：重启视为新的机会——偶发失败可重试，持续失败不打扰。
#[derive(Default)]
pub struct ThumbScheduler {
    in_flight: HashSet<String>,
    attempts: HashMap<String, u8>,
}

impl ThumbScheduler {
    /// 每会话每条目最大抽帧次数。
    pub const MAX_ATTEMPTS: u8 = 3;

    /// 该目录当前是否允许起抽帧任务（未在途且未耗尽重试次数）。
    pub fn should_start(&self, dir: &str) -> bool {
        !self.in_flight.contains(dir)
            && self.attempts.get(dir).copied().unwrap_or(0) < Self::MAX_ATTEMPTS
    }

    pub fn mark_started(&mut self, dir: &str) {
        self.in_flight.insert(dir.to_string());
    }

    pub fn mark_finished(&mut self, dir: &str) {
        self.in_flight.remove(dir);
        *self.attempts.entry(dir.to_string()).or_insert(0) += 1;
    }
}

/// UI → 引擎的写动作。
#[derive(Debug, Clone, PartialEq)]
pub enum EngineAction {
    /// 指派/换壁纸（SessionManager::assign，只重建该显示器会话）。
    Assign {
        monitor_id: String,
        entry_id: String,
    },
    /// 暂停/恢复全部（SessionManager::pause_all，托盘）。
    PauseAll(bool),
    /// 单显示器暂停/恢复（SessionManager::pause_one，§4.4 屏卡片）。
    PauseOne { monitor_id: String, paused: bool },
    /// 全量同步显示器（SessionManager::sync_monitors，幂等）。
    SyncMonitors,
    /// 主显示器循环换下一张（SessionManager::cycle_main）。
    CycleMain,
    /// 导入文件（SessionManager::import_entry：校验/拷贝/清单落盘）。
    Import { path: String },
    /// 设置更新（SessionManager::update_settings，写内存 + 落盘）。
    UpdateSettings(Settings),
    /// 开机自启开关（auto-launch，随设置页/托盘勾选项）。
    SetAutostart(bool),
    /// 从库移除条目（SessionManager::remove_entry，不删文件）。
    Remove { entry_id: String },
    /// 单显示器帧率上限（SessionManager::set_fps，§4.4）。
    SetMonitorFps { monitor_id: String, fps: u8 },
    /// 托盘「管理窗口…」：激活主窗口。
    FocusMainWindow,
    /// 后台抽帧任务完成（thumb 调度 → 引擎：释放在途标记 + 触发快照回灌）。
    ThumbsDone { dir: String },
}

static ENGINE_ACTIONS: Mutex<Vec<EngineAction>> = Mutex::new(Vec::new());

/// UI 线程调用：入队一个写动作，立即返回（不做乐观等待）。
pub fn enqueue(action: EngineAction) {
    ENGINE_ACTIONS.lock().expect("引擎动作队列").push(action);
}

/// 引擎轮询线程调用：取走全部待执行动作。
pub fn drain() -> Vec<EngineAction> {
    std::mem::take(&mut *ENGINE_ACTIONS.lock().expect("引擎动作队列"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_dedups_in_flight() {
        let mut s = ThumbScheduler::default();
        assert!(s.should_start("dir-a"));
        s.mark_started("dir-a");
        assert!(!s.should_start("dir-a"), "在途任务不得重复起");
        assert!(s.should_start("dir-b"), "不影响其他条目");

        s.mark_finished("dir-a");
        assert!(s.should_start("dir-a"), "完成后可再次起任务");
        assert_eq!(s.attempts.get("dir-a"), Some(&1));
    }

    #[test]
    fn scheduler_caps_retry_attempts() {
        let mut s = ThumbScheduler::default();
        for _ in 0..ThumbScheduler::MAX_ATTEMPTS {
            assert!(s.should_start("bad"));
            s.mark_started("bad");
            s.mark_finished("bad");
        }
        assert!(
            !s.should_start("bad"),
            "重试耗尽后本会话不再对同一目录起任务"
        );
    }
}
