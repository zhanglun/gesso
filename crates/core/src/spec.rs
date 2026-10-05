//! ContentSpec —— 内容库与壁纸宿主页之间的唯一契约（技术方案 §5.1）。

use serde::{Deserialize, Serialize};

/// 壁纸内容类型，决定宿主页分派哪个渲染器。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperKind {
    Video,
    Image,
    Shader,
    Html,
}

/// 填充方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fit {
    Cover,
    Contain,
    Fill,
}

/// 素材元信息。`origin` 标记来源；Wallpaper Engine 导入项固定为
/// `"wallpaper-engine"`（普通导入为 `"local"`、内置样例为 `"builtin"`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecMeta {
    pub title: String,
    pub origin: String,
}

/// 内容规格：宿主页收到它即完成一次渲染器切换，不携带任何其他隐式状态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentSpec {
    pub kind: WallpaperKind,
    /// `wallpaper://<条目id>/<相对路径>`；宿主页唯一可访问的地址空间。
    pub source: String,
    pub fit: Fit,
    pub fps_cap: u8,
    pub audio: AudioPolicy,
    pub meta: SpecMeta,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioPolicy {
    /// 壁纸默认静音（技术方案 §5.2）—— `#[default]` 显式承载该语义。
    #[default]
    Muted,
    On,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_json_roundtrip() {
        let spec = ContentSpec {
            kind: WallpaperKind::Shader,
            source: "wallpaper://ab12cd34/main.glsl".into(),
            fit: Fit::Cover,
            fps_cap: 60,
            audio: AudioPolicy::Muted,
            meta: SpecMeta {
                title: "waves".into(),
                origin: "local".into(),
            },
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains(r#""kind":"shader""#));
        let back: ContentSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(spec, back);
    }
}
