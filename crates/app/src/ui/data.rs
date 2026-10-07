//! 领域视图类型（真源在 gesso-core；本模块只保留 UI 投影与常量）。
//!
//! 数据结构（类型定义），不含运行时数据。

use gpui_kit::SharedString;

/// 壁纸内容类型（§1 能力：video / 动图(GIF·WebP) / 静态图片 / shader / html）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Video,
    Gif,
    Photo,
    Shader,
    Web,
}

impl Kind {
    /// 角标文案与筛选段控件的标签一致。
    pub fn label(self) -> &'static str {
        match self {
            Kind::Video => "VIDEO",
            Kind::Gif => "GIF",
            Kind::Photo => "PHOTO",
            Kind::Shader => "SHADER",
            Kind::Web => "WEB",
        }
    }

    /// 筛选段控件用的本地化名（文案唯一出处 = strings，语言切换实时生效）。
    pub fn filter_label(self) -> &'static str {
        match self {
            Kind::Video => super::strings::KIND_VIDEO(),
            Kind::Gif => super::strings::KIND_GIF(),
            Kind::Photo => super::strings::KIND_PHOTO(),
            Kind::Shader => super::strings::KIND_SHADER(),
            Kind::Web => super::strings::KIND_WEB(),
        }
    }
}

/// 预览配色渐变（M4 起换抽帧真图；shader/html 保留图标占位）。
#[derive(Clone, Copy)]
pub struct Art {
    pub from: u32,
    pub to: u32,
}

/// 内容库条目（视图投影；真源在 gesso-core library manifest）。
#[derive(Clone)]
pub struct LibraryItem {
    pub id: SharedString,
    pub name: SharedString,
    pub kind: Kind,
    /// 来源为 Wallpaper Engine 工坊引用（只读）。
    pub we: bool,
    /// 远端网页条目（origin=url）：无本地目录，「打开所在目录」置灰。
    pub remote: bool,
    /// meta 行：分辨率 · 时长/帧率（tabular 数字）。
    pub meta: SharedString,
    /// 指派到的显示器下标；None = 未指派。
    pub assigned: Option<usize>,
    /// 素材失效（文件已移除）——danger 只表真故障。
    pub broken: bool,
    /// 真实库条目（gesso-core LibraryEntry）。
    pub real: bool,
    pub art: Art,
    /// 预览帧序列（video = 抽帧 thumb.png + thumb-1..7.png；gif = 素材本身；
    /// 空 = 渐变占位）。悬停时 UI 轮播这些帧。
    pub thumbs: Vec<String>,
}

/// 运行状态（技术方案 §9 状态机的视图投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayState {
    Playing,
    UserPaused,
    FullscreenPaused,
    /// 电池供电自动暂停（§5 状态矩阵；数据桥接入前的预留态）。
    #[allow(dead_code)]
    BatteryPaused,
}

impl PlayState {
    pub fn paused(self) -> bool {
        !matches!(self, PlayState::Playing)
    }
}

/// 显示器条目（视图投影；稳定 ID 在 core 用 EDID 哈希，UI 只见名字）。
#[derive(Clone)]
pub struct MonitorEntry {
    pub name: SharedString,
    pub real_id: String,
    /// 托盘/状态行用的短名（"→ 主屏"）。
    pub short: SharedString,
    /// "27″ · 3840×2160"（tabular）。
    pub label: SharedString,
    /// 拓扑图用的工作区矩形（逻辑像素 x/y/w/h）。
    pub rect: (f32, f32, f32, f32),
    /// 系统主显示器（详情条角标 accent 依据）。
    pub is_main: bool,
    pub wallpaper: Option<SharedString>,
    pub state: PlayState,
    pub fps: u32,
}

/// 全屏/电池时的策略（设置页下拉）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SuspendPolicy {
    Pause,
    Downclock,
    Ignore,
}

impl SuspendPolicy {
    /// 设置页下拉选项（文案唯一出处 = strings）。
    pub fn label(self) -> &'static str {
        match self {
            SuspendPolicy::Pause => super::strings::POLICY_PAUSE(),
            SuspendPolicy::Downclock => super::strings::POLICY_DOWNCLOCK(),
            SuspendPolicy::Ignore => super::strings::POLICY_IGNORE(),
        }
    }
}

/// 设置项（全部即时生效；真源 = core AppConfig.settings，快照映射注入）。
#[derive(Clone)]
pub struct Settings {
    pub fps_cap: u32,
    pub fullscreen: SuspendPolicy,
    pub battery: SuspendPolicy,
    pub idle_downclock: bool,
    pub autolaunch: bool,
    pub startup_random: bool,
    pub weather_custom_key: bool,
    pub weather_key: String,
    /// 界面语言（0=auto, 1=zh, 2=en）。
    pub language: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            fps_cap: 60,
            fullscreen: SuspendPolicy::Pause,
            battery: SuspendPolicy::Pause,
            idle_downclock: true,
            autolaunch: true,
            startup_random: false,
            weather_custom_key: false,
            weather_key: String::new(),
            language: 0,
        }
    }
}

impl Settings {
    /// UI 投影 → core 真源（设置页写回经 EngineAction::UpdateSettings 落盘）。
    pub fn to_core_settings(&self) -> gesso_core::Settings {
        let policy = |p: SuspendPolicy| match p {
            SuspendPolicy::Pause => gesso_core::PausePolicy::Pause,
            SuspendPolicy::Downclock => gesso_core::PausePolicy::Downscale,
            SuspendPolicy::Ignore => gesso_core::PausePolicy::Ignore,
        };
        gesso_core::Settings {
            fps_cap_default: self.fps_cap as u8,
            fullscreen_policy: policy(self.fullscreen),
            battery_policy: policy(self.battery),
            idle_downscale: self.idle_downclock,
            autostart: self.autolaunch,
            startup_behavior: if self.startup_random {
                gesso_core::StartupBehavior::Random
            } else {
                gesso_core::StartupBehavior::RestoreLast
            },
            language: match self.language {
                1 => gesso_core::config::Language::Zh,
                2 => gesso_core::config::Language::En,
                _ => gesso_core::config::Language::Auto,
            },
        }
    }
}

pub const FPS_OPTIONS: [u32; 4] = [60, 30, 15, 5];
