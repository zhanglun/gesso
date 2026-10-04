//! 自绘业务小件（DESIGN.md：基础控件用 kit，业务组件自绘）。
//!
//! 角标 / 预览占位 / 状态图标 / 设置行 / 段控件 —— 信息密度与状态语义是本设计的。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::gpui::{
    div, linear_color_stop, linear_gradient, px, rgb, App, FontFeatures, Hsla, IntoElement,
    ParentElement, Styled,
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
/// - `thumbs` 非空：`img()` 加载 `thumbs[frame]`（video = 抽帧序列，悬停时
///   由调用方推进 frame 实现轮播；gif = 素材本身，GPUI 自动多帧播放）
/// - 空：渐变占位 + 类型图标（shader/html 的 M4 前形态 / 加载失败兜底）
/// - `broken`：灰底问号（素材失效永不白屏）
pub fn preview(
    art: Art,
    kind: Option<Kind>,
    broken: bool,
    thumbs: &[String],
    frame: usize,
    cx: &App,
) -> gpui_kit::gpui::AnyElement {
    let t = tokens(cx);

    // 1) 已失效 → 灰底问号
    if broken {
        return div()
            .w_full()
            .aspect_ratio(16. / 9.)
            .overflow_hidden()
            .relative()
            .rounded_t(px(11.))
            .bg(t.preview_frame)
            .child(
                div().size_full().flex().items_center().justify_center()
                    .text_color(t.text2)
                    .child(Icon::new(IconName::CircleQuestionMark).size_4()),
            )
            .into_any_element();
    }

    // 2) 有缩略图 → 显示缩略图帧
    if !thumbs.is_empty() {
        if let Some(path) = thumbs.get(frame.min(thumbs.len() - 1)) {
            if std::path::Path::new(path).exists() {
                return div()
                    .w_full()
                    .aspect_ratio(16. / 9.)
                    .overflow_hidden()
                    .relative()
                    .rounded_t(px(11.))
                    .bg(t.preview_bg)
                    .child({
                        use gpui_kit::gpui::StyledImage as _;
                        let source = gpui_kit::gpui::ImageSource::Resource(
                            gpui_kit::gpui::Resource::Path(std::path::PathBuf::from(path).into()),
                        );
                        gpui_kit::gpui::img(source)
                            .size_full()
                            .object_fit(gpui_kit::gpui::ObjectFit::Cover)
                            .rounded_t(px(11.))
                    })
                    .into_any_element();
            }
        }
    }

    // 3) 兜底：渐变底色 + 居中加载指示（缩略图后台生成中）
    let from: Hsla = rgb(art.from).into();
    let to: Hsla = rgb(art.to).into();
    div()
        .w_full()
        .aspect_ratio(16. / 9.)
        .overflow_hidden()
        .relative()
        .rounded_t(px(11.))
        .bg(linear_gradient(
            135.,
            linear_color_stop(from, 0.),
            linear_color_stop(to, 1.),
        ))
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size_8()
                        .rounded_full()
                        .bg(gpui_kit::gpui::black().opacity(0.30))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child({
                            // video：帧序列后台生成中 → loading；其余类型永不产帧 → 类型图标
                            let icon = if kind == Some(Kind::Video) {
                                IconName::RefreshCw
                            } else {
                                kind_icon(kind.unwrap_or(Kind::Video))
                            };
                            Icon::new(icon).size_4().text_color(gpui_kit::gpui::white())
                        }),
                ),
        )
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
        .min_h(px(40.))
        .gap_3()
        .items_center()
        .child(left)
        .child(control)
        .into_any_element()
}

/// Select 的行内槽位。kit 的 Select 根节点是 `size_full`：放进定高行（min_h +
/// items_center）会撑满整行高，而触发器在其内部贴顶排布，控件整体不再随行居中
/// （§4.5 表单行标签与控件错位）。槽位宽度撑满剩余行宽、高度塌缩到触发器自身，
/// 行级 items_center 对槽位整体居中。
pub fn select_slot(control: gpui_kit::gpui::AnyElement) -> gpui_kit::gpui::AnyElement {
    div().w_full().child(control).into_any_element()
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
