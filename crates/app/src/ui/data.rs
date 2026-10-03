//! 演示数据与领域视图类型。
//!
//! M3 接入 gesso-core 前，库/显示器/设置全部为页内合成的演示数据
//! （PRODUCT.md「Evidence on Hand」约定：演示数据须显式标注）。
//! 类型形状按 技术方案 §3.6 的内部 API 语义设计，接入 core 时仅替换来源。

use gpui_kit::SharedString;

/// 壁纸内容类型（§1 能力：video / image(GIF·WebP) / shader / html）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Video,
    Gif,
    Shader,
    Web,
}

impl Kind {
    /// 角标文案与筛选段控件的标签一致。
    pub fn label(self) -> &'static str {
        match self {
            Kind::Video => "VIDEO",
            Kind::Gif => "GIF",
            Kind::Shader => "SHADER",
            Kind::Web => "WEB",
        }
    }

    /// 筛选段控件用的中文名。
    pub fn filter_label(self) -> &'static str {
        match self {
            Kind::Video => "视频",
            Kind::Gif => "动图",
            Kind::Shader => "Shader",
            Kind::Web => "网页",
        }
    }
}

/// 演示预览的渐变配色（真实实现 = 导入时预生成的抽帧序列，界面与交互设计 §6）。
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
    /// meta 行：分辨率 · 时长/帧率（tabular 数字）。
    pub meta: SharedString,
    /// 指派到的显示器下标；None = 未指派。
    pub assigned: Option<usize>,
    /// 素材失效（文件已移除）——danger 只表真故障。
    pub broken: bool,
    /// 真实库条目（gesso-core LibraryEntry）；false = 页内演示条目，不桥接会话。
    pub real: bool,
    pub art: Art,
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
    /// 真实显示器 ID（pin 层 cg-<id>）；空串 = 演示数据，不桥接会话。
    pub real_id: String,
    /// 托盘/状态行用的短名（"→ 主屏"）。
    pub short: SharedString,
    /// "27″ · 3840×2160"（tabular）。
    pub label: SharedString,
    /// 拓扑图用的工作区矩形（逻辑像素 x/y/w/h）。
    pub rect: (f32, f32, f32, f32),
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
    pub fn label(self) -> &'static str {
        match self {
            SuspendPolicy::Pause => "暂停",
            SuspendPolicy::Downclock => "降帧至 5 fps",
            SuspendPolicy::Ignore => "忽略",
        }
    }
}

/// 设置项（全部即时生效；持久化在 core config.json，此处为演示内存态）。
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
        }
    }
}

pub const FPS_OPTIONS: [u32; 4] = [60, 30, 15, 5];

