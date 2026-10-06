//! Wallpaper Engine 内容解析：**只在用户主动导入时**被调用。
//!
//! 我们绝不扫描用户磁盘、不枚举 Steam/工坊。用户通过导入对话框自己选中一个
//! WE 条目的 `project.json`（或整目录），本模块负责解析它；之后的使用走
//! `gesso://steam/<entry-id>/<rel>` 路由（见 protocol.rs）。
//!
//! WE 数据模型：`<条目目录>/project.json`，431960 = WE AppID。
//! scene/application 明确拒绝，video/web 支持。

use std::path::{Path, PathBuf};

use serde::Deserialize;

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

/// 解析后的一个 WE 条目。
#[derive(Debug, Clone)]
pub struct WeEntry {
    /// 工坊 ID（通常是条目目录名；由用户导入路径得出）。
    pub workshop_id: String,
    pub project: WeProject,
    /// 条目目录（含 project.json；用户选中的位置）。
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

/// 从用户选中的条目目录解析单个 WE 条目。`dir` = 含 project.json 的目录。
pub fn import_we_at(dir: &Path, workshop_id: &str) -> Option<WeEntry> {
    let bytes = std::fs::read(dir.join("project.json")).ok()?;
    let project = serde_json::from_slice::<WeProject>(&bytes).ok()?;
    Some(WeEntry {
        workshop_id: workshop_id.to_string(),
        project,
        dir: dir.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 解析用户选中的 WE project.json：video/web 识别，scene/application 拒绝。
    #[test]
    fn parses_user_selected_project() {
        let base =
            std::env::temp_dir().join(format!("gesso-we-parse-{}", gesso_core::generate_id()));
        let dir = base.join("12345");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("project.json"),
            r#"{"type":"web","file":"index.html","title":"User Web"}"#,
        )
        .unwrap();

        let e = import_we_at(&dir, "12345").expect("解析成功");
        assert_eq!(e.workshop_id, "12345");
        assert_eq!(e.project.title, "User Web");
        assert_eq!(e.kind(), WeKind::Web);
        assert_eq!(e.dir, dir);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn missing_project_json_returns_none() {
        let base =
            std::env::temp_dir().join(format!("gesso-we-none-{}", gesso_core::generate_id()));
        fs::create_dir_all(&base).unwrap();
        assert!(import_we_at(&base, "x").is_none());
        let _ = fs::remove_dir_all(&base);
    }
}
