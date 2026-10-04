//! 窗口状态（全局唯一真源）。
//!
//! 形状按 技术方案 §3.8：持久状态唯一真源是一个 Global；UI Entity 只是投影。
//! 所有变更走本模块的 `update`（唯一写入路径），完成后刷新窗口 —— 单向数据流。

use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, BorrowAppContext as _, Global, SharedString, Window};

use super::data::{Kind, LibraryItem, MonitorEntry, PlayState, Settings};

/// 筛选段控件的当前值。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Kind(Kind),
    We,
}

impl Filter {
    pub fn label(self) -> &'static str {
        match self {
            Filter::All => super::strings::FILTER_ALL,
            Filter::We => super::strings::FILTER_WE,
            Filter::Kind(k) => k.filter_label(),
        }
    }
}

/// 库卡片拖拽载荷（signature #2：拖卡片 → 显示器投放区 → 放手即指派）。
#[derive(Clone)]
pub struct CardDrag {
    pub item_id: SharedString,
    pub item_name: SharedString,
    pub art: super::data::Art,
}

pub struct GessoState {
    pub library: Vec<LibraryItem>,
    pub monitors: Vec<MonitorEntry>,
    pub settings: Settings,
    /// 当前页签（Shell 的三页签状态也归真源管）。
    pub active_tab: super::shell::Tab,
    pub filter: Filter,
    pub query: String,
    /// 单击选中（状态条显示 meta 详情）。
    pub selected: Option<SharedString>,
    /// 悬停中的库卡片（signature #1：对应显示器边框点亮）。
    pub hovered: Option<SharedString>,
    /// 悬停轮播相位（所有卡片共用 tick；hover 时推进）。
    pub hover_frame: usize,
    /// 悬停预载阶段：帧序列尚未全部就绪，卡片显示 spinner 覆盖层
    pub hover_preloading: bool,
    /// 「检测到新显示器」提示条可见性。
    pub pending_new_monitor: bool,
    /// 顶栏手动主题切换后的模式提示（None = 跟随系统）。
    pub import_counter: usize,
    /// 状态条红字（不支持类型等原因说明，§4.3 拖入文件交互）。
    pub status_error: Option<String>,
    /// 库卡片拖拽进行中（区分卡片拖拽与 OS 文件拖放，供投放覆盖层判定）。
    pub card_dragging: bool,
    /// false = 页面数据来自真会话快照（main.rs 装配）；true = 纯演示数据。
    pub demo: bool,
}

impl Global for GessoState {}

impl Default for GessoState {
    fn default() -> Self {
        GessoState {
            // 真源注入前的空态（绝不用演示数据伪装——显示器页曾因此显示幽灵双屏）
            library: Vec::new(),
            monitors: Vec::new(),
            settings: Settings::default(),
            active_tab: super::shell::Tab::Library,
            filter: Filter::All,
            query: String::new(),
            selected: None,
            hovered: None,
            hover_frame: 0,
            hover_preloading: false,
            pending_new_monitor: false,
            import_counter: 0,
            status_error: None,
            card_dragging: false,
            demo: false,
        }
    }
}

pub fn state(cx: &App) -> &GessoState {
    cx.try_global::<GessoState>().expect("GessoState 未初始化")
}

/// 唯一写入路径；改完即整窗刷新（M3 换 Entity observe 精细化重绘）。
pub fn update(window: &mut Window, cx: &mut App, f: impl FnOnce(&mut GessoState)) {
    cx.update_global::<GessoState, _>(|g, _| f(g));
    window.refresh();
}

