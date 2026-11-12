//! Wallpaper Engine 内容解析：**只在用户主动导入时**被调用。
//!
//! 我们绝不扫描用户磁盘、不枚举 Steam/工坊。用户通过导入对话框自己选中一个
//! WE 条目的 `project.json`（或整目录），本模块负责解析它；之后的使用走
//! `gesso://steam/<entry-id>/<rel>` 路由（见 protocol.rs）。
//!
//! WE 数据模型：`<条目目录>/project.json`，431960 = WE AppID。
//! scene 支持基础图片层实时渲染，application 明确拒绝。

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
            "scene" => WeKind::Scene,
            "application" => WeKind::Unsupported("application"),
            other => WeKind::UnsupportedStr(other.to_string()),
        }
    }

    /// scene 条目的场景文件相对路径（project.json 的 file，默认 scene.json）。
    pub fn scene_file(&self) -> String {
        if self.project.file.is_empty() {
            "scene.json".into()
        } else {
            self.project.file.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WeKind {
    Video,
    Web,
    /// scene 项目（一期：基础图层 + 关键帧动画 + 视差；效果栈二期）。
    Scene,
    /// 具名不支持（application）。
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

// ── scene 快速扫描（一期：识别阻断项与不支持项计数；不支持项随导入结果
// 返回给 UI，导入时如实告知）────

/// scene 扫描结果。
#[derive(Debug, Clone, Default)]
pub struct SceneScan {
    /// 图片层数（可渲染的部分）。
    pub layers: usize,
    /// 阻断项：存在则整张场景不渲染（导入应拒绝）。
    pub blockers: Vec<&'static str>,
    /// 不支持项计数（渲染时跳过，不影响其余部分）。键为稳定英文键，
    /// UI 按键本地化（strings::omission_label），本模块不掺文案。
    pub omissions: Vec<(&'static str, usize)>,
}

impl SceneScan {
    pub fn omissions_summary(&self) -> String {
        if self.omissions.is_empty() {
            return "无".into();
        }
        self.omissions
            .iter()
            .map(|(n, c)| {
                if *c > 1 {
                    format!("{n}×{c}")
                } else {
                    n.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// 条目内资源路径：拒绝穿越、绝对路径和逃出条目根目录的符号链接。
fn safe_file(dir: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains('\\') {
        return None;
    }
    let rel_path = Path::new(rel);
    if rel_path
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    let root = dir.canonicalize().ok()?;
    let file = root.join(rel_path).canonicalize().ok()?;
    file.starts_with(&root).then_some(file)
}

/// 读取条目目录内的 JSON 文件（深度和文件大小受限）。
fn read_json(dir: &Path, rel: &str, depth: usize) -> Option<serde_json::Value> {
    if depth > 4 {
        return None;
    }
    let file = safe_file(dir, rel)?;
    let meta = std::fs::metadata(&file).ok()?;
    if !meta.is_file() || meta.len() > 4 * 1024 * 1024 {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 快速扫描 scene 项目（只读 scene.json + 图层 model json，深度受限）。
/// 安全模型：工坊内容 = 不可信输入；路径包含校验 + 读取深度上限。
pub fn scene_scan(dir: &Path, scene_file: &str) -> Option<SceneScan> {
    let root: serde_json::Value = read_json(dir, scene_file, 0)?;
    let general = root.get("general");
    let mut scan = SceneScan::default();

    // 画布尺寸来自正交投影；缺失 = 3D 相机场景（一期不支持整张拒绝）
    let ortho = general
        .and_then(|g| g.get("orthogonalprojection"))
        .and_then(|o| o.get("width").and_then(|w| o.get("height").map(|h| (w, h))));
    let Some((w, h)) = ortho else {
        scan.blockers.push("3D 相机场景");
        return Some(scan);
    };
    let (w, h) = (w.as_f64().unwrap_or(0.0), h.as_f64().unwrap_or(0.0));
    if !(1.0..=8192.0).contains(&w) || !(1.0..=8192.0).contains(&h) {
        scan.blockers.push("场景尺寸异常");
        return Some(scan);
    }

    // 不支持项计数：同键累加，n 为本次计数
    let omission = |scan: &mut SceneScan, key: &'static str, n: usize| {
        if let Some((_, c)) = scan.omissions.iter_mut().find(|(k, _)| *k == key) {
            *c += n;
        } else {
            scan.omissions.push((key, n));
        }
    };
    let blocker = |scan: &mut SceneScan, name: &'static str| {
        if !scan.blockers.contains(&name) {
            scan.blockers.push(name);
        }
    };

    for obj in root
        .get("objects")
        .and_then(|o| o.as_array())
        .into_iter()
        .flatten()
    {
        if obj.get("visible").and_then(|v| v.as_bool()) == Some(false) {
            continue;
        }
        // image 层：model json 里带 puppet = 阻断（画出来会是部件图集）。
        if let Some(model_rel) = obj.get("image").and_then(|v| v.as_str()) {
            scan.layers += 1;
            let Some(model) = read_json(dir, model_rel, 1) else {
                blocker(&mut scan, "图片层资源缺失");
                continue;
            };
            if model.get("puppet").is_some() {
                blocker(&mut scan, "puppet-warp 角色");
                continue;
            }
            let Some(material_rel) = model.get("material").and_then(|v| v.as_str()) else {
                blocker(&mut scan, "图片材质缺失");
                continue;
            };
            let Some(material) = read_json(dir, material_rel, 2) else {
                blocker(&mut scan, "图片材质缺失");
                continue;
            };
            let Some(pass) = material
                .get("passes")
                .and_then(|v| v.as_array())
                .and_then(|v| v.first())
            else {
                blocker(&mut scan, "图片材质缺失");
                continue;
            };
            if let Some(texture) = pass
                .get("textures")
                .and_then(|v| v.as_array())
                .and_then(|v| v.first())
                .and_then(|v| v.as_str())
            {
                if !texture.starts_with("_rt_") && safe_file(dir, texture).is_none() {
                    blocker(&mut scan, "图片纹理缺失");
                }
            }
            // 效果链计数（一期不支持效果渲染）
            if let Some(effects) = obj.get("effects").and_then(|e| e.as_array()) {
                let n = effects.len();
                if n > 0 {
                    omission(&mut scan, "effects", n);
                }
            }
        } else if obj.get("particle").is_some() {
            omission(&mut scan, "particles", 1);
        } else if obj.get("text").is_some() {
            omission(&mut scan, "text", 1);
        } else if obj.get("sound").is_some() {
            omission(&mut scan, "sound", 1);
        } else if obj.get("model").is_some() {
            omission(&mut scan, "3d-model", 1);
        }
    }
    Some(scan)
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

    #[test]
    fn scene_scan_classifies_layers_blockers_and_omissions() {
        let base =
            std::env::temp_dir().join(format!("gesso-we-scan-{}", gesso_core::generate_id()));
        let dir = base.join("777");
        fs::create_dir_all(dir.join("materials")).unwrap();
        fs::write(
            dir.join("project.json"),
            r#"{"type":"scene","file":"scene.json","title":"S"}"#,
        )
        .unwrap();
        fs::write(
            dir.join("scene.json"),
            r#"{
              "general": {"orthogonalprojection": {"width": 1920, "height": 1080}},
              "objects": [
                {"image": "materials/l1.json"},
                {"image": "materials/l2.json", "effects": [{"file": "e1.json"}, {"file": "e2.json"}]},
                {"particle": "p.json"},
                {"text": "t"},
                {"sound": "s.json"},
                {"model": "m.glb"}
              ]
            }"#,
        )
        .unwrap();
        fs::write(
            dir.join("materials/l1.json"),
            r#"{"material": "materials/m1.json", "width": 100, "height": 50}"#,
        )
        .unwrap();
        fs::write(
            dir.join("materials/l2.json"),
            r#"{"material": "materials/m2.json"}"#,
        )
        .unwrap();
        fs::write(
            dir.join("materials/m1.json"),
            r#"{"passes": [{"textures": ["textures/a.png"]}]}"#,
        )
        .unwrap();
        fs::write(
            dir.join("materials/m2.json"),
            r#"{"passes": [{"textures": ["textures/b.png"]}]}"#,
        )
        .unwrap();
        fs::create_dir_all(dir.join("textures")).unwrap();
        fs::write(dir.join("textures/a.png"), b"png").unwrap();
        fs::write(dir.join("textures/b.png"), b"png").unwrap();

        let e = import_we_at(&dir, "777").unwrap();
        assert_eq!(e.kind(), WeKind::Scene);
        assert_eq!(e.scene_file(), "scene.json");

        let scan = scene_scan(&dir, &e.scene_file()).unwrap();
        assert_eq!(scan.layers, 2);
        assert!(scan.blockers.is_empty());
        let find = |n: &str| {
            scan.omissions
                .iter()
                .find(|(x, _)| *x == n)
                .map(|(_, c)| *c)
        };
        assert_eq!(find("effects"), Some(2));
        assert_eq!(find("particles"), Some(1));
        assert_eq!(find("text"), Some(1));
        assert_eq!(find("sound"), Some(1));
        assert_eq!(find("3d-model"), Some(1));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn scene_scan_rejects_unsafe_paths() {
        let base =
            std::env::temp_dir().join(format!("gesso-we-path-{}", gesso_core::generate_id()));
        fs::create_dir_all(&base).unwrap();
        fs::write(
            base.join("scene.json"),
            r#"{"general":{"orthogonalprojection":{"width":1,"height":1}}}"#,
        )
        .unwrap();
        assert!(scene_scan(&base, "../scene.json").is_none());
        assert!(scene_scan(&base, "scene\\.json").is_none());
        let outside = base.with_extension("outside.json");
        fs::write(&outside, b"{}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, base.join("link.json")).unwrap();
        #[cfg(unix)]
        assert!(scene_scan(&base, "link.json").is_none());
        let _ = fs::remove_file(outside);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn scene_scan_blocks_puppet_and_3d_camera() {
        let base =
            std::env::temp_dir().join(format!("gesso-we-block-{}", gesso_core::generate_id()));
        let dir = base.join("888");
        fs::create_dir_all(dir.join("materials")).unwrap();
        fs::write(
            dir.join("project.json"),
            r#"{"type":"scene","file":"scene.json"}"#,
        )
        .unwrap();
        // 3D 相机（无正交投影）
        fs::write(
            dir.join("scene.json"),
            r#"{"general": {}, "objects": [{"image": "a.json"}]}"#,
        )
        .unwrap();
        fs::write(dir.join("a.json"), r#"{}"#).unwrap();
        let scan = scene_scan(&dir, "scene.json").unwrap();
        assert_eq!(scan.blockers, vec!["3D 相机场景"]);

        // puppet 角色
        fs::write(
            dir.join("scene.json"),
            r#"{
              "general": {"orthogonalprojection": {"width": 1920, "height": 1080}},
              "objects": [{"image": "materials/p.json"}]
            }"#,
        )
        .unwrap();
        fs::write(
            dir.join("materials/p.json"),
            r#"{"material": "m.json", "puppet": {}}"#,
        )
        .unwrap();
        let scan = scene_scan(&dir, "scene.json").unwrap();
        assert_eq!(scan.blockers, vec!["puppet-warp 角色"]);

        let _ = fs::remove_dir_all(&base);
    }
}
