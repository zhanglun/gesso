//! 引擎接线层（API.md §4 —— UI 与引擎之间唯一通道）。
//!
//! - 读：引擎持有 `AppState`（含 SessionManager），UI 经快照（main.rs `snapshot_ui`）
//!   拿渲染数据；UI 不直接触碰 pin/protocol/PinSession。
//! - 写：UI 只把 `EngineAction` 入队；引擎 150ms 轮询执行（与托盘同一通道）。
//!   禁止 UI 直接创建/销毁壁纸窗口——窗口生命周期由 `sync_monitors` 独占。

use std::sync::Mutex;

use gpui_kit::Global;

use crate::session::SessionManager;

/// 引擎全局（API.md §4：cx.set_global(AppState::new(sm))）。
pub struct AppState {
    pub sm: SessionManager,
}

impl AppState {
    pub fn new(sm: SessionManager) -> Self {
        AppState { sm }
    }
}

impl Global for AppState {}

/// UI → 引擎的写动作。
#[derive(Debug, Clone, PartialEq)]
pub enum EngineAction {
    /// 指派/换壁纸（SessionManager::assign，只重建该显示器会话）。
    Assign {
        monitor_id: String,
        entry_id: String,
    },
    /// 暂停/恢复全部（SessionManager::pause_all）。
    /// 托盘菜单已覆盖；为后续界面全局暂停控件保留。
    #[allow(dead_code)]
    PauseAll(bool),
    /// 全量同步显示器（SessionManager::sync_monitors，幂等）。
    SyncMonitors,
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
