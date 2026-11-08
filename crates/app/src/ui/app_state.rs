//! 窗口状态（全局唯一真源）。
//!
//! 持久状态唯一真源是一个 Global；UI Entity 只是投影。
//! 所有变更走本模块的 `update`（唯一写入路径），完成后刷新窗口 —— 单向数据流。

use gpui_kit::component::WindowExt as _;
use gpui_kit::{App, BorrowAppContext as _, Global, SharedString, Window};

use super::data::{Kind, LibraryItem, MonitorEntry, PlayState, Settings, UpdateView};

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
            Filter::All => super::strings::FILTER_ALL(),
            Filter::We => super::strings::FILTER_WE(),
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
    /// 顶栏手动主题切换后的模式提示（None = 跟随系统）。
    pub import_counter: usize,
    /// 状态条红字（不支持类型等原因说明，§4.3 拖入文件交互）。
    pub status_error: Option<String>,
    /// 软件更新子系统投影（§4.7；真源 = app::update 模块全局，快照注入）。
    pub update: UpdateView,
    /// 库卡片拖拽进行中（区分卡片拖拽与 OS 文件拖放，供投放覆盖层判定）。
    pub card_dragging: bool,
    /// 「为选屏挑壁纸」流程的目标屏（§4.3 2026-11-07）：存平台稳定 ID（real_id），
    /// 不存下标——拓扑变化下标会漂移；指派成功或取消后清空（UI 本地流程态）。
    pub assign_target: Option<String>,
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
            import_counter: 0,
            status_error: None,
            update: UpdateView::default(),
            card_dragging: false,
            assign_target: None,
            demo: false,
        }
    }
}

pub fn state(cx: &App) -> &GessoState {
    cx.try_global::<GessoState>().expect("GessoState 未初始化")
}

/// 唯一写入路径；改完即整窗刷新（可演进为 Entity observe 精细化重绘）。
pub fn update(window: &mut Window, cx: &mut App, f: impl FnOnce(&mut GessoState)) {
    cx.update_global::<GessoState, _>(|g, _| f(g));
    window.refresh();
}

impl GessoState {
    /// 筛选 + 搜索后的库条目下标（标题/标签模糊匹配，即时过滤）。
    /// 展示序 = 插入时间倒序：清单 insert 恒为尾部追加（插入序 = 时间序），
    /// 反转后最新入库的卡片排最前，导入后第一眼可见。
    pub fn visible_items(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        let mut idx: Vec<usize> = self
            .library
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
            .collect();
        idx.reverse();
        idx
    }

