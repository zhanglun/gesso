//! 内容类型描述表 —— "类型 ↔ 扩展名 ↔ MIME ↔ 缩略图方式"的**单一事实源**。
//!
//! 历史教训：这套知识曾被复制到导入分类 / 主资源白名单 / 默认扩展名 /
//! 缩略图直引清单 / MIME 表 / 文件对话框 filter 共 6 处，加一个扩展名要
//! 同时改 6 个地方，漏一处就出现"能导入但无缩略图 / 角标永远为空"（M6
//! 连续踩中两次）。现在每种 `WallpaperKind` 只在此声明一次，其余全部查表。

use crate::WallpaperKind;

/// 缩略图来源策略。调度层据此决定"是否生成 / 起哪种任务"，无需按类型分叉。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbStrategy {
    /// 源文件即预览，不生成任何帧（静态图/动图）。
    Direct,
    /// 后台视频抽帧。
    Extract,
    /// 主线程 webview 截图（shader/html）。
    Capture,
}

/// 一种内容类型的完整描述（不可变常量，按 `WallpaperKind` 查表）。
#[derive(Debug, Clone, Copy)]
pub struct ContentType {
    /// 该类型可接受的源资源扩展名（小写，含点号之外的部分）。
    /// 主资源发现只在此集合内匹配 —— 条目目录还含宿主页 `index.html`，
    /// 白名单能防止把异名文件误判为主资源。
    pub extensions: &'static [&'static str],
    /// 类型默认扩展名（必须出现在 `extensions` 中）。
    pub default_ext: &'static str,
    /// 缩略图来源策略。
    pub thumb: ThumbStrategy,
}

/// 按内容类型查描述。
pub fn content_type(kind: WallpaperKind) -> ContentType {
    match kind {
        WallpaperKind::Video => ContentType {
            // mkv 在扩展名集合内（主资源发现），但导入被 classify 显式拒绝
            extensions: &["mp4", "webm", "mkv"],
            default_ext: "mp4",
            thumb: ThumbStrategy::Extract,
        },
        WallpaperKind::Image => ContentType {
            extensions: &["gif", "webp", "jpg", "jpeg", "png", "avif"],
            default_ext: "gif",
            thumb: ThumbStrategy::Direct,
        },
        WallpaperKind::Shader => ContentType {
            extensions: &["glsl"],
            default_ext: "glsl",
            thumb: ThumbStrategy::Capture,
        },
        WallpaperKind::Html => ContentType {
            extensions: &["html"],
            default_ext: "html",
            thumb: ThumbStrategy::Capture,
        },
    }
}

/// 扩展名 → MIME（资源协议按文件扩展名回写 Content-Type）。
/// 只覆盖本引擎认识的扩展名；不认识返回 None。
pub fn mime_for_ext(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "html" | "htm" => Some("text/html; charset=utf-8"),
        "js" => Some("application/javascript"),
        "css" => Some("text/css"),
        "json" => Some("application/json"),
        "mp4" | "m4v" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mkv" => Some("video/x-matroska"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "avif" => Some("image/avif"),
        "glsl" => Some("text/plain"),
        _ => None,
    }
}

/// 扩展名 → 内容类型（仅匹配可导入的扩展名；不识别返回 None）。
pub fn kind_from_ext(ext: &str) -> Option<WallpaperKind> {
    let ext = ext.to_ascii_lowercase();
    [
        WallpaperKind::Video,
        WallpaperKind::Image,
        WallpaperKind::Shader,
        WallpaperKind::Html,
    ]
    .into_iter()
    .find(|k| content_type(*k).extensions.contains(&ext.as_str()))
}

/// 该扩展名是否为"会动的图片"（gif / webp）。UI 据此把底层同一
/// `WallpaperKind::Image` 细分为动图与静态图片。
pub fn is_animated_image_ext(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), "gif" | "webp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ext_is_always_declared() {
        for k in [
            WallpaperKind::Video,
            WallpaperKind::Image,
            WallpaperKind::Shader,
            WallpaperKind::Html,
        ] {
            let c = content_type(k);
            assert!(
                c.extensions.contains(&c.default_ext),
                "{k:?} 的 default_ext 必须在 extensions 内"
            );
        }
    }

    #[test]
    fn every_declared_ext_resolves_back_to_kind() {
        // 扩展名表与 kind_from_ext 互为逆：声明的扩展名都能反查
        for k in [
            WallpaperKind::Video,
            WallpaperKind::Image,
            WallpaperKind::Shader,
            WallpaperKind::Html,
        ] {
            for ext in content_type(k).extensions {
                assert_eq!(kind_from_ext(ext), Some(k), "{ext} 应反查到 {k:?}");
            }
        }
        assert_eq!(kind_from_ext("exe"), None);
        assert_eq!(kind_from_ext("MOV"), None, "mov 不在视频白名单");
    }

    #[test]
    fn every_declared_ext_has_mime() {
        for k in [
            WallpaperKind::Video,
            WallpaperKind::Image,
            WallpaperKind::Shader,
            WallpaperKind::Html,
        ] {
            for ext in content_type(k).extensions {
                assert!(mime_for_ext(ext).is_some(), "{ext} 缺少 MIME");
            }
        }
    }

    #[test]
    fn animated_image_split() {
        assert!(is_animated_image_ext("gif"));
        assert!(is_animated_image_ext("WEBP"));
        assert!(!is_animated_image_ext("png"));
        assert!(!is_animated_image_ext("jpg"));
    }
}
