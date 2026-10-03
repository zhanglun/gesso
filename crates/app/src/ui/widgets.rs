//! 自绘业务小件（DESIGN.md：基础控件用 kit，业务组件自绘）。
//!
//! 角标 / 预览占位 / 状态图标 / 设置行 / 段控件 —— 信息密度与状态语义是本设计的。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::gpui::{
    div, linear_color_stop, linear_gradient, px, rgb, rgba, App, FontFeatures, Hsla,
    InteractiveElement as _, IntoElement, ParentElement, Styled,
};

use super::data::{Art, Kind, PlayState};
use super::theme::tokens;

/// 数字等宽（§1.2：FPS/时长/分辨率一律 tabular-nums）。
pub fn tabular() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

/// 角标（§3：10-11px 大写，hairline 描边胶囊；WE 角标 accent 描边）。
pub fn badge(label: &str, accent: bool, cx: &App) -> gpui_kit::gpui::AnyElement {
    let t = tokens(cx);
    let (fg, border) = if accent {
        (t.accent, t.accent)
    } else {
        (t.text2, t.hairline)
    };
    div()
        .px_1()
        .text_size(px(10.))
        .line_height(px(16.))
        .text_color(fg)
        .border_1()
        .border_color(border)
        .rounded(px(4.))
        .child(label.to_string())
        .into_any_element()
}

/// 类型对应的线性图标（§1.3：16px 单色线性；禁 emoji）。
pub fn kind_icon(kind: Kind) -> IconName {
    match kind {
        Kind::Video => IconName::Film,
        Kind::Gif => IconName::Image,
        Kind::Shader => IconName::Sparkles,
        Kind::Web => IconName::Globe,
    }
}

/// 16:9 预览区（撑满父容器宽度）。
///
/// - `thumb` 有值：`img()` 加载真图（video = 导入时抽帧的 thumb.png；
///   gif/webp = 素材本身，GPUI 自动多帧播放）
/// - 否则：渐变占位 + 类型图标（shader/html 的 M4 前形态 / 加载失败兜底）
/// - `broken`：灰底问号（素材失效永不白屏）
pub fn preview(
    art: Art,
    kind: Option<Kind>,
    broken: bool,
    chip: bool,
    thumb: Option<&str>,
    cx: &App,
) -> gpui_kit::gpui::AnyElement {
    let t = tokens(cx);
    let base = if broken { t.preview_frame } else { t.preview_bg };

    // 真图路径（存在且未失效）
    if !broken {
        if let Some(path) = thumb {
            if std::path::Path::new(path).exists() {
                let mut frame = div()
                    .w_full()
                    .aspect_ratio(16. / 9.)
                    .overflow_hidden()
                    .relative()
                    .bg(base)
                    .child(
                        {
                            use gpui_kit::gpui::StyledImage as _;
                            gpui_kit::gpui::img(path)
                                .size_full()
                                .object_fit(gpui_kit::gpui::ObjectFit::Cover)
                        }
                    );
                frame = if chip {
                    frame.child(play_chip())
                } else {
                    frame
                };
                return frame.into_any_element();
            }
        }
    }

    let (from, to) = if broken {
        (base, base)
    } else {
        (rgb(art.from).into(), rgb(art.to).into())
    };
    let frame = div()
        .w_full()
        .aspect_ratio(16. / 9.)
        .overflow_hidden()
        .relative()
        .bg(linear_gradient(
            135.,
            linear_color_stop(from, 0.),
            linear_color_stop(to, 1.),
        ))
        .child(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.text2)
                .child(match (broken, kind) {
                    (true, _) => Icon::new(IconName::CircleQuestionMark)
                        .size_4()
                        .into_any_element(),
                    (false, Some(k)) => Icon::new(kind_icon(k)).size_4().into_any_element(),
                    (false, None) => div().into_any_element(),
                }),
        );
    let frame = if chip {
        frame.child(play_chip())
    } else {
        frame
    };
    frame.into_any_element()
}

