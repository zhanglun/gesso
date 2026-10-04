//! 视觉主题：DESIGN.md 冻结 token → gpui-kit 语义主题（亮/暗双套）。
//!
//! 分两层：
//! 1. `apply` 把 DESIGN.md 的九个语义色映射进 kit `ThemeColor`（kit 组件用）；
//! 2. `Tokens` 暴露原型 CSS 变量的完整集合（自绘业务件用），按当前模式即时取值。
//!
//! 系统外观跟随：启动时 `Theme::sync_system_appearance`，窗口收到外观变化事件后重放。

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::gpui::{rgb, rgba, App, BorrowAppContext as _, Hsla, Rgba, Window};

/// 原型 CSS 变量的完整集合（DESIGN.md「Tokens」表；自绘组件专用）。
#[derive(Clone, Copy)]
pub struct Tokens {
    pub surface: Hsla,
    pub panel: Hsla,
    pub elevated: Hsla,
    pub text1: Hsla,
    pub text2: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub danger: Hsla,
    pub danger_soft: Hsla,
    pub hairline: Hsla,
    pub hairline2: Hsla,
    /// 卡片/屏预览的底色（唯一的高饱和内容区之外的暗面）。
    pub preview_bg: Hsla,
    pub preview_frame: Hsla,
}

impl Tokens {
    pub fn light() -> Self {
        Tokens {
            surface: rgb(0xFFFFFF).into(),
            panel: rgb(0xF5F5F7).into(),
            elevated: rgb(0xFFFFFF).into(),
            text1: rgb(0x1D1D1F).into(),
            text2: rgb(0x6E6E73).into(),
            accent: rgb(0x316EF5).into(),
            accent_soft: rgba(0x316EF51F).into(), // 蓝 12%
            danger: rgb(0xD33A3A).into(),
            danger_soft: rgba(0xD33A3A1A).into(), // 红 10%
            hairline: rgb(0xE5E5EA).into(),
            hairline2: rgb(0xD8D8DE).into(),
            preview_bg: rgb(0x0E0F13).into(),
            preview_frame: rgb(0x26262A).into(),
        }
    }

    pub fn dark() -> Self {
        Tokens {
            surface: rgb(0x1E1E20).into(),
            panel: rgb(0x26262A).into(),
            elevated: rgb(0x303034).into(),
            text1: rgb(0xEBEBED).into(),
            text2: rgb(0x9B9BA1).into(),
            accent: rgb(0x5B8DEF).into(),
            accent_soft: rgba(0x5B8DEF2E).into(), // 蓝 18%
            danger: rgb(0xE5545B).into(),
            danger_soft: rgba(0xE5545B24).into(), // 红 14%
            hairline: rgb(0x3A3A3F).into(),
            hairline2: rgb(0x4A4A50).into(),
            preview_bg: rgb(0x0E0F13).into(),
            preview_frame: rgb(0x303034).into(),
        }
    }

    pub fn for_mode(mode: ThemeMode) -> Self {
        if mode.is_dark() {
            Tokens::dark()
        } else {
            Tokens::light()
        }
    }
}

/// 当前模式下的设计 token（自绘组件统一从这里取色）。
pub fn tokens(cx: &App) -> Tokens {
    Tokens::for_mode(cx.theme().mode)
}

/// 把 DESIGN.md token 注入 kit 语义主题；在启动、系统外观变化、手动切换时调用。
pub fn apply(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None, cx);
    let t = Tokens::for_mode(mode);
    cx.update_global::<Theme, _>(|theme, _| {
        let c = &mut theme.colors;
        // 底与文本
        c.background = t.surface;
        c.foreground = t.text1;
        c.border = t.hairline;
        c.window_border = t.hairline;
        c.input = t.panel;
        c.muted = t.panel;
        c.muted_foreground = t.text2;
        // 弹出层（右键菜单/下拉/通知）
        c.popover = t.elevated;
        c.popover_foreground = t.text1;
        c.overlay = rgba(0x00000073u32).into(); // 原型遮罩 rgba(0,0,0,.45)
                                                // 强调色：kit 的 primary = 我们的主操作；kit 的 accent = 我们的 accent/soft
                                                // （kit 中 accent 语义是 MenuItem/ListItem 悬停底，恰好对应选中底/悬停底）
        c.primary = t.accent;
        c.primary_foreground = rgb(0xFFFFFF).into();
        c.primary_hover = shift(t.accent, 0.08);
        c.primary_active = shift(t.accent, -0.05);
        c.accent = t.accent_soft;
        c.accent_foreground = t.text1;
        c.link = t.accent;
        c.ring = t.accent;
        c.selection = t.accent_soft;
        c.drop_target = t.accent_soft;
        // 次按钮 = panel 底 + hairline 边（§3 全局组件规格）
        c.secondary = t.panel;
        c.secondary_foreground = t.text1;
        c.secondary_hover = t.elevated;
        c.secondary_active = t.panel;
        // 危险色只表真故障（§1.1 规则）
        c.danger = t.danger;
        c.danger_foreground = rgb(0xFFFFFF).into();
        c.danger_hover = t.danger_soft;
        // 页签（顶栏三页签 + 筛选段控件）
        c.tab_bar = t.surface;
        c.tab_bar_segmented = t.panel;
        c.tab = t.surface;
        c.tab_foreground = t.text2;
        c.tab_active = t.elevated;
        c.tab_active_foreground = t.text1;
        // 顶栏/状态条
        c.title_bar = t.surface;
        c.title_bar_border = t.hairline;
        c.status_bar = t.surface;
        c.status_bar_border = t.hairline;
        // 开关状态色 = accent（§3）
        c.switch = t.accent;
        c.switch_thumb = rgb(0xFFFFFF).into();
        // 列表/卡片悬停
        c.list = t.surface;
        c.list_hover = t.accent_soft;
        c.list_active = t.accent_soft;
        c.list_active_border = t.accent;
    });
}

/// 等比提亮/压暗（对应原型 brightness 滤镜的近似）。
fn shift(c: Hsla, amount: f32) -> Hsla {
    let rgba = Rgba::from(c);
    let f = 1. + amount;
    let ch = |v: f32| ((v * 255.).clamp(0., 255.) * f).round().min(255.) as u32;
    let hex = (ch(rgba.r) << 16) | (ch(rgba.g) << 8) | ch(rgba.b);
    rgb(hex).into()
}

/// 启动时初始化：跟随系统外观。
pub fn init(cx: &mut App) {
    let mode = ThemeMode::from(cx.window_appearance());
    apply(mode, cx);
}

/// 窗口外观变化时重放（亮暗跟随系统）。
pub fn sync_on_appearance_change(window: &mut Window, cx: &mut App) {
    let mode = ThemeMode::from(window.appearance());
    apply(mode, cx);
}

/// 手动切换（顶栏月亮按钮，便于验收双主题；正式版仅跟随系统）。
pub fn toggle(cx: &mut App) {
    let next = if cx.theme().mode.is_dark() {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    apply(next, cx);
}
