//! Wallpaper Engine 工坊导入（M6）：读取用户本机已订阅内容，只读不下载。
//!
//! 数据模型：`<库根>/steamapps/workshop/content/431960/<id>/project.json`
//! （431960 = WE AppID）。Steam 可有多个内容库——主库 + libraryfolders.vdf
//! 里登记的其他库；工坊内容可能分散在各库，扫描/路由都要覆盖全部库。
//! 本模块只做定位/扫描/解析；导入（落库）在 `SessionManager::import_we_entry`。
//! scene/application 类型明确拒绝，video/web 支持。

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

/// 定位 Steam 主安装根（含 steamapps 的目录）。
///
/// 候选：① `$STEAM_DIR`；② macOS 常规安装路径；③ Windows 常见盘符路径
/// （注册表定位待 M1）。其他内容库经 [`find_libraries`] 展开。
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

/// 工坊根目录（某库下；可能不存在 = 该库未订阅任何内容）。
pub fn workshop_root(library: &Path) -> PathBuf {
    library.join("steamapps/workshop/content").join(APPID)
}

/// 枚举全部 Steam 内容库根（含 steamapps 的目录）：主库 + libraryfolders.vdf
/// 里登记的其他库，去重、只保留含 steamapps 者。返回空 = Steam 不可用。
pub fn find_libraries() -> Vec<PathBuf> {
    let Some(main) = find_steam() else { return Vec::new(); };
    let mut libs = vec![main.clone()];
    // libraryfolders.vdf：主库 steamapps 下，列出其他内容库。
    let vdf = main.join("steamapps/libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(&vdf) {
        for p in parse_library_paths(&text) {
            let p = PathBuf::from(p);
            if p.join("steamapps").is_dir() && !libs.contains(&p) {
                libs.push(p);
            }
        }
    }
    libs
}

/// 从 libraryfolders.vdf 提取各库路径。只认现代格式 `"path" "..."`（所有现存
/// Steam 版本均如此）；不做完整 VDF 分词，逐行找 `"path"` 后取下一个引号串。
/// 反斜杠转义还原（Windows 路径在 vdf 里写成 `D:\\Steam`）。
fn parse_library_paths(vdf: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in vdf.lines() {
        let bytes = line.as_bytes();
        let Some(after) = find_quoted(b"path", bytes) else { continue; };
        if let Some(v) = next_quoted(&bytes[after..]) {
            out.push(v);
        }
    }
    out
}

/// 在行内定位 `"key"`，返回其结束引号之后的字节偏移。
fn find_quoted(key: &[u8], line: &[u8]) -> Option<usize> {
    let n = key.len();
    let mut i = 0;
    while i + n < line.len() {
        if line[i] == b'"' && &line[i + 1..i + 1 + n] == key && line[i + 1 + n] == b'"' {
            return Some(i + n + 2);
        }
        i += 1;
    }
    None
}