/// 视频/动图缩略图路径：条目目录 thumb.png（不存在时惰性生成）。
/// 生成用系统 qlmanage（macOS 自带 QuickLook，无第三方依赖）；
/// 失败静默回退渐变占位（预览失败不该打扰用户）。
pub fn ensure_thumb(source_dir: &str, kind: Kind) -> Option<String> {
    let dir = std::path::Path::new(source_dir);
    let thumb = dir.join("thumb.png");
    if thumb.exists() {
        return Some(thumb.display().to_string());
    }
    // gif/webp 素材本身就是图，直接用原文件
    if matches!(kind, Kind::Gif) {
        let src = dir.join("index.gif");
        if src.exists() {
            return Some(src.display().to_string());
        }
        return None;
    }
    if kind != Kind::Video {
        return None; // shader/html 无首帧概念（M4 换截图序列）
    }
    let src = dir.join("index.mp4");
    if !src.exists() {
        return None;
    }
    let out = std::process::Command::new("/usr/bin/qlmanage")
        .args(["-t", "-s", "480", "-o", &dir.display().to_string()])
        .arg(&src)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    if !out.success() {
        return None;
    }
    // qlmanage 产物名 = <源文件名>.png → 统一改名 thumb.png
    let produced = dir.join("index.mp4.png");
    if produced.exists() {
        std::fs::rename(&produced, &thumb).ok()?;
        Some(thumb.display().to_string())
    } else {
        None
    }
}

/// 悬停时的播放角标（play-chip：24×24 黑底圆角，卡片悬停时浮现）。
pub fn play_chip() -> gpui_kit::gpui::AnyElement {
    div()
        .absolute()
        .right_2()
        .bottom_2()
        .size(px(24.))
        .rounded(px(6.))
        .bg(rgba(0x0000008Cu32)) // .55
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgba(0xFFFFFFFFu32))
        .opacity(0.)
        .group_hover("card", |s| s.opacity(1.))
        .child(Icon::new(IconName::Play).size_3())
        .into_any_element()
}

/// 运行状态 →（图标，文案，颜色）：§5 跨屏状态矩阵的视图投影。
pub fn play_state_visual(state: PlayState, cx: &App) -> (IconName, &'static str, Hsla) {
    let t = tokens(cx);
    match state {
        PlayState::Playing => (IconName::Play, super::strings::ST_PLAYING, t.accent),
        PlayState::UserPaused => (IconName::Pause, super::strings::ST_PAUSED, t.text2),
        PlayState::FullscreenPaused => (IconName::Maximize, super::strings::ST_FULLSCREEN, t.text2),
        PlayState::BatteryPaused => (IconName::Zap, super::strings::ST_BATTERY, t.text2),
    }
}

/// 设置页的一行（label + 可选 desc 左，控件右；min-h 34，§4.5 分组表单）。
pub fn set_row(
    label: &'static str,
    desc: Option<&'static str>,
    control: gpui_kit::gpui::AnyElement,
    cx: &App,
) -> gpui_kit::gpui::AnyElement {
    let t = tokens(cx);
    let mut left = v_flex().flex_1().gap_1().child(
        div()
            .text_size(px(13.))
            .text_color(t.text1)
            .child(label.to_string()),
    );
    if let Some(d) = desc {
        left = left.child(
            div()
                .text_size(px(12.))
                .text_color(t.text2)
                .child(d.to_string()),
        );
    }
    h_flex()
        .min_h(px(34.))
        .gap_3()
        .items_center()
        .child(left)
        .child(control)
        .into_any_element()
}

/// 空状态插画位（简线风格：虚线圆角框 + 图标）。
pub fn empty_art(icon: IconName, cx: &App) -> gpui_kit::gpui::AnyElement {
    let t = tokens(cx);
    div()
        .size(px(72.))
        .rounded(px(16.))
        .border_1()
        .border_dashed()
        .border_color(t.hairline2)
        .flex()
        .items_center()
        .justify_center()
        .text_color(t.text2)
        .child(Icon::new(icon).size_7())
        .into_any_element()
}
