//! Wallpaper Engine 工坊导入（M6）：读取用户本机已订阅内容，只读不下载。
//!
//! 数据模型：`<steam>/steamapps/workshop/content/431960/<id>/project.json`
//! （431960 = WE AppID）。本模块只做定位/扫描/解析；导入（拷贝/落库）在
//! `SessionManager::import_we_entry`。scene/application 类型明确拒绝，
//! video/web 支持。零拷贝直引留给 gesso:// 协议修复，当前走拷贝制。

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// WE AppID（工坊目录名）。
pub const APPID: &str = "431960";

/// WE `project.json`（只取需要的字段，其余忽略）。
#[derive(Debug, Clone, Deserialize)]
pub struct WeProject {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub title: String,
}

/// 扫描到的一个 WE 条目。
#[derive(Debug, Clone)]
pub struct WeEntry {
    /// 工坊 ID（目录名）。
    pub workshop_id: String,
    pub project: WeProject,
    /// 条目目录（含 project.json）。
    pub dir: PathBuf,
}

impl WeEntry {
    /// 归一化的 WE 类型。
    pub fn kind(&self) -> WeKind {
        match self.project.kind.as_str() {
            "video" => WeKind::Video,
            "web" => WeKind::Web,
            "scene" => WeKind::Unsupported("scene"),
            "application" => WeKind::Unsupported("application"),
            other => WeKind::UnsupportedStr(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WeKind {
    Video,
    Web,
    /// 具名不支持（scene/application）。
    Unsupported(&'static str),
    /// 未知类型串。
    UnsupportedStr(String),
}

/// 定位 Steam 安装根（含 steamapps 的目录）。
///
/// 候选：① `$STEAM_DIR`；② macOS 常规安装路径；③ Windows 注册表（M1 阶段补，
/// 暂查常见盘符路径）。多 Steam 库（libraryfolders.vdf）的展开为升级项。
pub fn find_steam() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("STEAM_DIR") {
        let p = PathBuf::from(d);
        if p.join("steamapps").is_dir() {
            return Some(p);
        }
    }
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "~/Library/Application Support/Steam",
            "~/Steam",
        ]
    } else {
        // ponytail: Windows 路径先靠 STEAM_DIR/常规盘符；注册表查找 M1 补
        &[
            "C:/Program Files (x86)/Steam",
            "C:/Program Files/Steam",
            "D:/Steam",
        ]
    };
    for c in candidates {
        let p = shellexpand(c);
        if p.join("steamapps").is_dir() {
            return Some(p);
        }
    }
    None
}

/// 简单 `~` 展开（不引第三方依赖）。
fn shellexpand(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(s)
}

/// 工坊根目录（可能不存在 = 未订阅任何内容）。
pub fn workshop_root(steam: &Path) -> PathBuf {
    steam.join("steamapps/workshop/content").join(APPID)
}

/// 从条目目录重建单个 WeEntry（动作只带路径/ID，不携带项目数据）。
pub fn import_we_at(dir: &Path, workshop_id: &str) -> Option<WeEntry> {
    let bytes = std::fs::read(dir.join("project.json")).ok()?;
    let project = serde_json::from_slice::<WeProject>(&bytes).ok()?;
    Some(WeEntry { workshop_id: workshop_id.to_string(), project, dir: dir.to_path_buf() })
}

/// 扫描全部 WE 工坊条目（解析每个 `<id>/project.json`，坏条目跳过）。
pub fn scan(steam: &Path) -> Vec<WeEntry> {
    let root = workshop_root(steam);
    let Ok(rd) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let pj = dir.join("project.json");
        let Ok(bytes) = std::fs::read(&pj) else {
            continue;
        };
        match serde_json::from_slice::<WeProject>(&bytes) {
            Ok(project) => out.push(WeEntry {
                workshop_id: dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_string(),
                project,
                dir,
            }),
            Err(_) => continue, // 损坏的 project.json：跳过，不阻断整轮
        }
    }
    // 标题排序，UI 稳定展示
    out.sort_by(|a, b| a.project.title.cmp(&b.project.title));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fix() -> PathBuf {
        let base = std::env::temp_dir().join("gesso-we-fix");
        let _ = fs::remove_dir_all(&base);
        let root = base.join("steam/steamapps/workshop/content").join(APPID);
        fs::create_dir_all(&root).unwrap();
        // video
        let v = root.join("111"); fs::create_dir_all(&v).unwrap();
        fs::write(v.join("project.json"), r#"{"type":"video","file":"wall.mp4","title":"A Video"}"#).unwrap();
        fs::write(v.join("wall.mp4"), b"VID").unwrap();
        // web
        let w = root.join("222"); fs::create_dir_all(w.join("assets")).unwrap();
        fs::write(w.join("project.json"), r#"{"type":"web","file":"index.html","title":"B Web"}"#).unwrap();
        fs::write(w.join("index.html"), "<html><head></head><body></body></html>").unwrap();
        fs::write(w.join("assets/a.js"), b"js").unwrap();
        // scene
        let c = root.join("333"); fs::create_dir_all(&c).unwrap();
        fs::write(c.join("project.json"), r#"{"type":"scene","file":"scene.pkg","title":"C Scene"}"#).unwrap();
        // 坏 project.json
        let b = root.join("444"); fs::create_dir_all(&b).unwrap();
        fs::write(b.join("project.json"), b"not json").unwrap();
        base.join("steam")
    }

    #[test]
    fn scan_fixture_classifies_and_skips_bad() {
        let steam = fix();
        let v = scan(&steam);
        // 坏条目 444 跳过；剩 3 个，标题排序 A,B,C
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].project.title, "A Video");
        assert_eq!(v[0].kind(), WeKind::Video);
        assert_eq!(v[1].kind(), WeKind::Web);
        assert_eq!(v[2].kind(), WeKind::Unsupported("scene"));
        // 工坊 ID 正确
        assert_eq!(v[0].workshop_id, "111");
    }

    #[test]
    fn missing_workshop_returns_empty() {
        let base = std::env::temp_dir().join("gesso-we-empty");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("steam/steamapps")).unwrap();
        assert!(scan(&base.join("steam")).is_empty());
    }
}