impl GessoState {
    /// 筛选 + 搜索后的库条目下标（标题/标签模糊匹配，即时过滤）。
    pub fn visible_items(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        self.library
            .iter()
            .enumerate()
            .filter(|(_, w)| {
                let kind_ok = match self.filter {
                    Filter::All => true,
                    Filter::We => w.we,
                    Filter::Kind(k) => w.kind == k,
                };
                let query_ok = q.is_empty()
                    || w.name.to_lowercase().contains(&q)
                    || w.id.to_lowercase().contains(&q);
                kind_ok && query_ok
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// 指派（§3.6 set_wallpaper 语义）：该屏旧壁纸置为未指派；失效素材拒绝指派。
    pub fn assign(&mut self, monitor_idx: usize, item_id: &str) -> Result<String, &'static str> {
        let Some(pos) = self.library.iter().position(|w| w.id.as_ref() == item_id) else {
            return Err("找不到该壁纸");
        };
        if self.library[pos].broken {
            return Err("素材失效，无法指派");
        }
        for w in &mut self.library {
            if w.assigned == Some(monitor_idx) {
                w.assigned = None;
            }
        }
        self.library[pos].assigned = Some(monitor_idx);
        if let Some(m) = self.monitors.get_mut(monitor_idx) {
            m.wallpaper = Some(item_id.into());
            m.state = PlayState::Playing; // 指派即恢复（原型行为）
        }
        Ok(self.library[pos].name.to_string())
    }

    /// 从库中移除（只出库，永不删文件 —— §7 用词纪律）。
    pub fn remove(&mut self, item_id: &str) {
        self.library.retain(|w| w.id.as_ref() != item_id);
        if self
            .selected
            .as_ref()
            .is_some_and(|s| s.as_ref() == item_id)
        {
            self.selected = None;
        }
    }

    /// 单屏暂停/恢复（用户暂停，§9 Paused(user)）。
    pub fn toggle_pause(&mut self, monitor_idx: usize) {
        if let Some(m) = self.monitors.get_mut(monitor_idx) {
            m.state = if m.state.paused() {
                PlayState::Playing
            } else {
                PlayState::UserPaused
            };
        }
    }

    /// 重新检测（演示：揭示「检测到新显示器」提示条）。
    pub fn redetect(&mut self) {
        self.pending_new_monitor = true;
    }

    /// 主显示器下标（0 = 主；多显示器时即第一块）。
    pub fn main_monitor(&self) -> usize {
        0
    }

    /// 找某 id 的显示器名。
    pub fn monitor_name(&self, idx: usize) -> String {
        self.monitors
            .get(idx)
            .map(|m| m.name.to_string())
            .unwrap_or_else(|| "未知显示器".into())
    }
}

/// UI 指派（API.md §4）：UI 立即更新投影并触发刷新；引擎写操作经动作队列
/// 由引擎轮询执行（真源），失败由下一次快照回灌自愈（不做乐观承诺）。
/// 返回 Ok(条目名) / Err(原因)。
pub fn bridge_assign(
    window: &mut Window,
    cx: &mut App,
    monitor_idx: usize,
    item_id: &str,
) -> Result<String, String> {
    let (name, real_monitor, real_item, demo) = cx.update_global::<GessoState, _>(|g, _| {
        let out = g.assign(monitor_idx, item_id);
        (
            out,
            g.monitors
                .get(monitor_idx)
                .map(|m| m.real_id.clone())
                .unwrap_or_default(),
            g.library
                .iter()
                .find(|w| w.id.as_ref() == item_id)
                .map(|w| w.real),
            g.demo,
        )
    });
    let name = name.map_err(|e| e.to_string())?;
    if !demo && real_item.unwrap_or(false) && !real_monitor.is_empty() {
        crate::engine::enqueue(crate::engine::EngineAction::Assign {
            monitor_id: real_monitor,
            entry_id: item_id.into(),
        });
    }
    update(window, cx, |g| g.selected = Some(item_id.into()));
    Ok(name)
}

/* ---------- 导入（P1：rfd 对话框 / 文件拖入共用同一路径） ---------- */

use crate::session::{ImportCheck, ImportError};

/// 导入失败 → 文案（strings.rs 唯一出处；§7 失败文案带原因和出路）。
pub fn import_error_text(e: ImportError) -> String {
    use super::strings::*;
    match e {
        ImportError::Unsupported => import_err_unsupported(),
        ImportError::Mkv => import_err_mkv(),
        ImportError::Hevc => import_err_hevc(),
        ImportError::Io => import_err_io(),
    }
}

/// 预检 + 入队导入；不支持类型 → 状态条红字（返回是否受理）。
pub fn import_paths(
    paths: impl Iterator<Item = std::path::PathBuf>,
    window: &mut Window,
    cx: &mut App,
) {
    for path in paths {
        match crate::session::SessionManager::classify_import(&path) {
            ImportCheck::Ok(_) => {
                crate::engine::enqueue(crate::engine::EngineAction::Import {
                    path: path.display().to_string(),
                });
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("未命名")
                    .to_string();
                update(window, cx, |g| {
                    g.status_error = None;
                });
                window.push_notification(
                    gpui_kit::component::notification::Notification::success(
                        super::strings::toast_imported(&name),
                    ),
                    cx,
                );
            }
            ImportCheck::Err(e) => {
                let msg = import_error_text(e);
                update(window, cx, |g| {
                    g.status_error = Some(msg.clone());
                    g.selected = None;
                });
                window.push_notification(
                    gpui_kit::component::notification::Notification::warning(msg),
                    cx,
                );
            }
        }
    }
}

/// 「导入」按钮：rfd **异步**文件选择 → 同一导入路径。
///
/// ⚠️ 必须用 AsyncFileDialog：同步 `FileDialog::pick_file` 会在主线程跑
/// `NSApp run_modal` 嵌套事件循环，GPUI 的 App RefCell 在嵌套循环里被
/// 事件重入借用 → "RefCell already borrowed" panic（app.rs:955，实测崩溃）。
/// ModalFuture/FileHandle 均 Send，可在 GPUI 后台任务 await，结果入队动作。
pub fn import_with_dialog(_window: &mut Window, cx: &mut App) {
    cx.background_executor()
        .spawn(async move {
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter("壁纸文件", &["mp4", "webm", "gif", "webp", "html", "glsl"])
                .pick_file()
                .await
            else {
                return; // 用户取消
            };
            crate::engine::enqueue(crate::engine::EngineAction::Import {
                path: handle.path().display().to_string(),
            });
        })
        .detach();
}

