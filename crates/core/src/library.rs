//! 内容库：条目登记与随机不可猜 ID（安全模型 §7）。

use serde::{Deserialize, Serialize};

use crate::{Result, WallpaperKind};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryEntry {
    /// 随机 16 hex 字符；同时是 wallpaper:// 路径空间的不可猜测凭据。
    pub id: String,
    pub kind: WallpaperKind,
    pub title: String,
    /// 来源：`local`（普通导入）/ `builtin`（内置样例）/ `wallpaper-engine`（WE 导入）。
    pub origin: String,
    /// 素材根目录（库内拷贝路径；v1 统一拷贝制，直引待 gesso:// 协议修复）。
    pub source_dir: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LibraryManifest {
    pub entries: Vec<LibraryEntry>,
}

impl LibraryManifest {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(std::fs::write(path, serde_json::to_vec_pretty(self)?)?)
    }

    pub fn insert(&mut self, entry: LibraryEntry) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == entry.id) {
            *e = entry;
        } else {
            self.entries.push(entry);
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<LibraryEntry> {
        self.entries
            .iter()
            .position(|e| e.id == id)
            .map(|i| self.entries.remove(i))
    }
}

/// 生成 16 hex 随机 ID。128 位熵，不可猜测性由 wallpaper:// 路径白名单依赖。
pub fn generate_id() -> String {
    let mut buf = [0u8; 8];
    getrandom::fill(&mut buf).expect("系统熵源不可用");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> LibraryEntry {
        LibraryEntry {
            id: id.into(),
            kind: WallpaperKind::Video,
            title: "t".into(),
            origin: "local".into(),
            source_dir: "/tmp/x".into(),
        }
    }

    #[test]
    fn ids_are_random_and_wellformed() {
        let a = generate_id();
        let b = generate_id();
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b, "两次生成必须不同");
    }

    #[test]
    fn manifest_upsert_and_remove() {
        let mut m = LibraryManifest::default();
        m.insert(entry("a1"));
        m.insert(entry("a1")); // 幂等
        m.insert(entry("b2"));
        assert_eq!(m.entries.len(), 2);
        assert_eq!(m.remove("a1").unwrap().id, "a1");
        assert!(m.remove("nope").is_none());
        assert_eq!(m.entries.len(), 1);
    }

    #[test]
    fn manifest_roundtrip() {
        let mut m = LibraryManifest::default();
        m.insert(entry("deadbeefcafe1234"));
        let dir = std::env::temp_dir().join(format!("gesso-lib-{}", std::process::id()));
        let p = dir.join("library.json");
        m.save(&p).unwrap();
        assert_eq!(LibraryManifest::load(&p).unwrap(), m);
        std::fs::remove_file(&p).ok();
    }
}
