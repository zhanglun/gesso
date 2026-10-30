//! 引擎接线层（API.md §4 —— UI 与引擎之间唯一通道）。
//!
//! - 读：引擎持有 `AppState`（含 SessionManager），UI 经快照（main.rs `snapshot_ui`）
//!   拿渲染数据；UI 不直接触碰 pin/protocol/PinSession。
//! - 写：UI 只把 `EngineAction` 入队；引擎 150ms 轮询执行（与托盘同一通道）。
//!   禁止 UI 直接创建/销毁壁纸窗口——窗口生命周期由 `sync_monitors` 独占。

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gesso_core::{
    AdoptionEvent, AdoptionState, PetActivity, PetBehavior, PetBehaviorEvent, PetInteraction,
    PetPosition, PetSave, PetSize, PetSpecies, PetStats, Settings,
};
use gpui_kit::Global;

use crate::session::SessionManager;

/// 引擎全局（API.md §4：cx.set_global(AppState::new(sm))）。
pub struct AppState {
    pub sm: SessionManager,
    pub pet_save: PetSave,
    pub pet_adoption: AdoptionState,
    pub pet_store: crate::pet_store::PetStore,
    /// 启动读取失败时禁止用默认值覆盖原存档。
    pub pet_storage_blocked: bool,
    /// 最近一次领养存档错误，仅供领养窗口显示。
    pub pet_error: Option<String>,
    /// 互动统计已变更，等待主循环节流写盘。
    pub pet_stats_dirty: bool,
    /// 全屏应用出现时隐藏宠物窗口，但不改写用户的 visible 偏好。
    pub pet_fullscreen: bool,
    /// 当前会话是否锁屏；锁屏期间不累计陪伴时间。
    pub pet_locked: bool,
    /// 陪伴时间的单调时钟与不足一秒的余数。
    pub pet_companion_clock: PetCompanionClock,
    /// 互动反馈状态机；渲染层只消费当前状态。
    pub pet_behavior: PetBehavior,
    /// 缩略图抽帧调度簿记（主线程独占，见 `ThumbScheduler`）。
    pub thumbs: ThumbScheduler,
}

impl AppState {
    pub fn new(
        sm: SessionManager,
        pet_store: crate::pet_store::PetStore,
        pet_save: PetSave,
        pet_storage_blocked: bool,
    ) -> Self {
        let pet_behavior = PetBehavior::new(pet_save.activity);
        let pet_adoption = if pet_save.adoption.is_some() {
            AdoptionState::Adopted
        } else {
            AdoptionState::NotAdopted
        };
        AppState {
            sm,
            pet_save,
            pet_adoption,
            pet_store,
            pet_storage_blocked,
            pet_error: None,
            pet_stats_dirty: false,
            pet_fullscreen: false,
            pet_locked: false,
            pet_companion_clock: PetCompanionClock::default(),
            pet_behavior,
            thumbs: ThumbScheduler::default(),
        }
    }
}

impl Global for AppState {}

#[derive(Debug, Default)]
pub struct PetCompanionClock {
    last_at: Option<Instant>,
    remainder: Duration,
}