/// 取从切片开头起第一个引号内的字符串，还原 `\` / `"` 转义。
fn next_quoted(s: &[u8]) -> Option<String> {
    let start = s.iter().position(|&b| b == b'"')? + 1;
    let mut out = String::new();
    let mut i = start;
    while i < s.len() {
        match s[i] {
            b'"' => return Some(out),
            b'\\' if i + 1 < s.len() => {
                out.push(s[i + 1] as char); // VDF 只转义反斜杠和引号
                i += 2;
            }
            b => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    None
}

/// 从条目目录重建单个 WeEntry（动作只带路径/ID，不携带项目数据）。
pub fn import_we_at(dir: &Path, workshop_id: &str) -> Option<WeEntry> {
    let bytes = std::fs::read(dir.join("project.json")).ok()?;
    let project = serde_json::from_slice::<WeProject>(&bytes).ok()?;
    Some(WeEntry { workshop_id: workshop_id.to_string(), project, dir: dir.to_path_buf() })
}

/// 扫描单个内容库里的 WE 工坊条目（坏条目跳过）。
fn scan_library(library: &Path) -> Vec<WeEntry> {
    let root = workshop_root(library);
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
    out
}

/// 扫描全部内容库的 WE 工坊条目（主库 + libraryfolders.vdf 其他库）；坏条目
/// 跳过，按标题排序，UI 稳定展示。
pub fn scan_all() -> Vec<WeEntry> {
    let mut all = Vec::new();
    for lib in find_libraries() {
        all.extend(scan_library(&lib));
    }
    all.sort_by(|a, b| a.project.title.cmp(&b.project.title));
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 在某库根下造一个工坊条目。
    fn put_entry(lib: &Path, id: &str, project: &str, files: &[&str]) {
        let dir = lib.join("steamapps/workshop/content").join(APPID).join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("project.json"), project).unwrap();
        for f in files {
            let fp = dir.join(f);
            if let Some(p) = fp.parent() {
                fs::create_dir_all(p).unwrap();
            }
            fs::write(fp, b"x").unwrap();
        }
    }

    #[test]
    fn parses_libraryfolders_vdf() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"name"		"main"
		"path"		"/Users/x/Library/Application Support/Steam"
	}
	"1"
	{
		"name"		"ssd"
		"path"		"/Volumes/SSD/Steam Library"
	}
}
"#;
        let paths = parse_library_paths(vdf);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], "/Users/x/Library/Application Support/Steam");
        assert_eq!(paths[1], "/Volumes/SSD/Steam Library");
    }

    #[test]
    fn unescapes_windows_backslash_and_ignores_non_path() {
        // Windows vdf 路径双反斜杠；其它键（"pathid"）不得误匹配
        let vdf = "  \"path\"  \"D:\\\\Steam\"\n\"pathid\" \"123\"\n";
        let paths = parse_library_paths(vdf);
        assert_eq!(paths, vec!["D:\\Steam".to_string()]);
    }

    #[test]
    fn scans_all_libraries_and_skips_bad() {
        let _env = crate::ENV_LOCK.lock().unwrap();
        // 两个 Steam 库（主 + 第二），STEAM_DIR 指主库，libraryfolders.vdf 指向第二库
        let base = std::env::temp_dir().join(format!("gesso-we-multi-{}", gesso_core::generate_id()));
        let main = base.join("main");
        let second = base.join("second drive");
        fs::create_dir_all(main.join("steamapps")).unwrap();
        fs::create_dir_all(second.join("steamapps")).unwrap();

        put_entry(&main, "111",
            r#"{"type":"video","file":"a.mp4","title":"A Main Video"}"#, &["a.mp4"]);
        // 坏 project.json 条目（scan 应跳过，不阻断整轮）
        let bad = main.join("steamapps/workshop/content").join(APPID).join("444");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join("project.json"), b"not json").unwrap();
        put_entry(&second, "222",
            r#"{"type":"web","file":"index.html","title":"B Second Web"}"#, &["index.html"]);

        // vdf 登记第二库（路径含空格）
        let vdf = format!("\"libraryfolders\"\n{{\n  \"1\"\n  {{\n    \"path\" \"{}\"\n  }}\n}}\n",
            second.display());
        fs::write(main.join("steamapps/libraryfolders.vdf"), vdf).unwrap();

        unsafe { std::env::set_var("STEAM_DIR", &main); }
        let libs = find_libraries();
        assert_eq!(libs.len(), 2, "主库 + vdf 第二库");
        assert!(libs.contains(&second));

        let v = scan_all();
        // 坏条目 444 跳过；跨两库剩 2 个，标题排序 A,B
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].project.title, "A Main Video");
        assert_eq!(v[0].kind(), WeKind::Video);
        assert_eq!(v[1].project.title, "B Second Web");
        assert_eq!(v[1].kind(), WeKind::Web);
        // 第二库条目目录确实落在 second 库
        assert!(v[1].dir.starts_with(&second));

        unsafe { std::env::remove_var("STEAM_DIR"); }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn missing_workshop_returns_empty() {
        let _env = crate::ENV_LOCK.lock().unwrap();
        let base = std::env::temp_dir().join(format!("gesso-we-empty-{}", gesso_core::generate_id()));
        fs::create_dir_all(base.join("main/steamapps")).unwrap();
        unsafe { std::env::set_var("STEAM_DIR", base.join("main")); }
        assert!(scan_all().is_empty());
        unsafe { std::env::remove_var("STEAM_DIR"); }
        let _ = fs::remove_dir_all(&base);
    }
}
