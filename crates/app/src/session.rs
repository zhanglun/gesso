//! 会话管理器（M3）：唯一编排者。
//!
//! 职责：读配置 → 枚举显示器 → 每个已配置显示器一个壁纸窗口会话；
//! 状态机转移全部经 gesso_core::session::transfer（单一真源）；
//! 显示器热插拔轮询 → diff 重建（v1 简化轮询，后续换平台通知）。

use std::collections::BTreeMap;

use gesso_core::{
    transfer, AppConfig, ContentSpec, LibraryEntry, PausePolicy, SessionEvent, SessionState,
    WallpaperKind,
};

use crate::pin::{self, MonitorInfo, WallpaperWindow};

/// 导入失败原因（UI 侧映射 strings.rs 文案；§7 失败文案带原因和出路）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportError {
    /// 不支持的类型（拒绝并说明支持的类型列表）。
    Unsupported,
    /// mkv 容器：webview 不支持，需转封装。
    Mkv,
    /// HEVC：Windows 需系统扩展（macOS 原生支持，仍统一提示）。
    Hevc,
    /// 磁盘 I/O 失败。
    Io,
}

/// 导入类型判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportCheck {
    Ok(WallpaperKind),
    Err(ImportError),
}

/// 自动暂停的原因（同一 Autopause 状态的 UI 投影区分；技术方案 §418 的
/// Paused(fullscreen)/Paused(battery)）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutopauseReason {
    Fullscreen,
    Battery,
}

/// 策略解析结果（纯函数 `suspend_effect` 的输出；执行在 apply_autopause）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspendEffect {
    /// 不干预。
    None,
    /// 自动暂停（经状态机 AutoPauseTrigger/Clear）。
    Pause(AutopauseReason),
    /// 降帧到 5 fps（时钟类壁纸需求，技术方案 §423；引擎经 setFps 透传，
    /// shader 实时生效、html 建议值、video 无帧率杠杆维持播放）。
    Downscale,
}

/// M5 数据桥策略解析：全屏优先于电池（本屏全屏时按全屏策略）。
pub fn suspend_effect(
    fullscreen_on_monitor: bool,
    on_battery: bool,
    fullscreen_policy: PausePolicy,
    battery_policy: PausePolicy,
) -> SuspendEffect {
    if fullscreen_on_monitor {
        match fullscreen_policy {
            PausePolicy::Pause => SuspendEffect::Pause(AutopauseReason::Fullscreen),
            PausePolicy::Downscale => SuspendEffect::Downscale,
            PausePolicy::Ignore => SuspendEffect::None,
        }
    } else if on_battery {
        match battery_policy {
            PausePolicy::Pause => SuspendEffect::Pause(AutopauseReason::Battery),
            PausePolicy::Downscale => SuspendEffect::Downscale,
            PausePolicy::Ignore => SuspendEffect::None,
        }
    } else {
        SuspendEffect::None
    }
}

pub struct Session {
    pub state: SessionState,
    pub monitor: MonitorInfo,
    pub entry_id: String,
    pub window: Option<Box<dyn WallpaperWindow>>,
    /// 当前生效的自动暂停原因（None = 非自动暂停态；进 PausedUser 也不清它——
    /// 状态机里用户暂停压住自动暂停，退出全屏后仍由 AutoPauseClear 恢复）。
    pub autopause_reason: Option<AutopauseReason>,
    /// M5 降帧生效中（恢复时按会话配置回设 fps）。
    pub downscaled: bool,
    /// 光标 feed：本会话上次推送的量化状态（present,x_q,y_q,bits）。
    /// None = 尚未推送过；present=false 表示光标已离开本屏。
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    // 平台面：macOS 光标 feed（M5）读写
    pub mouse_last: Option<(bool, u16, u16, u8)>,
    /// 鼠标静止空闲降帧生效中（与全屏降帧互斥，恢复时回设 fps）。
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub idle_down: bool,
}

/// 会话的 UI 投影视图（`views()` 产物）。
pub struct SessionView {
    pub monitor_id: String,
    pub state: SessionState,
    pub autopause_reason: Option<AutopauseReason>,
}

pub struct SessionManager {
    pub config: AppConfig,
    pub library: Vec<LibraryEntry>,
    sessions: BTreeMap<String, Session>,
}

impl SessionManager {
    pub fn new(config: AppConfig, library: Vec<LibraryEntry>) -> Self {
        Self {
            config,
            library,
            sessions: BTreeMap::new(),
        }
    }

