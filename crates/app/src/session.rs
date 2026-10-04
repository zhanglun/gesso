//! 会话管理器（M3）：唯一编排者。
//!
//! 职责：读配置 → 枚举显示器 → 每个已配置显示器一个壁纸窗口会话；
//! 状态机转移全部经 gesso_core::session::transfer（单一真源）；
//! 显示器热插拔轮询 → diff 重建（v1 简化轮询，后续换平台通知）。

use std::collections::BTreeMap;

use gesso_core::{
    transfer, AppConfig, ContentSpec, LibraryEntry, SessionEvent, SessionState, WallpaperKind,
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

pub struct Session {
    pub state: SessionState,
    pub monitor: MonitorInfo,
    pub entry_id: String,
    pub window: Option<Box<dyn WallpaperWindow>>,
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

    /// 全量同步：枚举显示器 → 按配置建/拆会话（启动与显示器轮询共用）。
    pub fn sync_monitors(&mut self) {
        let monitors = pin::macos::enumerate_monitors();

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
    fn ensure_entry_host(entry: &LibraryEntry) -> String {
        let dir = std::path::PathBuf::from(&entry.source_dir);
        let host = dir.join("index.html");
        let src = crate::protocol::assets_dir().join("host/index.html");
        std::fs::create_dir_all(&dir).ok();
        std::fs::copy(&src, &host).ok(); // 开发期每次同步；M3 起随条目冻结
                                         // file:// 加载（gesso:// 自定义协议在本版 WKWebView 下静默失败，见 SPIKE-REPORT）
                                         // ⚠️ 路径必须百分号编码：库路径含空格（"Application Support"），
                                         // 裸空格会拼出非法 URL 被 WKWebView 拒载
        format!(
            "file://{}",
            percent_encode_path(&host.display().to_string())
        )
    }

    /// ContentSpec 构建（assign / set_fps / build_session 共用同一套字段映射）
    pub(crate) fn content_spec(entry: &LibraryEntry, fps: u8) -> ContentSpec {
        ContentSpec {
            kind: entry.kind,
            source: entry_main_source(&entry),
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
            urlencode(&serde_json::to_string(&spec).expect("ContentSpec 序列化"))
        );
        if entry.kind == WallpaperKind::Shader {
            if let Some(name) = main_asset_name(&entry.source_dir, entry.kind) {
                if let Ok(bytes) = std::fs::read(std::path::Path::new(&entry.source_dir).join(name))
                {
                    url.push_str("&code=");
                    url.push_str(&base64url(&bytes));
                }
            }
        }
        url
    }

    fn fps_for(&self, monitor_id: &str) -> u8 {
        self.config
            .monitor_fps
            .get(monitor_id)
            .copied()
            .unwrap_or(self.config.settings.fps_cap_default)
    }

    fn build_session(monitor: MonitorInfo, entry: LibraryEntry, fps: u8) -> gesso_core::Result<Session> {
        let mut window = pin::create_wallpaper_window(&monitor)?;
        let url = Self::entry_host_url(&entry, fps);
        window.load(&url);
        // 隔离实验：builder 的 with_url 可能绕过 scheme handler，创建后再显式加载一次
        println!("[session] 宿主页 URL = {}", url);
        Ok(Session {
            state: SessionState::Playing,
            monitor,
            entry_id: entry.id,
            window: Some(window),
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

    /// 从库移除条目：清单 + 显示器映射 + 会话一并拆除（文件保留）。
    pub fn remove_entry(&mut self, entry_id: &str) {
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
        gesso_core::LibraryManifest {
            entries: self.library.clone(),
        }
        .save(&p)
        .map_err(|_| ())
        .ok();
    }

    /// 导入结果的类型判定（UI 预检与引擎执行共用同一套规则）。
    pub fn classify_import(path: &std::path::Path) -> ImportCheck {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            return ImportCheck::Err(ImportError::Unsupported);
        };
        match ext.to_ascii_lowercase().as_str() {
            "mp4" | "webm" => ImportCheck::Ok(WallpaperKind::Video),
            "gif" | "webp" => ImportCheck::Ok(WallpaperKind::Image),
            "glsl" => ImportCheck::Ok(WallpaperKind::Shader),
            "html" => ImportCheck::Ok(WallpaperKind::Html),
            "mkv" => ImportCheck::Err(ImportError::Mkv),
            "hevc" | "h265" | "heic" => ImportCheck::Err(ImportError::Hevc),
            _ => ImportCheck::Err(ImportError::Unsupported),
        }
    }

    /// 导入（P1）：随机 ID → 拷贝进库（v1 拷贝制）→ 清单落盘 → 追加内存库。
    pub fn import_entry(&mut self, path: &std::path::Path) -> Result<LibraryEntry, ImportError> {
        let kind = match Self::classify_import(path) {
            ImportCheck::Ok(kind) => kind,
            ImportCheck::Err(e) => return Err(e),
        };
        let ext = import_ext(path, kind);
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

    /// UI 只读快照（API.md §1）
    pub fn library(&self) -> &[gesso_core::LibraryEntry] {
        &self.library
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn monitors(&self) -> Vec<MonitorInfo> {
        pin::macos::enumerate_monitors()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn states(&self) -> Vec<(String, String, SessionState)> {
        self.sessions
            .iter()
            .map(|(m, s)| (m.clone(), s.entry_id.clone(), s.state))
            .collect()
    }
}

/// base64url（无填充：base64url 字母表全属 unreserved，查询参数免转义；
/// 浏览器侧 atob 按 forgiving-base64 规则接受无填充输入）
fn base64url(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(T[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(T[n as usize & 63] as char);
        }
    }
    out
}

/// 文件路径 → URL 路径段编码（保留分隔符 /，其余非 unreserved 全部编码）
fn percent_encode_path(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// urlencode（spec 传参）
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 导入落盘用的扩展名：**保留源文件扩展名**（WKWebView 按扩展名判定媒体类型，
/// 把 `.webm` 存成 `index.mp4`、`.webp` 存成 `index.gif` 会直接播不出来）；
/// 源文件无扩展名时退回类型默认名。
pub fn import_ext(path: &std::path::Path, kind: WallpaperKind) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| default_ext(kind).to_string())
}

/// 类型对应的默认扩展名（导入时源文件无扩展名的兜底）
pub fn default_ext(kind: WallpaperKind) -> &'static str {
    match kind {
        WallpaperKind::Video => "mp4",
        WallpaperKind::Image => "gif",
        WallpaperKind::Shader => "glsl",
        WallpaperKind::Html => "html",
    }
}

/// 条目主资源文件名：优先目录内真实存在的 `index.*`（导入保留源扩展名），
/// 找不到时回退到类型默认名。**导入 / 失效判定 / 宿主页 spec 三处必须共用本函数**，
/// 否则 webm/webp 这类条目会出现"能导入但被判失效"或"宿主页请求错文件名"。
///
/// Html 条目例外：主资源固定 `wallpaper.html`——条目里的 `index.html` 永远是
/// 宿主页（`ensure_entry_host` 每次启动覆盖写入），绝不能被"任意 index.*"回退
/// 命中成壁纸资源（否则未完成导入的空壳条目看起来永远有效）。
pub fn main_asset_name(source_dir: &str, kind: WallpaperKind) -> Option<String> {
    if kind == WallpaperKind::Html {
        let p = std::path::Path::new(source_dir).join("wallpaper.html");
        return p.is_file().then(|| "wallpaper.html".to_string());
    }
    let dir = std::path::Path::new(source_dir);
    let pref = std::fs::read_dir(dir).ok()?;
    let mut fallback: Option<String> = None;
    for ent in pref.flatten() {
        let name = ent.file_name().to_string_lossy().to_string();
        if let Some(rest) = name.strip_prefix("index.") {
            if !rest.is_empty() {
                // 类型默认扩展名优先，其次任意 index.*
                if rest.eq_ignore_ascii_case(default_ext(kind)) {
                    return Some(name);
                }
                fallback.get_or_insert(name);
            }
        }
    }
    fallback
}

/// 条目主资源（相对宿主页同目录；条目自包含）
fn entry_main_source(entry: &LibraryEntry) -> String {
    main_asset_name(&entry.source_dir, entry.kind).unwrap_or_else(|| match entry.kind {
        WallpaperKind::Html => "wallpaper.html".to_string(),
        k => format!("index.{}", default_ext(k)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn base64url_known_vectors() {
        // RFC 4648 测试向量（标准字母表）经 url 字母表映射
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(b"fooba"), "Zm9vYmE");
        assert_eq!(base64url(b"foobar"), "Zm9vYmFy");
        // +/ 不出现（URL 安全）
        let all: Vec<u8> = (0..=255u8).collect();
        let enc = base64url(&all);
        assert!(!enc.contains('+') && !enc.contains('/'));
    }

    #[test]
    fn import_ext_preserves_source_extension() {
        // webm/webp 不能被改名成 mp4/gif（WKWebView 按扩展名判定类型）
        assert_eq!(
            import_ext(Path::new("/tmp/a.webm"), WallpaperKind::Video),
            "webm"
        );
        assert_eq!(
            import_ext(Path::new("/tmp/a.webp"), WallpaperKind::Image),
            "webp"
        );
        assert_eq!(
            import_ext(Path::new("/tmp/a.MP4"), WallpaperKind::Video),
            "mp4"
        );
        // 无扩展名 → 类型默认
        assert_eq!(
            import_ext(Path::new("/tmp/noext"), WallpaperKind::Html),
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
            main_asset_name(&dir.display().to_string(), WallpaperKind::Html),
            None
        );

        // wallpaper.html 就位 → 命中
        std::fs::write(dir.join("wallpaper.html"), b"<html>wallpaper</html>").unwrap();
        assert_eq!(
            main_asset_name(&dir.display().to_string(), WallpaperKind::Html).as_deref(),
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
            main_asset_name(&dir.display().to_string(), WallpaperKind::Video),
            None
        );

        // 只有 index.webm → 命中（类型默认缺失时用任意 index.*）
        std::fs::write(dir.join("index.webm"), b"x").unwrap();
        assert_eq!(
            main_asset_name(&dir.display().to_string(), WallpaperKind::Video).as_deref(),
            Some("index.webm")
        );

        // 同时存在 index.mp4 → 类型默认优先
        std::fs::write(dir.join("index.mp4"), b"x").unwrap();
        assert_eq!(
            main_asset_name(&dir.display().to_string(), WallpaperKind::Video).as_deref(),
            Some("index.mp4")
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