impl PetCompanionClock {
    /// 推进陪伴时间；inactive 时只重置时间基准，不补算暂停期间。
    pub fn advance(&mut self, now: Instant, active: bool, stats: &mut PetStats) -> bool {
        let elapsed = self
            .last_at
            .replace(now)
            .map_or(Duration::ZERO, |last| now.saturating_duration_since(last));
        if !active {
            self.remainder = Duration::ZERO;
            return false;
        }

        self.remainder = self.remainder.saturating_add(elapsed);
        let seconds = self.remainder.as_secs();
        if seconds == 0 {
            return false;
        }
        stats.companion_seconds = stats.companion_seconds.saturating_add(seconds);
        self.remainder -= Duration::from_secs(seconds);
        true
    }
}

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

    /// UI 投影：该条目缩略图是否"生成中/仍会生成"（在途或重试未耗尽）。
    /// 驱动库卡片 spinner；重试耗尽 → false，退回静态占位不假装在加载。
    pub fn pending(&self, dir: &str) -> bool {
        self.in_flight.contains(dir)
            || self.attempts.get(dir).copied().unwrap_or(0) < Self::MAX_ATTEMPTS
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
    /// 导入远端网页（SessionManager::import_url_entry；§4.3 工具条 🔗）。
    ImportUrl { url: String },
    /// 设置更新（SessionManager::update_settings，写内存 + 落盘）。
    UpdateSettings(Settings),
    /// 开机自启开关（auto-launch，随设置页/托盘勾选项）。
    SetAutostart(bool),
    /// 从库移除条目（SessionManager::remove_entry；local 拷贝目录入回收站，WE/url 只断引用）。
    Remove { entry_id: String },
    /// 单显示器帧率上限（SessionManager::set_fps，§4.4）。
    SetMonitorFps { monitor_id: String, fps: u8 },
    /// 单显示器填充模式（SessionManager::set_fit，§4.4 填充下拉，2026-10-12）。
    SetMonitorFit {
        monitor_id: String,
        fit: gesso_core::Fit,
    },
    /// 检查应用更新（§4.7；update 模块后台线程执行，open_modal = 兑现时开小窗）。
    UpdateCheck { open_modal: bool },
    /// 下载可用更新的安装包（仅 Available 态受理；进度经状态代数回灌）。
    UpdateDownload,
    /// 取消进行中的下载（回到 Available 态并清理 .part）。
    UpdateCancelDownload,
    /// 在系统浏览器中打开本地宠物控制台。
    OpenWebControl,
    /// 打开宠物领养窗口。
    OpenPetAdoption,
    /// 打开管理窗口的宠物页。
    OpenPetSettings,
    /// 创建一轮临时候选。
    StartPetAdoption { species: PetSpecies },
    /// 领养 UI 的选择、刷新和确认。
    PetAdoption(AdoptionEvent),
    /// 保存宠物窗口拖动后的显示器归属和屏内位置。
    SavePetPosition(PetPosition),
    /// 宠物窗口的戳/摸互动事件。
    PetInteraction(PetInteraction),
    /// 宠物窗口行为反馈事件。
    PetBehavior(PetBehaviorEvent),
    /// 更新宠物活动档位并持久化。
    SetPetActivity(PetActivity),
    /// 更新宠物尺寸并持久化。
    SetPetSize(PetSize),
    /// 切换宠物用户可见状态并持久化。
    TogglePetVisibility,
    /// 幂等设置可见状态（Web 写操作用；避免 TOCTOU 读改写）。
    SetPetVisibility(bool),
    /// 设置页高级区域：重置宠物数据后重新领养。
    ResetPetData,
    /// 托盘「管理窗口…」：激活主窗口。
    FocusMainWindow,
    /// 托盘「恢复系统壁纸」：清全部指派 + 拆全部壁纸窗——运行时不接管
    /// 系统桌面，桌面即回到用户自己的壁纸；重新指派即可再上壁纸。
    RestoreSystemWallpaper,
    /// 后台抽帧任务完成（thumb 调度 → 引擎：释放在途标记 + 触发快照回灌）。
    ThumbsDone { dir: String },
    /// 托盘「退出 Gesso」：引擎线程上拆除全部壁纸会话 → 通知 shell 重绘 → 进程退出。
    /// 不能像旧版那样在托盘线程 process::exit——那会跳过壁纸窗口销毁，桌面留黑底。
    Quit,
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
    fn companion_clock_counts_active_seconds_without_counting_pauses() {
        let start = Instant::now();
        let mut clock = PetCompanionClock::default();
        let mut stats = PetStats::default();

        assert!(!clock.advance(start, true, &mut stats));
        assert!(clock.advance(start + Duration::from_secs(2), true, &mut stats));
        assert_eq!(stats.companion_seconds, 2);
        assert!(!clock.advance(start + Duration::from_secs(10), false, &mut stats));
        assert_eq!(stats.companion_seconds, 2);
        assert!(!clock.advance(start + Duration::from_millis(10_500), true, &mut stats));
        assert!(clock.advance(start + Duration::from_millis(11_500), true, &mut stats));
        assert_eq!(stats.companion_seconds, 3);
    }

    #[test]
    fn companion_clock_saturates_at_u64_max() {
        let start = Instant::now();
        let mut clock = PetCompanionClock::default();
        let mut stats = PetStats {
            companion_seconds: u64::MAX,
            ..PetStats::default()
        };
        clock.advance(start, true, &mut stats);
        clock.advance(start + Duration::from_secs(1), true, &mut stats);
        assert_eq!(stats.companion_seconds, u64::MAX);
    }

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
