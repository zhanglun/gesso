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
        let mut monitors = pin::macos::enumerate_monitors();

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
            match Self::build_session(m.clone(), entry) {
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

    fn build_session(monitor: MonitorInfo, entry: LibraryEntry) -> gesso_core::Result<Session> {
        let mut window = pin::create_wallpaper_window(&monitor)?;
        let spec = ContentSpec {
            kind: entry.kind,
            source: entry_main_source(&entry),
            fit: gesso_core::Fit::Cover,
            fps_cap: 60,
            audio: gesso_core::AudioPolicy::Muted,
            meta: gesso_core::SpecMeta {
                title: entry.title.clone(),
                origin: entry.origin.clone(),
            },
        };
        let url = format!(
            "{}?spec={}",
            Self::ensure_entry_host(&entry),
            urlencode(&serde_json::to_string(&spec).expect("ContentSpec 序列化"))
        );
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
        self.config
            .monitors
            .insert(monitor_id.into(), entry_id.into());
        let _ = self.save_config();
        if let Some(s) = self.sessions.get_mut(monitor_id) {
            // 状态机：暂停/错误态重新指派 → Loading → Playing
            s.state = transfer(s.state, SessionEvent::Assign);
            if let Some(entry) = self.library.iter().find(|e| e.id == entry_id).cloned() {
                s.entry_id = entry_id.into();
                if let Ok(mut w) = Self::build_window_only(&s.monitor) {
                    let spec = ContentSpec {
                        kind: entry.kind,
                        source: entry_main_source(&entry),
                        fit: gesso_core::Fit::Cover,
                        fps_cap: 60,
                        audio: gesso_core::AudioPolicy::Muted,
                        meta: gesso_core::SpecMeta {
                            title: entry.title.clone(),
                            origin: entry.origin.clone(),
                        },
                    };
                    w.load(&format!(
                        "{}?spec={}",
                        Self::ensure_entry_host(&entry),
                        urlencode(&serde_json::to_string(&spec).unwrap())
                    ));
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

/// 条目主资源：v1 约定 source_dir 内 index.<ext>（video: index.mp4；image: index.gif…）
fn entry_main_source(entry: &LibraryEntry) -> String {
    let ext = match entry.kind {
        WallpaperKind::Video => "mp4",
        WallpaperKind::Image => "gif",
        WallpaperKind::Shader => "glsl",
        WallpaperKind::Html => "html",
    };
    format!("index.{ext}") // 相对宿主页同目录（条目自包含）
}