    /// explorer 重启自愈（Windows M1）：TaskbarCreated 后由引擎轮询触发。
    /// explorer 死亡会连带销毁挂在其 WorkerW 下的壁纸窗口（跨进程父窗口死亡），
    /// 所以这里**整窗重建**（pin::create + load），不是对旧句柄重挂。
    /// 返回 false = 有窗口未落位（explorer 未就绪），调用方应置位重试。
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))] // 仅 Windows 重挂载路径
    pub fn remount_all(&mut self) -> bool {
        // 先提取重建计划（避免 sessions/library 交叉借用），再逐个整窗重建
        let plans: Vec<(String, MonitorInfo, LibraryEntry, u8)> = self
            .sessions
            .iter()
            .filter_map(|(mid, s)| {
                let entry = self.library.iter().find(|e| e.id == s.entry_id).cloned()?;
                let fps = self.fps_for(mid);
                Some((mid.clone(), s.monitor.clone(), entry, fps))
            })
            .collect();
        let mut all_ok = true;
        for (mid, monitor, entry, fps) in plans {
            let Some(s) = self.sessions.get_mut(&mid) else {
                continue;
            };
            s.window = None; // 旧窗口多半已被 explorer 连带销毁；Drop 容忍 DestroyWindow 失败
            match Self::build_window_only(&monitor) {
                Ok(mut w) => {
                    w.load(&Self::entry_host_url(&entry, fps));
                    all_ok &= w.mount_ok();
                    s.window = Some(w);
                }
                Err(e) => {
                    println!("[session] {mid} 重钉失败：{e}");
                    all_ok = false;
                }
            }
        }
        all_ok
    }

    /// 全量同步：枚举显示器 → 按配置建/拆会话（启动与显示器轮询共用）。
    pub fn sync_monitors(&mut self) {
        let monitors = pin::enumerate_monitors();

        // v1 已知限制：CGDirectDisplayID 不跨重启/重连稳定（技术方案 §4.3 的 EDID 哈希是正解）。
        // 当配置里的 key 全部失配（如系统重编了 cg-id）时，把映射迁移到当前主屏，避免静默丢壁纸。
        let known: std::collections::BTreeSet<String> =
            monitors.iter().map(|m| m.id.clone()).collect();
        if !self.config.monitors.is_empty()
            && self.config.monitors.keys().all(|k| !known.contains(k))
        {
            if let Some(main) = monitors
                .iter()
                .find(|m| m.is_main)
                .or_else(|| monitors.first())
            {
                let old_map: Vec<(String, String)> = self
                    .config
                    .monitors
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let first_entry = old_map.first().map(|(_, v)| v.clone());
                self.config.monitors.clear();
                if let Some(entry) = first_entry {
                    self.config.monitors.insert(main.id.clone(), entry);
                }
                let _ = self.save_config();
                println!(
                    "[session] 显示器 ID 已失效（系统重编），配置迁移 → {}",
                    main.id
                );
            }
        }

        // 首启占位键 "main" → 解析为真实主屏 ID（一次性改写并落盘）
        if self.config.monitors.remove("main").is_some() {
            if let Some(main) = monitors
                .iter()
                .find(|m| m.is_main)
                .or_else(|| monitors.first())
            {
                let real = main.id.clone();
                let entry = "builtin-testsrc"; // 占位键只配内置样例
                self.config.monitors.insert(real.clone(), entry.into());
                let _ = self.save_config();
                println!("[session] 首启占位 main → {real}");
            }
        }

        let now: std::collections::BTreeSet<String> =
            monitors.iter().map(|m| m.id.clone()).collect();

        // 拆掉已拔出的显示器
        let gone: Vec<String> = self
            .sessions
            .keys()
            .filter(|id| !now.contains(*id))
            .cloned()
            .collect();
        for id in gone {
            self.sessions.remove(&id);
            println!("[session] 显示器 {id} 已断开，会话拆除（配置保留）");
        }

        // 新建/补齐已配置的显示器
        for m in monitors {
            let Some(entry_id) = self.config.monitors.get(&m.id).cloned() else {
                continue;
            };
            if self.sessions.contains_key(&m.id) {
                continue;
            }
            let entry = match self.library.iter().find(|e| e.id == entry_id) {
                Some(e) => e.clone(),
                None => {
                    println!("[session] {} 指向的条目 {} 不存在，跳过", m.id, entry_id);
                    continue;
                }
            };
            let title = entry.title.clone();
            let fps = self.fps_for(&m.id);
            match Self::build_session(m.clone(), entry, fps) {
                Ok(s) => {
                    println!("[session] {} ← {}（{}）", m.id, s.entry_id, title);
                    self.sessions.insert(m.id, s);
                }
                Err(e) => println!("[session] {} 创建失败：{e}", m.id),
            }
        }
    }

    /// 条目自包含：把宿主页拷进条目目录（贴 WE 项目模型；M3 起随条目分发）
    /// 宿主页 URL：走 gesso:// 协议，宿主页为 assets 中的单一共享副本，
    /// 不再拷入条目目录（避免宿主页与用户资源共用 index.* 命名空间）。
    fn ensure_entry_host(_entry: &LibraryEntry) -> String {
        crate::protocol::host_url().to_string()
    }

    /// ContentSpec 构建（assign / set_fps / build_session 共用同一套字段映射）。
    pub(crate) fn content_spec(entry: &LibraryEntry, fps: u8) -> ContentSpec {
        // 远端网页条目：source 就是 https URL 本身（宿主页 iframe 直装）；
        // 本地/WE 条目走 entry_main_source → gesso:// 资源 URL
        let main_source = match &entry.source_url {
            Some(url) => {
                return ContentSpec {
                    kind: entry.kind,
                    source: url.clone(),
                    fit: gesso_core::Fit::Cover,
                    fps_cap: fps,
                    audio: gesso_core::AudioPolicy::Muted,
                    meta: gesso_core::SpecMeta {
                        title: entry.title.clone(),
                        origin: entry.origin.clone(),
                    },
                };
            }
            None => crate::encoding::entry_main_source(entry),
        };
        ContentSpec {
            kind: entry.kind,
            // WE 条目走 steam 直引路由，其余走 library；都是绝对 gesso URL
            source: crate::protocol::entry_url(entry, &main_source),
            fit: gesso_core::Fit::Cover,
            fps_cap: fps,
            audio: gesso_core::AudioPolicy::Muted,
            meta: gesso_core::SpecMeta {
                title: entry.title.clone(),
                origin: entry.origin.clone(),
            },
        }
    }

    /// 宿主页 URL：条目自包含 host 拷贝 + spec。
    /// shader 额外携带 code=（base64url 源码）：file:// 页面里 fetch/XHR 被
    /// WKWebView 拦截，查询参数不受限，源码随 URL 直达宿主页。
    pub(crate) fn entry_host_url(entry: &LibraryEntry, fps: u8) -> String {
        let spec = Self::content_spec(entry, fps);
        let mut url = format!(
            "{}?spec={}",
            Self::ensure_entry_host(entry),
            crate::encoding::urlencode(&serde_json::to_string(&spec).expect("ContentSpec 序列化"))
        );
        if entry.kind == WallpaperKind::Shader {
            if let Some(name) = crate::encoding::main_asset_name(&entry.source_dir, entry.kind) {
                if let Ok(bytes) = std::fs::read(std::path::Path::new(&entry.source_dir).join(name))
                {
                    url.push_str("&code=");
                    url.push_str(&crate::encoding::base64url(&bytes));
                }
            }
        }
        url
    }

    /// 会话帧率 = 显示器覆盖值 → 全局默认（UI 快照与策略共用同一解析）
    pub(crate) fn fps_for(&self, monitor_id: &str) -> u8 {
        self.config
            .monitor_fps
            .get(monitor_id)
            .copied()
            .unwrap_or(self.config.settings.fps_cap_default)
    }

    fn build_session(
        monitor: MonitorInfo,
        entry: LibraryEntry,
        fps: u8,
    ) -> gesso_core::Result<Session> {
        let mut window = pin::create_wallpaper_window(&monitor)?;
        let url = Self::entry_host_url(&entry, fps);
        window.load(&url);
        println!("[session] 宿主页 URL = {}", url);
        Ok(Session {
            state: SessionState::Playing,
            monitor,
            entry_id: entry.id,
            window: Some(window),
            autopause_reason: None,
            downscaled: false,
            mouse_last: None,
            idle_down: false,
        })
    }

    /// 托盘：暂停/恢复全部
    pub fn pause_all(&mut self, paused: bool) {
        let ev = if paused {
            SessionEvent::UserPause
        } else {
            SessionEvent::UserResume
        };
        for s in self.sessions.values_mut() {
            let next = transfer(s.state, ev);
            s.state = next;
            if let Some(w) = s.window.as_mut() {
                if next == SessionState::PausedUser {
                    w.set_paused(true);
                } else if next == SessionState::Playing {
                    w.set_paused(false);
                }
            }
        }
    }

    /// 单显示器暂停/恢复（§4.4 屏卡片 ⏸/▶；经状态机转移，与 pause_all 同级）
    pub fn pause_one(&mut self, monitor_id: &str, paused: bool) {
        let ev = if paused {
            SessionEvent::UserPause
        } else {
            SessionEvent::UserResume
        };
        if let Some(s) = self.sessions.get_mut(monitor_id) {
            let next = transfer(s.state, ev);
            s.state = next;
            if let Some(w) = s.window.as_mut() {
                if next == SessionState::PausedUser {
                    w.set_paused(true);
                } else if next == SessionState::Playing {
                    w.set_paused(false);
                }
            }
        }
    }

    /// 主显示器循环切换到下一个库条目（托盘「换壁纸」）
    pub fn cycle_main(&mut self) {
        let Some(main_id) = self.sessions.keys().next().cloned() else {
            return;
        };
        let cur = self.config.monitors.get(&main_id).cloned();
        let idx = cur
            .and_then(|id| self.library.iter().position(|e| e.id == id))
            .map(|i| (i + 1) % self.library.len())
            .unwrap_or(0);
        let next_entry = self.library[idx].id.clone();
        self.assign(&main_id, &next_entry);
    }

    /// 指派（换壁纸）：diff 式只重建目标会话。
    pub fn assign(&mut self, monitor_id: &str, entry_id: &str) {
        // 先在不可变阶段提取全部所需数据，再进入可变操作
        let entry = self.library.iter().find(|e| e.id == entry_id).cloned();
        let fps = self.fps_for(monitor_id);
        self.config
            .monitors
            .insert(monitor_id.into(), entry_id.into());
        let _ = self.save_config();
        if let Some(s) = self.sessions.get_mut(monitor_id) {
            // 状态机：暂停/错误态重新指派 → Loading → Playing
            s.state = transfer(s.state, SessionEvent::Assign);
            if let Some(entry) = entry {
                s.entry_id = entry_id.into();
                if let Ok(mut w) = Self::build_window_only(&s.monitor) {
                    w.load(&Self::entry_host_url(&entry, fps));
                    s.window = Some(w);
                    s.state = transfer(s.state, SessionEvent::Loaded);
                    // 新会话页不继承旧桥状态（autopause/降帧随指派清零）
                    s.autopause_reason = None;
                    s.downscaled = false;
                }
            }
        } else {
            self.sync_monitors(); // 未建会话的显示器（如新指派）
        }
    }

    fn build_window_only(monitor: &MonitorInfo) -> gesso_core::Result<Box<dyn WallpaperWindow>> {
        pin::create_wallpaper_window(monitor)
    }

    /// 设置更新（设置页全部即时生效：写内存 + 落盘，§4.5）
    pub fn update_settings(&mut self, settings: gesso_core::Settings) {
        self.config.settings = settings;
        let _ = self.save_config();
    }

    /// 单显示器帧率上限（§4.4 FPS 下拉）：写配置 + 热重载该会话宿主页。
    pub fn set_fps(&mut self, monitor_id: &str, fps: u8) {
        // 找到该显示器当前指派的条目 → 构建新 spec URL → 热重载宿主页
        let Some(entry_id) = self.config.monitors.get(monitor_id) else {
            return;
        };
        let entry_id = entry_id.clone();
        let Some(entry) = self.library.iter().find(|e| e.id == entry_id) else {
            return;
        };
        let entry = entry.clone();
        let url = Self::entry_host_url(&entry, fps);
        // 落配置
        self.config.monitor_fps.insert(monitor_id.into(), fps);
        let _ = self.save_config();
        // 热重载已建会话的宿主页（会话不存在则只留配置，下次 sync 生效）
        if let Some(s) = self.sessions.get_mut(monitor_id) {
            if let Some(w) = s.window.as_mut() {
                w.load(&url);
            }
        }
        println!("[session] {monitor_id} 帧率上限 = {fps} fps（热重载）");
    }

    /// 从库移除条目：清单 + 显示器映射 + 会话一并拆除。目录清理（§7 移除语义,
    /// 2026-10-07 变更）：local/builtin 的库内拷贝与 url 条目的缩略图家目录由
    /// Gesso 托管，随条目删除；WE 零拷贝引用指向用户 Steam 目录，绝不触碰。
    pub fn remove_entry(&mut self, entry_id: &str) {
        let removed = self.library.iter().find(|e| e.id == entry_id).cloned();
        self.library.retain(|e| e.id != entry_id);
        let affected: Vec<String> = self
            .config
            .monitors
            .iter()
            .filter(|(_, e)| e.as_str() == entry_id)
            .map(|(m, _)| m.clone())
            .collect();
        for m in &affected {
            self.config.monitors.remove(m);
            self.sessions.remove(m);
        }
        let _ = self.save_config();
        let p = crate::protocol::library_dir().join("library.json");
        let saved = gesso_core::LibraryManifest {
            entries: self.library.clone(),
        }
        .save(&p)
        .is_ok();
        // 目录清理只在清单落盘成功后执行——落盘失败时清单仍含该条目，
        // 删目录会造成「清单有、磁盘无」的失效条目
        if saved {
            if let Some(entry) = removed {
                if let Some(dir) = Self::owned_dir_in(
                    &entry,
                    &crate::protocol::library_dir(),
                ) {
                    if let Err(e) = std::fs::remove_dir_all(&dir) {
                        println!("[session] 条目目录清理失败（{}）：{e}", dir.display());
                    }
                }
            }
        }
    }

    /// 条目移除时 Gesso 有权删除的目录（纯函数，安全守卫可测）：
    /// - `wallpaper-engine`：None——零拷贝引用指向用户 Steam 工坊，绝不删除
    /// - `url`：库内 `<id>/`（缩略图家目录，内容全由 Gesso 生成）
    /// - `local`/`builtin`：`source_dir` 本身，但**仅当它经 canonicalize 后
    ///   确实位于库根之内**——异常清单指向库外时守卫拒绝（防 ../ 与符号链接逃逸）
    fn owned_dir_in(
        entry: &LibraryEntry,
        lib_root: &std::path::Path,
    ) -> Option<std::path::PathBuf> {
        match entry.origin.as_str() {
            "wallpaper-engine" => None,
            "url" => Some(lib_root.join(&entry.id)),
            _ => {
                let p = std::path::PathBuf::from(&entry.source_dir);
                let canon_entry = std::fs::canonicalize(&p).ok()?;
                let canon_root = std::fs::canonicalize(lib_root).ok()?;
                (canon_entry != canon_root && canon_entry.starts_with(canon_root)).then_some(p)
            }
        }
    }