    /// 指派（§3.6 set_wallpaper 语义）：该屏旧壁纸置为未指派；失效素材拒绝指派。
    pub fn assign(&mut self, monitor_idx: usize, item_id: &str) -> Result<String, &'static str> {
        let Some(pos) = self.library.iter().position(|w| w.id.as_ref() == item_id) else {
            return Err(super::strings::ERR_WALLPAPER_MISSING());
        };
        if self.library[pos].broken {
            return Err(super::strings::ERR_ASSET_BROKEN());
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

    /// 从库中移除（§7 移除语义：local 拷贝目录入回收站，WE/url 只断引用）。
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

    /// 主显示器下标（枚举序不保证主屏在前，按 is_main 定位；找不到回落 0）。
    pub fn main_monitor(&self) -> usize {
        self.monitors.iter().position(|m| m.is_main).unwrap_or(0)
    }

    /// 「为选屏挑壁纸」流程的目标屏下标（real_id → 当前枚举序）。
    /// 屏已拔出 / 列表未含该 ID = None，横幅随之消失（自愈，不清存储——
    /// 重新接入同 ID 屏后流程仍在）。
    pub fn assign_target_index(&self) -> Option<usize> {
        let id = self.assign_target.as_ref()?;
        self.monitors.iter().position(|m| &m.real_id == id)
    }

    /// 找某 id 的显示器名。
    pub fn monitor_name(&self, idx: usize) -> String {
        self.monitors
            .get(idx)
            .map(|m| m.name.to_string())
            .unwrap_or_else(|| super::strings::UNKNOWN_DISPLAY().into())
    }
}

/// 指派动作裁决（纯函数便于测试）：Ok(Some) = 入队引擎动作；Ok(None) =
/// 演示模式 / 非引擎条目，UI 投影照常但不写引擎；Err = 目标屏缺失等失败原因。
fn resolve_assign_action(
    demo: bool,
    real_item: bool,
    real_monitor: &str,
    entry_id: &str,
) -> Result<Option<(String, String)>, String> {
    if demo || !real_item {
        return Ok(None);
    }
    if real_monitor.is_empty() {
        // 目标屏不在当前枚举里（拔出 / 列表未刷新）：如实报错——此前静默
        // no-op 但 toast 显示成功，是「设了没反应」的假成功来源
        return Err(crate::ui::strings::ERR_MONITOR_MISSING().into());
    }
    Ok(Some((real_monitor.to_string(), entry_id.to_string())))
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
    if let Some((monitor_id, entry_id)) =
        resolve_assign_action(demo, real_item.unwrap_or(false), &real_monitor, item_id)?
    {
        crate::engine::enqueue(crate::engine::EngineAction::Assign {
            monitor_id,
            entry_id,
        });
    }
    // 指派成功即结束「为选屏挑壁纸」流程（任何指派路径都经此处）
    update(window, cx, |g| {
        g.assign_target = None;
        g.selected = Some(item_id.into());
    });
    Ok(name)
}

/* ---------- 导入（P1：rfd 对话框 / 文件拖入共用同一路径） ---------- */

use crate::session::{ImportCheck, ImportError};

/// 导入失败 → 文案（strings.rs 唯一出处；§7 失败文案带原因和出路）。
pub fn import_error_text(e: ImportError) -> String {
    match e {
        ImportError::Unsupported => super::strings::IMPORT_ERR_UNSUPPORTED().into(),
        ImportError::Mkv => super::strings::IMPORT_ERR_MKV().into(),
        ImportError::Hevc => super::strings::IMPORT_ERR_HEVC().into(),
        ImportError::Io => super::strings::IMPORT_ERR_IO().into(),
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
                    .unwrap_or(super::strings::UNNAMED())
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
            // 扩展名全集见 gesso_core::content 描述表；rfd 要求 'static 切片，
            // 故在此列明，新增扩展名时同步此处（core 一致性测试会提示类型表侧）。
            let Some(handle) = rfd::AsyncFileDialog::new()
                .add_filter(
                    super::strings::FILTER_WALLPAPERS(),
                    &[
                        "mp4", "webm", "gif", "webp", "jpg", "jpeg", "png", "avif", "html", "glsl",
                    ],
                )
                .add_filter(super::strings::FILTER_WE_PROJECT(), &["json"])
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

/* ---------- 测试（纯状态逻辑，无 GPUI 上下文） ---------- */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::data::{Art, Kind, PlayState};
    use crate::ui::strings::ERR_MONITOR_MISSING;

    fn mon(real_id: &str, is_main: bool) -> MonitorEntry {
        MonitorEntry {
            name: real_id.to_string().into(),
            real_id: real_id.to_string(),
            short: real_id.to_string().into(),
            label: "1920×1080".into(),
            rect: (0., 0., 1920., 1080.),
            is_main,
            wallpaper: None,
            state: PlayState::Playing,
            fps: 60,
            fit: Default::default(),
        }
    }

    fn item(id: &str, broken: bool) -> LibraryItem {
        LibraryItem {
            thumb_pending: false,
            id: id.to_string().into(),
            name: id.to_string().into(),
            kind: Kind::Photo,
            we: false,
            meta: "test".into(),
            assigned: None,
            broken,
            remote: false,
            real: true,
            art: Art {
                from: 0x24345C,
                to: 0x0E0F13,
            },
            thumbs: vec![],
        }
    }

    /// 测试状态构造（clippy field_reassign_with_default：紧跟 default 的字段
    /// 赋值一律走结构体字面量）。
    fn g_state(monitors: Vec<MonitorEntry>, library: Vec<LibraryItem>) -> GessoState {
        GessoState {
            monitors,
            library,
            ..Default::default()
        }
    }

    /// 主屏定位：is_main 不在枚举首位时也要找对（此前硬编码 0——枚举序
    /// 不保证主屏在前，双击会指到非主屏）。
    #[test]
    fn main_monitor_prefers_is_main_over_order() {
        let mut g = g_state(
            vec![mon("win-DISPLAY2", false), mon("win-DISPLAY1", true)],
            vec![],
        );
        assert_eq!(g.main_monitor(), 1);
        // 无主屏 / 空表回落 0
        g.monitors = vec![mon("win-DISPLAY2", false)];
        assert_eq!(g.main_monitor(), 0);
        g.monitors.clear();
        assert_eq!(g.main_monitor(), 0);
    }

    /// 目标屏解析：real_id 命中 → 下标；拔出 / 空表 / 未设 → None（自愈，
    /// 库页横幅随之消失；重新接入同 ID 屏后流程仍在——存储不被清除）。
    #[test]
    fn assign_target_index_resolves_and_self_heals() {
        let mut g = g_state(
            vec![mon("edid-bnq802e", true), mon("edid-deld06f", false)],
            vec![],
        );
        g.assign_target = Some("edid-deld06f".into());
        assert_eq!(g.assign_target_index(), Some(1));
        g.assign_target = Some("edid-gone".into());
        assert_eq!(g.assign_target_index(), None);
        g.assign_target = None;
        assert_eq!(g.assign_target_index(), None);
        g.monitors.clear();
        g.assign_target = Some("edid-bnq802e".into());
        assert_eq!(g.assign_target_index(), None);
    }

    /// 指派投影（§3.6 set_wallpaper 语义）：目标屏落壁纸并恢复播放；目标屏
    /// 旧条目在 library 侧断开（卡片角标真源）；缺失 / 失效素材拒绝且不污染
    /// 投影。显示器投影的旧屏 wallpaper 由快照回灌自愈（assign 不动它）。
    #[test]
    fn assign_updates_projection_and_rejects_bad_items() {
        let mut g = g_state(
            vec![mon("m1", true), mon("m2", false)],
            vec![item("w1", false), item("w2", true)],
        );
        g.monitors[1].state = PlayState::UserPaused;

        // 指派到 m2：w1 角标 → m2，m2 投影接管并恢复播放
        assert_eq!(g.assign(1, "w1").unwrap(), "w1");
        assert_eq!(g.library[0].assigned, Some(1));
        assert_eq!(g.monitors[1].wallpaper, Some("w1".into()));
        assert_eq!(g.monitors[1].state, PlayState::Playing);

        // 换壁纸：目标屏旧条目在 library 侧断开
        g.library.push(item("w3", false));
        assert_eq!(g.assign(1, "w3").unwrap(), "w3");
        assert_eq!(g.library[0].assigned, None);
        assert_eq!(g.library[2].assigned, Some(1));
        assert_eq!(g.monitors[1].wallpaper, Some("w3".into()));

        // 缺失 / 失效拒绝，投影与角标保持原状
        assert!(g.assign(0, "missing").is_err());
        assert!(g.assign(0, "w2").is_err());
        assert_eq!(g.monitors[0].wallpaper, None);
        assert_eq!(g.library[2].assigned, Some(1));
    }

    /// 挑壁纸流程全链（状态级复现 UI 行为）：设目标屏 → 解析下标 → 指派到
    /// 该屏。回归点：修复前双击恒指 main_monitor()（硬编码 0），副屏永不可达。
    #[test]
    fn picking_flow_targets_the_selected_screen() {
        let mut g = g_state(
            vec![mon("edid-bnq802e", true), mon("edid-deld06f", false)],
            vec![item("w1", false)],
        );
        g.assign_target = Some("edid-deld06f".into());
        let idx = g.assign_target_index().expect("目标屏应可解析");
        assert_ne!(idx, g.main_monitor(), "流程内双击应指派到副屏而非主屏");
        g.assign(idx, "w1").unwrap();
        assert_eq!(g.monitors[idx].wallpaper, Some("w1".into()));
    }

    /// 展示序 = 插入时间倒序：库 Vec 按清单插入序存放（尾部追加 = 时间正序），
    /// visible_items 反转后最新入库排最前（2026-11-08 用户要求）。
    #[test]
    fn visible_items_lists_newest_first() {
        let g = g_state(
            vec![],
            vec![item("old", false), item("mid", false), item("new", false)],
        );
        assert_eq!(g.visible_items(), vec![2, 1, 0]);
        // 移除不打乱相对序
        let mut g = g;
        g.remove("mid");
        assert_eq!(g.visible_items(), vec![1, 0]);
    }

    /// 指派动作裁决：演示 / 非引擎条目不入队；目标屏缺失如实报错（此前
    /// 静默 no-op + 假成功 toast，是「设了没反应」的另一来源）；正常路径
    /// 产出 (monitor_id, entry_id) 引擎动作。
    #[test]
    fn resolve_assign_action_gates_and_errors() {
        assert!(resolve_assign_action(true, true, "m1", "w1")
            .unwrap()
            .is_none());
        assert!(resolve_assign_action(false, false, "m1", "w1")
            .unwrap()
            .is_none());
        let err = resolve_assign_action(false, true, "", "w1").unwrap_err();
        assert_eq!(err, ERR_MONITOR_MISSING());
        assert_eq!(
            resolve_assign_action(false, true, "edid-deld06f", "w1").unwrap(),
            Some(("edid-deld06f".into(), "w1".into()))
        );
    }
}