/// 演示数据：与冻结原型 (prototype/index.html) 的 LIB/MONOS 逐条对应。
pub fn demo_library() -> Vec<LibraryItem> {
    vec![
        LibraryItem {
            id: "nebula".into(),
            name: "Nebula Drift".into(),
            kind: Kind::Video,
            we: false,
            meta: "4K · 12s".into(),
            assigned: Some(0),
            broken: false,
            real: false,
            art: Art {
                from: 0x24345C,
                to: 0x0E0F13,
            },
        },
        LibraryItem {
            id: "waves".into(),
            name: "Waves".into(),
            kind: Kind::Shader,
            we: false,
            meta: "1080p · 60fps".into(),
            assigned: Some(1),
            broken: false,
            real: false,
            art: Art {
                from: 0x1C4A5E,
                to: 0x0E0F13,
            },
        },
        LibraryItem {
            id: "clock".into(),
            name: "极简时钟".into(),
            kind: Kind::Web,
            we: false,
            meta: "自适应".into(),
            assigned: None,
            broken: false,
            real: false,
            art: Art {
                from: 0x2A2A30,
                to: 0x101012,
            },
        },
        LibraryItem {
            id: "sakura".into(),
            name: "Sakura Night".into(),
            kind: Kind::Video,
            we: true,
            meta: "4K · 24s".into(),
            assigned: None,
            broken: false,
            real: false,
            art: Art {
                from: 0x4A2C48,
                to: 0x120B12,
            },
        },
        LibraryItem {
            id: "rain".into(),
            name: "Rain Streaks".into(),
            kind: Kind::Gif,
            we: false,
            meta: "720p · GIF".into(),
            assigned: None,
            broken: false,
            real: false,
            art: Art {
                from: 0x2E3E50,
                to: 0x0E0F13,
            },
        },
        LibraryItem {
            id: "plasma".into(),
            name: "Plasma Field".into(),
            kind: Kind::Shader,
            we: false,
            meta: "1440p · 60fps".into(),
            assigned: None,
            broken: false,
            real: false,
            art: Art {
                from: 0x3E2A5E,
                to: 0x0E0F13,
            },
        },
        // 演示「素材失效」态：预览降级灰底 + 信息条红字「文件已移除」。
        LibraryItem {
            id: "fuji".into(),
            name: "Mount Fuji".into(),
            kind: Kind::Video,
            we: false,
            meta: "4K · 36s".into(),
            assigned: None,
            broken: true,
            real: false,
            art: Art {
                from: 0x26262A,
                to: 0x26262A,
            },
        },
        LibraryItem {
            id: "matrix".into(),
            name: "Digital Rain".into(),
            kind: Kind::Web,
            we: true,
            meta: "1080p".into(),
            assigned: None,
            broken: false,
            real: false,
            art: Art {
                from: 0x1E3A2E,
                to: 0x0A0F0C,
            },
        },
    ]
}

/// 演示显示器：主屏播放中；副屏全屏暂停（§5 跨屏状态矩阵的两种常态）。
pub fn demo_monitors() -> Vec<MonitorEntry> {
    vec![
        MonitorEntry {
            name: "主显示器".into(),
            real_id: String::new(),
            short: "主屏".into(),
            label: "27″ · 3840×2160".into(),
            rect: (0., 0., 2560., 1440.),
            wallpaper: Some("nebula".into()),
            state: PlayState::Playing,
            fps: 60,
        },
        MonitorEntry {
            name: "副显示器".into(),
            real_id: String::new(),
            short: "副屏".into(),
            label: "24″ · 1920×1080".into(),
            rect: (2560., 360., 1920., 1080.),
            wallpaper: Some("waves".into()),
            state: PlayState::FullscreenPaused,
            fps: 30,
        },
    ]
}

/// 供向导第②步复用的内置样例描述（3 视频 + 3 Shader + 2 网页时钟）。
pub struct SampleEntry {
    pub name: &'static str,
    pub kind: Kind,
    pub art: Art,
}

pub const WIZARD_SAMPLES: [SampleEntry; 8] = [
    SampleEntry {
        name: "星云漂移",
        kind: Kind::Video,
        art: Art {
            from: 0x24345C,
            to: 0x0E0F13,
        },
    },
    SampleEntry {
        name: "极光夜",
        kind: Kind::Video,
        art: Art {
            from: 0x1E4A3C,
            to: 0x0C110E,
        },
    },
    SampleEntry {
        name: "海浪",
        kind: Kind::Video,
        art: Art {
            from: 0x1C4A5E,
            to: 0x0E0F13,
        },
    },
    SampleEntry {
        name: "正弦波场",
        kind: Kind::Shader,
        art: Art {
            from: 0x2E2A5E,
            to: 0x0E0F13,
        },
    },
    SampleEntry {
        name: "等离子场",
        kind: Kind::Shader,
        art: Art {
            from: 0x3E2A5E,
            to: 0x0E0F13,
        },
    },
    SampleEntry {
        name: "粒子星系",
        kind: Kind::Shader,
        art: Art {
            from: 0x14304A,
            to: 0x0A0D12,
        },
    },
    SampleEntry {
        name: "极简时钟",
        kind: Kind::Web,
        art: Art {
            from: 0x2A2A30,
            to: 0x101012,
        },
    },
    SampleEntry {
        name: "翻页日历",
        kind: Kind::Web,
        art: Art {
            from: 0x3A2E26,
            to: 0x12100C,
        },
    },
];
