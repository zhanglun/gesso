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

/// 16:9 预览区（撑满父容器宽度）。真实实现 = 导入时预生成的抽帧循环
/// （界面与交互设计 §6）；页面开发期用演示渐变 + 类型图标表达，构图与原型一致。
pub fn preview(
    art: Art,
    kind: Option<Kind>,
    broken: bool,
    chip: bool,
    cx: &App,
) -> gpui_kit::gpui::AnyElement {
    let t = tokens(cx);
    let base = if broken {
        t.preview_frame
    } else {
        t.preview_bg
    };
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
