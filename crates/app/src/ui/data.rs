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

/// 预览配色渐变（缩略图就绪前的占位；失败兜底用类型图标）。
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
    /// 缩略图生成中（调度器在途/仍会重试）：卡片渐变占位上转 spinner；
    /// 重试耗尽后 false → 静态类型图标，不假装在加载。
    pub thumb_pending: bool,
}

/// 运行状态（状态机的 UI 投影）。
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

/// 显示器条目（视图投影；平台稳定 ID 只用于引擎映射，UI 主要展示友好名称）。
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
    /// 填充模式（详情条下拉；真源 = core Fit，快照回灌）。
    pub fit: FitMode,
}

/// 填充模式（§4.4 详情条下拉，2026-10-12；真源 = core Fit 三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FitMode {
    /// 填充（cover）：等比放大裁切铺满，无黑边——默认。
    #[default]
    Cover,
    /// 适应（contain）：完整显示不裁切，比例不合留边。
    Contain,
    /// 拉伸（fill）：无视比例铺满，比例不合变形。
    Fill,
}

impl FitMode {
    /// 下拉选项（文案唯一出处 = strings）。
    pub fn label(self) -> &'static str {
        match self {
            FitMode::Cover => super::strings::FIT_COVER(),
            FitMode::Contain => super::strings::FIT_CONTAIN(),
            FitMode::Fill => super::strings::FIT_FILL(),
        }
    }

    /// core Fit → UI 投影（快照回灌唯一映射点）。
    pub fn from_core(fit: gesso_core::Fit) -> Self {
        match fit {
            gesso_core::Fit::Cover => FitMode::Cover,
            gesso_core::Fit::Contain => FitMode::Contain,
            gesso_core::Fit::Fill => FitMode::Fill,
        }
    }

    /// UI 投影 → core Fit（动作入队唯一映射点）。
    pub fn to_core(self) -> gesso_core::Fit {
        match self {
            FitMode::Cover => gesso_core::Fit::Cover,
            FitMode::Contain => gesso_core::Fit::Contain,
            FitMode::Fill => gesso_core::Fit::Fill,
        }
    }
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
    /// 自动检查更新（真源 = core Settings.update_check）。
    pub update_check: bool,
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
            update_check: true,
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
            update_check: self.update_check,
        }
    }
}

pub const FPS_OPTIONS: [u32; 4] = [60, 30, 15, 5];

/// 更新子系统阶段（§4.7 弹窗六态的投影；真源 = app::update 模块全局态）。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum UpdatePhase {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Ready,
    Failed,
}

/// 更新视图（快照注入；UI 不直接触碰 update 模块全局）。
#[derive(Clone, Default)]
pub struct UpdateView {
    pub phase: UpdatePhase,
    pub current_version: SharedString,
    /// 新版本号（Available/Downloading/Ready 态有值）。
    pub version: SharedString,
    /// release notes（按纯文本展示，弹窗限高滚动）。
    pub notes: SharedString,
    pub asset_name: SharedString,
    /// 安装包字节数（0 = 未知；进度行走不确定文案）。
    pub asset_size: u64,
    /// 已下载安装包的绝对路径（Ready 态）。
    pub file_path: SharedString,
    /// 下载进度原始字节（Downloading 态；total = asset_size，0 = 未知大小）。
    pub downloaded: u64,
    /// 失败原因（含出路提示）。
    pub error: SharedString,
    /// true = 下载阶段失败（[重试] 直达下载）；false = 检查阶段失败（[重试] 重新检查）。
    pub can_retry_download: bool,
}