/// 导入结果的类型判定（UI 预检与引擎执行共用同一套规则）。
    /// 类型知识查 core 描述表；这里只保留"已知但拒绝"的特殊错误。
    pub fn classify_import(path: &std::path::Path) -> ImportCheck {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            return ImportCheck::Err(ImportError::Unsupported);
        };
        let ext = ext.to_ascii_lowercase();
        match ext.as_str() {
            "mkv" => return ImportCheck::Err(ImportError::Mkv),
            "hevc" | "h265" | "heic" => return ImportCheck::Err(ImportError::Hevc),
            _ => {}
        }
        match gesso_core::kind_from_ext(&ext) {
            Some(kind) => ImportCheck::Ok(kind),
            None => ImportCheck::Err(ImportError::Unsupported),
        }
    }

    /// 导入（P1）：随机 ID → 拷贝进库（v1 拷贝制）→ 清单落盘 → 追加内存库。
    pub fn import_entry(&mut self, path: &std::path::Path) -> Result<LibraryEntry, ImportError> {
        let kind = match Self::classify_import(path) {
            ImportCheck::Ok(kind) => kind,
            ImportCheck::Err(e) => return Err(e),
        };
        let ext = crate::encoding::import_ext(path, kind);
        let id = gesso_core::generate_id();
        let dst_dir = crate::protocol::library_dir().join(&id);
        std::fs::create_dir_all(&dst_dir).map_err(|_| ImportError::Io)?;
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("未命名")
            .to_string();
        // Html 条目的用户页面固定存为 wallpaper.html：index.html 留给宿主页
        // （ensure_entry_host 启动时会覆盖写 index.html）
        let asset = match kind {
            WallpaperKind::Html => "wallpaper.html".to_string(),
            _ => format!("index.{ext}"),
        };
        std::fs::copy(path, dst_dir.join(asset)).map_err(|_| ImportError::Io)?;
        // 抽帧不在本函数做：import_entry 跑在引擎主线程，v1 在这里同步抽 ~1s
        // （导入即卡顿），且经裸 FFI 路径。视频条目的补帧由 main.rs 的 Import
        // 分支在动作处理后异步调度（ThumbScheduler 去重/限额，完成经 ThumbsDone 回灌）。
        let entry = LibraryEntry {
            id: id.clone(),
            kind,
            title,
            origin: "local".into(),
            source_dir: dst_dir.display().to_string(),
            main_file: None,
            source_url: None,
        };
        self.library.push(entry.clone());
        gesso_core::LibraryManifest {
            entries: self.library.clone(),
        }
        .save(&crate::protocol::library_dir().join("library.json"))
        .map_err(|_| ImportError::Io)?;
        Ok(entry)
    }

    /// 导入远端网页（origin="url"；§4.3 工具条 🔗）。零素材目录：条目只在库内
    /// 占一个 `<id>/`（缩略图家目录，见 encoding::thumb_home），渲染地址就是
    /// URL 本身（content_spec 分支）。UI 预检与引擎侧共用 parse_remote_url。
    pub fn import_url_entry(&mut self, url: &str) -> Result<LibraryEntry, ImportError> {
        let url = crate::encoding::parse_remote_url(url).map_err(|_| ImportError::Unsupported)?;
        let id = gesso_core::generate_id();
        // 缩略图家目录必须存在，capture 才能写帧
        let home = crate::protocol::library_dir().join(&id);
        std::fs::create_dir_all(&home).map_err(|_| ImportError::Io)?;
        let entry = LibraryEntry {
            id: id.clone(),
            kind: WallpaperKind::Html,
            title: crate::encoding::title_from_url(&url),
            origin: "url".into(),
            source_dir: String::new(),
            main_file: None,
            source_url: Some(url),
        };
        self.library.push(entry.clone());
        gesso_core::LibraryManifest {
            entries: self.library.clone(),
        }
        .save(&crate::protocol::library_dir().join("library.json"))
        .map_err(|_| ImportError::Io)?;
        Ok(entry)
    }

    /// M6：导入一个 WE 工坊条目（video/web；scene/application 已在 UI 过滤）。
    ///
    /// video：拷主文件（project.file）为 index.<ext>，复用视频管线；
    /// web：拷贝整个条目目录，把入口（project.file）改名为 wallpaper.html
    /// 并注入 WE API shim，其余子资源相对路径不动，复用 html 管线。
    /// ponytail: 当前拷贝制（video 文档设想的零拷贝直引待 gesso:// 修复）。
    pub fn import_we_entry(&mut self, e: &crate::we::WeEntry) -> Result<LibraryEntry, ImportError> {
        use crate::we::WeKind;
        let title = if e.project.title.is_empty() {
            e.workshop_id.clone()
        } else {
            e.project.title.clone()
        };
        let id = gesso_core::generate_id();

        let kind = match e.kind() {
            WeKind::Video => WallpaperKind::Video,
            WeKind::Web => WallpaperKind::Html,
            WeKind::Unsupported(_) | WeKind::UnsupportedStr(_) => {
                return Err(ImportError::Unsupported)
            }
        };

        // 零拷贝：source_dir 直接指向 Steam 工坊目录（只读，绝不修改原文件）。
        // web 的 shim 在 gesso://steam 协议层内存注入，video 直接流式 Range 读取。
        // main_file 记住 project.json 声明的主资源名（video 媒体 / web 入口）。
        let main_file = match kind {
            WallpaperKind::Video if !e.project.file.is_empty() => Some(e.project.file.clone()),
            WallpaperKind::Html => Some(
                if e.project.file.is_empty() {
                    "index.html"
                } else {
                    e.project.file.as_str()
                }
                .to_string(),
            ),
            _ => None,
        };
        let entry = LibraryEntry {
            id: id.clone(),
            kind,
            title,
            origin: "wallpaper-engine".into(),
            source_dir: e.dir.display().to_string(),
            main_file,
            source_url: None,
        };
        self.library.push(entry.clone());
        gesso_core::LibraryManifest {
            entries: self.library.clone(),
        }
        .save(&crate::protocol::library_dir().join("library.json"))
        .map_err(|_| ImportError::Io)?;
        Ok(entry)
    }

    pub fn save_config(&self) -> gesso_core::Result<()> {
        self.config
            .save(&crate::protocol::config_dir().join("config.json"))
    }

    /// 诊断：对每个会话打时间点真值
    pub fn diagnose_all(&self, tag: &str) {
        for (id, s) in &self.sessions {
            println!("[diag {tag}] session {id} state={:?}", s.state);
            if let Some(w) = s.window.as_ref() {
                w.diag(tag);
            }
        }
    }

    /// M5 数据桥：把一次桥采样落成会话状态（全屏/电池自动暂停 + 降帧）。
    /// 返回是否有状态变化（调用方据此刷新 UI）。用户暂停（PausedUser）压住
    /// 自动暂停——状态机里 AutoPauseTrigger 对 PausedUser 是 no-op，这里同步跳过。
    pub fn apply_autopause(
        &mut self,
        fullscreen: &std::collections::BTreeSet<String>,
        on_battery: Option<bool>,
    ) -> bool {
        let fs_policy = self.config.settings.fullscreen_policy;
        let bat_policy = self.config.settings.battery_policy;
        let battery = on_battery == Some(true);
        let mut changed = false;
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in ids {
            let fps = self.fps_for(&id);
            let effect = suspend_effect(fullscreen.contains(&id), battery, fs_policy, bat_policy);
            let Some(s) = self.sessions.get_mut(&id) else {
                continue;
            };
            match effect {
                SuspendEffect::None => {
                    if s.state == SessionState::Autopause {
                        s.state = transfer(s.state, SessionEvent::AutoPauseClear);
                        s.autopause_reason = None;
                        if let Some(w) = s.window.as_mut() {
                            w.set_paused(false);
                        }
                        changed = true;
                        println!("[bridge] {id} 自动暂停解除");
                    }
                    if s.downscaled {
                        s.downscaled = false;
                        if let Some(w) = s.window.as_mut() {
                            w.send(crate::host_cmd::HostCommand::SetFps(fps));
                        }
                        println!("[bridge] {id} 恢复帧率 {fps} fps");
                    }
                }
                SuspendEffect::Pause(reason) => {
                    if s.state == SessionState::Playing {
                        s.state = transfer(s.state, SessionEvent::AutoPauseTrigger);
                        s.autopause_reason = Some(reason);
                        if let Some(w) = s.window.as_mut() {
                            w.set_paused(true);
                        }
                        changed = true;
                        println!("[bridge] {id} 自动暂停（{reason:?}）");
                    } else if s.state == SessionState::Autopause
                        && s.autopause_reason != Some(reason)
                    {
                        // 原因切换（全屏退出但仍电池供电）：窗口保持暂停，仅换投影
                        s.autopause_reason = Some(reason);
                        changed = true;
                    }
                    s.downscaled = false; // 暂停压过降帧
                }
                SuspendEffect::Downscale => {
                    if s.state == SessionState::Autopause {
                        s.state = transfer(s.state, SessionEvent::AutoPauseClear);
                        s.autopause_reason = None;
                        if let Some(w) = s.window.as_mut() {
                            w.set_paused(false);
                        }
                        changed = true;
                    }
                    if !s.downscaled {
                        s.downscaled = true;
                        if let Some(w) = s.window.as_mut() {
                            w.send(crate::host_cmd::HostCommand::SetFps(5));
                        }
                        println!("[bridge] {id} 降帧 → 5 fps");
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    /// M5 时间脉冲：Rust 每秒驱动一次宿主页时钟（时钟类壁纸 DoD——
    /// 挂钟时间由引擎事件推进，壁纸不必自起高频轮询）。
    pub fn broadcast_time_tick(&mut self) {
        for s in self.sessions.values_mut() {
            if let Some(w) = s.window.as_mut() {
                w.send(crate::host_cmd::HostCommand::Tick);
            }
        }
    }

    /// M5 光标 feed：把全局光标位置路由到它所在显示器的会话。
    ///
    /// 设计（性能纪律）：① 归一化到 [0,1] 后量化 u16 再比较，亚像素抖动不
    /// 触发推送；② 状态没变零 IPC，鼠标静止时只做空闲判断；③ 离开某屏推送
    /// 一次 present=0；④ 仅推给光标所在的那一个屏，其余屏不打扰；
    /// ⑤ 暂停 / Autopause / 全屏降帧期间不喂光标。
    /// 空闲降帧：静止超阈值（当前固定 5 分钟）降到 5fps，一动即恢复。
    pub fn poll_mouse(&mut self, m: &crate::bridge::MouseSample) {
        /// ponytail: 空闲阈值先固定 5 分钟；需要 per-user 时挪进 settings
        const IDLE_AFTER: f64 = 300.0;
        const Q: f64 = 10_000.0;
        let fps_by_id: std::collections::BTreeMap<String, u8> = self
            .sessions
            .keys()
            .map(|id| (id.clone(), self.fps_for(id)))
            .collect();

        // 光标落在哪个显示器（AppKit 全局坐标，矩形包含判定）
        let target: Option<String> = self
            .sessions
            .values()
            .find(|s| {
                let (x, y, w, h) = s.monitor.frame;
                m.x >= x && m.x <= x + w && m.y >= y && m.y <= y + h
            })
            .map(|s| s.monitor.id.clone());

        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in ids {
            let Some(s) = self.sessions.get_mut(&id) else {
                continue;
            };
            let here = target.as_deref() == Some(id.as_str());

            // 空闲降帧只在 Playing 且无自动暂停降帧时有意义；其它态一律收敛
            let normal_fps = fps_by_id[&id];
            let can_idle = s.state == SessionState::Playing && !s.downscaled;

            if !here || !can_idle {
                // 光标离屏或本会话不该响应：推送一次 present=0（shader 可据此淡出）
                if s.mouse_last != Some((false, 0, 0, 0)) {
                    if let Some(w) = s.window.as_mut() {
                        w.send(crate::host_cmd::HostCommand::MouseLeave);
                    }
                    s.mouse_last = Some((false, 0, 0, 0));
                }
                if s.idle_down {
                    s.idle_down = false;
                    if let Some(w) = s.window.as_mut() {
                        w.send(crate::host_cmd::HostCommand::SetFps(normal_fps));
                    }
                    println!("[bridge] {id} 空闲降帧解除");
                }
                continue;
            }

            // 归一化并量化。宿主页契约 = 左下原点（fragCoord/iMouse）：
            // macOS 桥交付的 m.y 已是左下全局（AppKit），ny 即左下占比；
            // Windows 桥/帧同为顶左物理像素，在此契约边界一次翻转为左下
            // （勿在桥内预翻再此处翻——「勿翻两次」，工程笔记 M5）。
            let (mx, my, mw, mh) = s.monitor.frame;
            let nx = ((m.x - mx) / mw).clamp(0.0, 1.0);
            let ny = ((m.y - my) / mh).clamp(0.0, 1.0);
            #[cfg(target_os = "windows")]
            let ny = 1.0 - ny;
            let xq = (nx * Q).round() as u16;
            let yq = (ny * Q).round() as u16;
            let cur = (true, xq, yq, m.buttons);
            if s.mouse_last != Some(cur) {
                if let Some(w) = s.window.as_mut() {
                    w.send(crate::host_cmd::HostCommand::Mouse {
                        x: xq,
                        y: yq,
                        buttons: m.buttons,
                    });
                }
                s.mouse_last = Some(cur);
            }

            // 空闲降帧 / 恢复
            if m.idle_secs >= IDLE_AFTER {
                if !s.idle_down {
                    s.idle_down = true;
                    if let Some(w) = s.window.as_mut() {
                        w.send(crate::host_cmd::HostCommand::SetFps(5));
                    }
                    println!("[bridge] {id} 空闲降帧 → 5 fps");
                }
            } else if s.idle_down {
                s.idle_down = false;
                if let Some(w) = s.window.as_mut() {
                    w.send(crate::host_cmd::HostCommand::SetFps(normal_fps));
                }
                println!("[bridge] {id} 空闲降帧解除");
            }
        }
    }

    /// UI 只读快照（API.md §1）
    pub fn library(&self) -> &[gesso_core::LibraryEntry] {
        &self.library
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn monitors(&self) -> Vec<MonitorInfo> {
        pin::enumerate_monitors()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// 会话视图（UI 快照数据源；autopause_reason 供 FullscreenPaused/BatteryPaused 投影）
    pub fn views(&self) -> Vec<SessionView> {
        self.sessions
            .iter()
            .map(|(m, s)| SessionView {
                monitor_id: m.clone(),
                state: s.state,
                autopause_reason: s.autopause_reason,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn suspend_effect_fullscreen_wins_over_battery() {
        use PausePolicy as P;
        // 全屏优先于电池（本屏全屏按全屏策略）
        assert_eq!(
            suspend_effect(true, true, P::Pause, P::Downscale),
            SuspendEffect::Pause(AutopauseReason::Fullscreen)
        );
        // 电池供电单独触发
        assert_eq!(
            suspend_effect(false, true, P::Pause, P::Pause),
            SuspendEffect::Pause(AutopauseReason::Battery)
        );
        // 接通电源 + 无全屏 = 不干预
        assert_eq!(
            suspend_effect(false, false, P::Pause, P::Pause),
            SuspendEffect::None
        );
        // 降帧策略（时钟类壁纸，技术方案 §423）
        assert_eq!(
            suspend_effect(true, false, P::Downscale, P::Pause),
            SuspendEffect::Downscale
        );
        assert_eq!(
            suspend_effect(false, true, P::Pause, P::Downscale),
            SuspendEffect::Downscale
        );
        // 忽略策略
        assert_eq!(
            suspend_effect(true, false, P::Ignore, P::Pause),
            SuspendEffect::None
        );
        assert_eq!(
            suspend_effect(false, true, P::Pause, P::Ignore),
            SuspendEffect::None
        );
    }

    fn entry_with(origin: &str, source_dir: &str) -> LibraryEntry {
        LibraryEntry {
            id: "test0123456789ab".into(),
            kind: gesso_core::WallpaperKind::Video,
            title: "t".into(),
            origin: origin.into(),
            source_dir: source_dir.into(),
            main_file: None,
            source_url: None,
        }
    }

    #[test]
    fn owned_dir_rules_by_origin_and_guard() {
        let root = std::env::temp_dir().join(format!("gesso-owned-{}", std::process::id()));
        let inside = root.join("entry01");
        std::fs::create_dir_all(&inside).unwrap();
        let outside = std::env::temp_dir().join(format!("gesso-outside-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();

        // local 在库内 → 删；source_dir 即守卫判定的返回路径
        assert_eq!(
            SessionManager::owned_dir_in(&entry_with("local", inside.to_str().unwrap()), &root),
            Some(inside.clone())
        );
        // builtin 同 local 语义
        assert!(SessionManager::owned_dir_in(&entry_with("builtin", inside.to_str().unwrap()), &root).is_some());
        // local 指向库外 → 守卫拒绝（canonicalize 防 ../ 与符号链接逃逸）
        assert_eq!(
            SessionManager::owned_dir_in(&entry_with("local", outside.to_str().unwrap()), &root),
            None
        );
        // source_dir = 库根本身 → 拒绝（绝不能删整库）
        assert_eq!(SessionManager::owned_dir_in(&entry_with("local", root.to_str().unwrap()), &root), None);
        // 目录不存在（canonicalize 失败）→ 拒绝
        assert_eq!(
            SessionManager::owned_dir_in(&entry_with("local", root.join("nope").to_str().unwrap()), &root),
            None
        );
        // WE 零拷贝 → 永不删
        assert_eq!(
            SessionManager::owned_dir_in(&entry_with("wallpaper-engine", inside.to_str().unwrap()), &root),
            None
        );
        // url → 库内 <id>/ 缩略图家目录
        assert_eq!(
            SessionManager::owned_dir_in(&entry_with("url", ""), &root),
            Some(root.join("test0123456789ab"))
        );

        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn base64url_known_vectors() {
        // RFC 4648 测试向量（标准字母表）经 url 字母表映射
        assert_eq!(crate::encoding::base64url(b""), "");
        assert_eq!(crate::encoding::base64url(b"f"), "Zg");
        assert_eq!(crate::encoding::base64url(b"fo"), "Zm8");
        assert_eq!(crate::encoding::base64url(b"foo"), "Zm9v");
        assert_eq!(crate::encoding::base64url(b"foob"), "Zm9vYg");
        assert_eq!(crate::encoding::base64url(b"fooba"), "Zm9vYmE");
        assert_eq!(crate::encoding::base64url(b"foobar"), "Zm9vYmFy");
        // +/ 不出现（URL 安全）
        let all: Vec<u8> = (0..=255u8).collect();
        let enc = crate::encoding::base64url(&all);
        assert!(!enc.contains('+') && !enc.contains('/'));
    }

    #[test]
    fn main_asset_ignores_host_html_pollution() {
        // 真实场景：图片条目目录混入宿主页 index.html + index.png
        let dir = std::env::temp_dir().join(format!("gesso-poll-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), b"HOST").unwrap();
        std::fs::write(dir.join("index.png"), b"IMG").unwrap();
        let got =
            crate::encoding::main_asset_name(&dir.display().to_string(), WallpaperKind::Image);
        assert_eq!(
            got.as_deref(),
            Some("index.png"),
            "不得误选宿主页 index.html"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn classify_static_images() {
        for f in ["a.jpg", "a.jpeg", "a.png", "a.avif"] {
            assert_eq!(
                SessionManager::classify_import(Path::new(&format!("/tmp/{f}"))),
                ImportCheck::Ok(WallpaperKind::Image),
                "{f} 应识别为图片"
            );
        }
    }

    #[test]
    fn import_ext_preserves_source_extension() {
        // webm/webp 不能被改名成 mp4/gif（WKWebView 按扩展名判定类型）
        assert_eq!(
            crate::encoding::import_ext(Path::new("/tmp/a.webm"), WallpaperKind::Video),
            "webm"
        );
        assert_eq!(
            crate::encoding::import_ext(Path::new("/tmp/a.webp"), WallpaperKind::Image),
            "webp"
        );
        assert_eq!(
            crate::encoding::import_ext(Path::new("/tmp/a.MP4"), WallpaperKind::Video),
            "mp4"
        );
        // 无扩展名 → 类型默认
        assert_eq!(
            crate::encoding::import_ext(Path::new("/tmp/noext"), WallpaperKind::Html),
            "html"
        );
    }

    #[test]
    fn html_entry_asset_is_wallpaper_html_never_host() {
        let dir = std::env::temp_dir().join(format!("gesso-html-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // 只有宿主页 index.html（导入半途）→ None：index.html 永远不是壁纸资源
        std::fs::write(dir.join("index.html"), b"<html>host</html>").unwrap();
        assert_eq!(
            crate::encoding::main_asset_name(&dir.display().to_string(), WallpaperKind::Html),
            None
        );

        // wallpaper.html 就位 → 命中
        std::fs::write(dir.join("wallpaper.html"), b"<html>wallpaper</html>").unwrap();
        assert_eq!(
            crate::encoding::main_asset_name(&dir.display().to_string(), WallpaperKind::Html)
                .as_deref(),
            Some("wallpaper.html")
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn main_asset_name_prefers_kind_ext_then_any_index() {
        let dir = std::env::temp_dir().join(format!("gesso-asset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // 空目录 → None（失效判定依赖它）
        assert_eq!(
            crate::encoding::main_asset_name(&dir.display().to_string(), WallpaperKind::Video),
            None
        );

        // 只有 index.webm → 命中（类型默认缺失时用任意 index.*）
        std::fs::write(dir.join("index.webm"), b"x").unwrap();
        assert_eq!(
            crate::encoding::main_asset_name(&dir.display().to_string(), WallpaperKind::Video)
                .as_deref(),
            Some("index.webm")
        );

        // 同时存在 index.mp4 → 类型默认优先
        std::fs::write(dir.join("index.mp4"), b"x").unwrap();
        assert_eq!(
            crate::encoding::main_asset_name(&dir.display().to_string(), WallpaperKind::Video)
                .as_deref(),
            Some("index.mp4")
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 光标归一化量化（poll_mouse 内联逻辑的纯函数镜像，防坐标系改坏）
    fn quantize(v: f64) -> u16 {
        (v.clamp(0.0, 1.0) * 10_000.0).round() as u16
    }

    #[test]
    fn mouse_normalize_quantize() {
        assert_eq!(quantize(0.0), 0);
        assert_eq!(quantize(1.0), 10_000);
        assert_eq!(quantize(0.5), 5_000);
        assert_eq!(quantize(-0.3), 0); // 越界收敛
        assert_eq!(quantize(1.7), 10_000);
        // 亚量化步长（1/10000）内抖动被同一量化值吸收 = 不触发推送
        // 亚量化步长（1/10000）内的抖动被同一量化值吸收 = 不触发推送
        assert_eq!(quantize(0.50003), quantize(0.50004));
    }
}

#[cfg(test)]
mod we_import_tests {
    use super::*;
    use std::fs;

    fn fixture() -> (tempfile_lite::TempHome, crate::we::WeEntry) {
        let home = tempfile_lite::TempHome::new();
        let root = home.path.join("workshop/content/431960/222");
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(
            root.join("project.json"),
            r#"{"type":"web","file":"index.html","title":"My Web"}"#,
        )
        .unwrap();
        fs::write(
            root.join("index.html"),
            "<html><head><title>T</title></head><body><h1>hi</h1></body></html>",
        )
        .unwrap();
        fs::write(root.join("assets/a.js"), b"JSCODE").unwrap();
        let entry = crate::we::import_we_at(&root, "222").unwrap();
        (home, entry)
    }

    #[test]
    fn imports_web_zero_copy_points_to_source() {
        let _env = crate::ENV_LOCK.lock().unwrap();
        let (home, entry) = fixture();
        let mut sm = SessionManager::new(Default::default(), Vec::new());
        let got = sm.import_we_entry(&entry).expect("导入成功");
        assert_eq!(got.kind, WallpaperKind::Html);
        assert_eq!(got.title, "My Web");
        assert_eq!(got.origin, "wallpaper-engine");
        assert_eq!(got.main_file.as_deref(), Some("index.html"));
        let dir = std::path::Path::new(&got.source_dir);
        // 零拷贝：原目录原封不动——入口仍 index.html，project.json 还在，shim 不落盘
        let orig = fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(orig.contains("<title>T</title>"));
        assert!(!orig.contains("wallpaperRegisterAudioListener"));
        assert!(dir.join("project.json").exists());
        assert_eq!(fs::read(dir.join("assets/a.js")).unwrap(), b"JSCODE");
        assert_eq!(crate::encoding::entry_main_source(&got), "index.html");
        // 入库 + manifest 落盘
        assert_eq!(sm.library.len(), 1);
        assert!(crate::protocol::library_dir().join("library.json").exists());
        drop(home); // 保活到断言结束（TempHome 提前 drop 会删掉夹具）
    }

    #[test]
    fn rejects_scene() {
        let _env = crate::ENV_LOCK.lock().unwrap();
        let home = tempfile_lite::TempHome::new();
        let root = home.path.join("workshop/content/431960/9");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("project.json"),
            r#"{"type":"scene","file":"s.pkg","title":"S"}"#,
        )
        .unwrap();
        let entry = crate::we::import_we_at(&root, "9").unwrap();
        let mut sm = SessionManager::new(Default::default(), Vec::new());
        assert_eq!(sm.import_we_entry(&entry), Err(ImportError::Unsupported));
    }
}

/// 最小临时 HOME：设置 HOME 环境变量指向临时目录，drop 时清理。
#[cfg(test)]
mod tempfile_lite {
    pub struct TempHome {
        pub path: std::path::PathBuf,
    }
    impl TempHome {
        pub fn new() -> Self {
            // 随机后缀：同进程多个 TempHome（并行测试）不得共用同一目录，
            // 否则后建者的 remove_dir_all 会删掉前者夹具。
            let suffix = gesso_core::generate_id();
            let path = std::env::temp_dir().join(format!("gesso-home-{}", suffix));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            // ponytail: set_var 依赖测试单进程；并行测试只共享 HOME 变量值，
            // 但各夹具路径已唯一，find_steam 读到的 HOME 是谁都能各自找到自己的文件。
            unsafe {
                std::env::set_var("HOME", &path);
            }
            TempHome { path }
        }
    }
    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
