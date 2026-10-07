//! 界面文案唯一出处（§7 文案原则）。
//!
//! 支持简体中文 / English：[`Texts`] 是一套完整文案包，`ZH` / `EN` 两个常量；
//! 运行时语言存于 [`LANG`]（0=中 1=英），由 [`set_lang`] 在启动/设置切换时更新。
//! 各 UI 点用同名访问函数（如 [`TAB_LIBRARY`]）取当前语言文案；带参的用
//! `*_fmt` 函数在包内拼装，调用处不拼字符串。

use std::sync::atomic::{AtomicU8, Ordering};

use gesso_core::config::Language;

/// 一套语言的全部文案（常量串）。字段命名即旧常量名，调用点改动最小。
#[derive(Debug, Clone, Copy)]
pub struct Texts {
    pub app_name: &'static str,

    // 页签
    pub tab_library: &'static str,
    pub tab_monitors: &'static str,
    pub tab_settings: &'static str,

    // 库页工具条
    pub search_placeholder: &'static str,
    pub btn_import: &'static str,
    pub filter_all: &'static str,
    pub filter_we: &'static str,

    // 库页状态条
    pub hint_library: &'static str,
    pub empty_library_title: &'static str,
    pub empty_library_desc: &'static str,
    pub btn_import_file: &'static str,
    pub btn_browse_samples: &'static str,
    pub btn_clear_search: &'static str,

    // 库卡片 / 右键菜单
    pub badge_we: &'static str,
    pub file_removed: &'static str,
    pub menu_set_wallpaper: &'static str,
    pub menu_all_monitors: &'static str,
    pub menu_open_folder: &'static str,
    pub menu_details: &'static str,
    pub menu_remove: &'static str,

    // 导入失败
    pub import_err_unsupported: &'static str,
    pub import_err_mkv: &'static str,
    pub import_err_hevc: &'static str,
    pub import_err_io: &'static str,

    // 拖拽投放区
    pub dz_hint: &'static str,

    // 显示器页
    pub btn_redetect: &'static str,
    pub monitor_hover_note: &'static str,
    pub st_select_wallpaper: &'static str,
    pub monitors_empty_title: &'static str,
    pub edid_note_short: &'static str,
    pub btn_change: &'static str,
    pub btn_pause: &'static str,
    pub btn_resume: &'static str,
    pub st_playing: &'static str,
    pub st_paused: &'static str,
    pub st_fullscreen: &'static str,
    pub st_battery: &'static str,
    pub st_unassigned: &'static str,
    pub btn_go_assign: &'static str,
    pub toast_redetect: &'static str,
    pub toast_change_hint: &'static str,

    // 设置页
    pub group_perf: &'static str,
    pub group_startup: &'static str,
    pub group_linkage: &'static str,
    pub group_experimental: &'static str,
    pub group_advanced: &'static str,
    pub set_fps_cap: &'static str,
    pub set_fullscreen: &'static str,
    pub set_fullscreen_desc: &'static str,
    pub set_battery: &'static str,
    pub set_idle_downclock: &'static str,
    pub set_idle_desc: &'static str,
    pub set_autolaunch: &'static str,
    pub set_startup_behavior: &'static str,
    pub startup_restore: &'static str,
    pub startup_random: &'static str,
    pub set_weather: &'static str,
    pub set_weather_desc: &'static str,
    pub weather_open_meteo: &'static str,
    pub weather_custom: &'static str,
    pub weather_key_placeholder: &'static str,
    pub set_log_dir: &'static str,
    pub btn_open: &'static str,
    pub set_language: &'static str,
    pub language_auto: &'static str,
    pub language_zh: &'static str,
    pub language_en: &'static str,
    pub set_reset: &'static str,
    pub btn_reset: &'static str,
    pub btn_reset_confirm: &'static str,
    pub toast_reset: &'static str,

    // 内容类型（筛选段控 + 向导种类行）
    pub kind_video: &'static str,
    pub kind_gif: &'static str,
    pub kind_photo: &'static str,
    pub kind_shader: &'static str,
    pub kind_web: &'static str,

    // 设置页策略下拉（全屏/电池共用一组）
    pub policy_pause: &'static str,
    pub policy_downclock: &'static str,
    pub policy_ignore: &'static str,

    // 托盘（构建时取启动语言；语言切换经 UpdateSettings 检测后实时重建）
    pub tray_pause_all: &'static str,
    pub tray_random: &'static str,
    pub tray_main_window: &'static str,
    pub tray_autostart: &'static str,
    pub tray_quit: &'static str,

    // 显示器命名（引擎快照注入）
    pub mon_main: &'static str,
    pub mon_main_short: &'static str,
    pub meta_builtin: &'static str,

    // 顶栏
    pub tip_replay_wizard: &'static str,
    pub tip_toggle_theme: &'static str,
    // Windows 标题栏按钮专用（调用点 cfg = windows），macOS 构建下未被读取
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub win_minimize: &'static str,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub win_maximize: &'static str,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub win_close: &'static str,

    // 库页补充（右键指派子菜单 / 状态条 / 空搜索）
    pub menu_main_display: &'static str,
    pub menu_second_display: &'static str,

    // 导入对话框过滤名 / 指派错误（app_state）
    pub filter_wallpapers: &'static str,
    pub filter_we_project: &'static str,
    pub err_wallpaper_missing: &'static str,
    pub err_asset_broken: &'static str,
    pub unknown_display: &'static str,
    pub unnamed: &'static str,

    // 从 URL 导入网页（「导入」菜单项 → 小窗）
    pub btn_cancel: &'static str,
    pub url_import_title: &'static str,
    pub url_import_desc: &'static str,
    pub url_import_placeholder: &'static str,
    pub url_import_invalid: &'static str,

    // 向导补充
    pub wizard_title_suffix: &'static str,
    pub wizard_back: &'static str,
    pub wizard_skip_toast: &'static str,
    pub wizard_title: &'static str,
    pub wizard_subtitle: &'static str,
    pub wizard_start: &'static str,
    pub wizard_skip: &'static str,
    pub wizard_pick_title: &'static str,
    pub wizard_pick_subtitle: &'static str,
    pub wizard_import_own: &'static str,
    pub wizard_done_title: &'static str,
    pub wizard_another: &'static str,
    pub wizard_finish: &'static str,
    pub wizard_sample_tag: &'static str,
    pub wizard_empty_library: &'static str,
}

/// 简体中文。
pub const ZH: Texts = Texts {
    app_name: "Gesso",

    tab_library: "壁纸库",
    tab_monitors: "显示器",
    tab_settings: "设置",

    search_placeholder: "搜索壁纸",
    btn_import: "导入",
    filter_all: "全部",
    filter_we: "WE",

    hint_library: "双击设为主显示器 · 拖入文件导入 · 拖拽卡片到「显示器」页指派",
    empty_library_title: "桌面还没动起来",
    empty_library_desc: "把第一张壁纸拖进来",
    btn_import_file: "导入文件",
    btn_browse_samples: "浏览内置样例",
    btn_clear_search: "清除搜索",

    badge_we: "WE",
    file_removed: "文件已移除",
    menu_set_wallpaper: "设为壁纸",
    menu_all_monitors: "全部",
    menu_open_folder: "打开所在目录",
    menu_details: "查看详情",
    menu_remove: "从库中移除",

    import_err_unsupported: "不支持的文件类型。支持：mp4 / webm / gif / webp / html / glsl",
    import_err_mkv: ".mkv 容器暂不支持：请用「快速转封装为 mp4」工具转换后再导入",
    import_err_hevc:
        "HEVC 视频需要系统安装 HEVC 扩展（Microsoft Store 免费），建议改用 H.264 编码的 mp4",
    import_err_io: "导入失败：文件复制出错（磁盘空间或权限问题）",

    dz_hint: "拖到目标显示器上放手",

    kind_video: "视频",
    kind_gif: "动图",
    kind_photo: "图片",
    kind_shader: "Shader",
    kind_web: "网页",

    policy_pause: "暂停",
    policy_downclock: "降帧至 5 fps",
    policy_ignore: "忽略",

    tray_pause_all: "暂停全部壁纸",
    tray_random: "随机换一张",
    tray_main_window: "管理窗口…",
    tray_autostart: "开机自启",
    tray_quit: "退出 Gesso",

    mon_main: "主显示器",
    mon_main_short: "主屏",
    meta_builtin: "内置样例",

    tip_replay_wizard: "重放首启向导",
    tip_toggle_theme: "切换亮 / 暗主题",
    win_minimize: "最小化",
    win_maximize: "最大化 / 还原",
    win_close: "关闭",

    menu_main_display: "主屏",
    menu_second_display: "副屏",

    filter_wallpapers: "壁纸文件",
    filter_we_project: "Wallpaper Engine 项目",
    err_wallpaper_missing: "找不到该壁纸",
    err_asset_broken: "素材失效，无法指派",
    unknown_display: "未知显示器",
    unnamed: "未命名",

    btn_cancel: "取消",
    url_import_title: "从 URL 导入网页",
    url_import_desc: "远端网页将嵌入桌面图标层之下（仅 https）；断网时该壁纸显示空白。",
    url_import_placeholder: "https://…（网页地址）",
    url_import_invalid: "无效的网页地址（需要 https://）",

    wizard_title_suffix: "首启向导",
    wizard_back: "上一步",
    wizard_skip_toast: "已使用纯色桌面；随时可以从托盘开始",

    btn_redetect: "重新检测",
    monitor_hover_note: "悬停库卡片时，对应显示器会亮起 · 点击屏选中，下方控制",
    st_select_wallpaper: "选择壁纸",
    monitors_empty_title: "未检测到显示器",
    edid_note_short: "标识取自 EDID · 配置随显示器保留，重新接入自动恢复",
    btn_change: "更换",
    btn_pause: "暂停",
    btn_resume: "恢复",
    st_playing: "播放中",
    st_paused: "已暂停",
    st_fullscreen: "全屏应用中",
    st_battery: "电池省电",
    st_unassigned: "未指派",
    btn_go_assign: "去指派",
    toast_redetect: "已重新检测显示器",
    toast_change_hint: "在壁纸库双击即设为主显示器",

    group_perf: "性能",
    group_startup: "启动",
    group_linkage: "联动",
    group_experimental: "实验",
    group_advanced: "高级",
    set_fps_cap: "默认帧率上限",
    set_fullscreen: "全屏应用时",
    set_fullscreen_desc: "前台应用进入全屏后自动暂停对应显示器",
    set_battery: "使用电池时",
    set_idle_downclock: "空闲时自动降帧",
    set_idle_desc: "光标不在该显示器且无音频时，帧率减半",
    set_autolaunch: "开机自启",
    set_startup_behavior: "启动后",
    startup_restore: "恢复上次",
    startup_random: "随机一张",
    set_weather: "天气数据源",
    set_weather_desc: "供「网页」类壁纸读取；open-meteo 免密钥",
    weather_open_meteo: "open-meteo 免密钥",
    weather_custom: "自定义 key…",
    weather_key_placeholder: "粘贴 API key",
    set_log_dir: "日志目录",
    btn_open: "打开",
    set_language: "界面语言",
    language_auto: "跟随系统",
    language_zh: "简体中文",
    language_en: "English",
    set_reset: "重置全部设置",
    btn_reset: "重置…",
    btn_reset_confirm: "确认重置？",
    toast_reset: "设置已恢复默认",

    wizard_title: "让桌面动起来",
    wizard_subtitle: "视频、Shader、网页，都能钉在桌面图标层之下。",
    wizard_start: "开始",
    wizard_skip: "跳过（使用纯色桌面）",
    wizard_pick_title: "选一张内置样例",
    wizard_pick_subtitle: "双击即应用到主显示器",
    wizard_import_own: "导入自己的文件…",
    wizard_done_title: "已应用到主显示器",
    wizard_another: "再配一块屏幕",
    wizard_finish: "完成",
    wizard_sample_tag: "样例",
    wizard_empty_library: "库里还没有壁纸——点下方「导入自己的文件…」或稍后从主窗口拖入",
};

/// English.
pub const EN: Texts = Texts {
    app_name: "Gesso",

    tab_library: "Library",
    tab_monitors: "Displays",
    tab_settings: "Settings",

    search_placeholder: "Search wallpapers",
    btn_import: "Import",
    filter_all: "All",
    filter_we: "WE",

    hint_library: "Double-click to set on the main display · Drop a file to import · Drag a card to the Displays tab to assign",
    empty_library_title: "Your desktop is still static",
    empty_library_desc: "Drop in your first wallpaper",
    btn_import_file: "Import file",
    btn_browse_samples: "Browse built-in samples",
    btn_clear_search: "Clear search",

    badge_we: "WE",
    file_removed: "File removed",
    menu_set_wallpaper: "Set as wallpaper",
    menu_all_monitors: "All displays",
    menu_open_folder: "Open containing folder",
    menu_details: "View details",
    menu_remove: "Remove from library",

    import_err_unsupported: "Unsupported file type. Supported: mp4 / webm / gif / webp / html / glsl",
    import_err_mkv: ".mkv containers are not supported yet — remux to mp4 with the quick remux tool first",
    import_err_hevc: "HEVC video needs the HEVC extension (free in the Microsoft Store); H.264 mp4 is recommended",
    import_err_io: "Import failed: file copy error (disk space or permissions)",

    dz_hint: "Drop onto the target display",

    kind_video: "Video",
    kind_gif: "Animated",
    kind_photo: "Image",
    kind_shader: "Shader",
    kind_web: "Web",

    policy_pause: "Pause",
    policy_downclock: "Downclock to 5 fps",
    policy_ignore: "Ignore",

    tray_pause_all: "Pause all wallpapers",
    tray_random: "Shuffle",
    tray_main_window: "Open manager…",
    tray_autostart: "Launch at login",
    tray_quit: "Quit Gesso",

    mon_main: "Main display",
    mon_main_short: "Main",
    meta_builtin: "Built-in sample",

    tip_replay_wizard: "Replay first-run wizard",
    tip_toggle_theme: "Toggle light / dark theme",
    win_minimize: "Minimize",
    win_maximize: "Maximize / Restore",
    win_close: "Close",

    menu_main_display: "Main display",
    menu_second_display: "Second display",

    filter_wallpapers: "Wallpaper files",
    filter_we_project: "Wallpaper Engine project",
    err_wallpaper_missing: "Wallpaper not found",
    err_asset_broken: "Asset unavailable, cannot assign",
    unknown_display: "Unknown display",
    unnamed: "Untitled",

    btn_cancel: "Cancel",
    url_import_title: "Import web page from URL",
    url_import_desc: "The remote page runs embedded below your desktop icons (https only); it goes blank when offline.",
    url_import_placeholder: "https://… (page URL)",
    url_import_invalid: "Invalid web page URL (https:// required)",

    wizard_title_suffix: "First-Run Wizard",
    wizard_back: "Back",
    wizard_skip_toast: "Plain desktop in use — start from the tray anytime",

    btn_redetect: "Redetect",
    monitor_hover_note: "Hovering a library card highlights its display · Click a display to select and control it below",
    st_select_wallpaper: "Choose wallpaper",
    monitors_empty_title: "No displays detected",
    edid_note_short: "ID from EDID · Settings follow the display and restore on reconnect",
    btn_change: "Change",
    btn_pause: "Pause",
    btn_resume: "Resume",
    st_playing: "Playing",
    st_paused: "Paused",
    st_fullscreen: "Fullscreen app active",
    st_battery: "Battery saver",
    st_unassigned: "Unassigned",
    btn_go_assign: "Assign",
    toast_redetect: "Displays redetected",
    toast_change_hint: "Double-click a wallpaper in the library to set it on the main display",

    group_perf: "Performance",
    group_startup: "Startup",
    group_linkage: "Integration",
    group_experimental: "Experimental",
    group_advanced: "Advanced",
    set_fps_cap: "Default frame rate cap",
    set_fullscreen: "When an app is fullscreen",
    set_fullscreen_desc: "Auto-pause that display when a foreground app enters fullscreen",
    set_battery: "When on battery",
    set_idle_downclock: "Reduce frame rate when idle",
    set_idle_desc: "Halve frame rate when the cursor is off that display and there is no audio",
    set_autolaunch: "Launch at login",
    set_startup_behavior: "After launch",
    startup_restore: "Restore last",
    startup_random: "Random wallpaper",
    set_weather: "Weather data source",
    set_weather_desc: "Read by Web wallpapers; open-meteo needs no key",
    weather_open_meteo: "open-meteo (no key)",
    weather_custom: "Custom key…",
    weather_key_placeholder: "Paste API key",
    set_log_dir: "Log directory",
    btn_open: "Open",
    set_language: "Interface language",
    language_auto: "System",
    language_zh: "简体中文",
    language_en: "English",
    set_reset: "Reset all settings",
    btn_reset: "Reset…",
    btn_reset_confirm: "Confirm reset?",
    toast_reset: "Settings restored to defaults",

    wizard_title: "Bring your desktop to life",
    wizard_subtitle: "Video, shaders, and the web — pinned below your desktop icons.",
    wizard_start: "Get started",
    wizard_skip: "Skip (keep a plain desktop)",
    wizard_pick_title: "Pick a built-in sample",
    wizard_pick_subtitle: "Double-click to apply on the main display",
    wizard_import_own: "Import my own file…",
    wizard_done_title: "Applied to the main display",
    wizard_another: "Set up another display",
    wizard_finish: "Done",
    wizard_sample_tag: "Sample",
    wizard_empty_library: "The library is empty — use \"Import my own file…\" below or drag into the main window later",
};

/// 当前语言：0 = 简体中文，1 = English。
static LANG: AtomicU8 = AtomicU8::new(0);

/// 当前语言序数（0=中 1=英）。供按构建时缓存文案的组件（设置页下拉）
/// 判断「语言变了需要重建」的时机。
pub fn lang() -> u8 {
    LANG.load(Ordering::Relaxed)
}

/// 当前语言包。
pub fn t() -> Texts {
    if LANG.load(Ordering::Relaxed) == 1 {
        EN
    } else {
        ZH
    }
}

/// 设置运行时语言（启动时按配置解析、设置页切换时调用）。
pub fn set_lang(lang: Language) {
    let resolved = match lang {
        Language::En => 1,
        Language::Zh => 0,
        Language::Auto => system_is_english() as u8,
    };
    LANG.store(resolved, Ordering::Relaxed);
}

/// 检测系统主语言是否为英语。无第三方依赖：读环境变量（跨平台，Windows 设置了
/// UI 语言相关变量；macOS/Linux 常用 LANG/LC_ALL）。无法判定时保守 false（中文）。
fn system_is_english() -> bool {
    for v in ["LC_ALL", "LANG", "LANGUAGE"] {
        if let Ok(val) = std::env::var(v) {
            let lower = val.to_ascii_lowercase();
            if lower.starts_with("en") {
                return true;
            }
            if !lower.is_empty() && lower != "c" && lower != "posix" {
                return false;
            }
        }
    }
    false
}

// ── 访问函数（旧 const 名，改调用点加 ()）──────────────────────────────────
macro_rules! accessors {
    ($($field:ident => $name:ident),* $(,)?) => {
        $(
            #[allow(non_snake_case)]
            #[doc = concat!("当前语言：", stringify!($name))]
            pub fn $name() -> &'static str {
                t().$field
            }
        )*
    };
}
accessors! {
    app_name => APP_NAME,
    tab_library => TAB_LIBRARY, tab_monitors => TAB_MONITORS, tab_settings => TAB_SETTINGS,
    search_placeholder => SEARCH_PLACEHOLDER, btn_import => BTN_IMPORT,
    filter_all => FILTER_ALL, filter_we => FILTER_WE,
    hint_library => HINT_LIBRARY, empty_library_title => EMPTY_LIBRARY_TITLE,
    empty_library_desc => EMPTY_LIBRARY_DESC, btn_import_file => BTN_IMPORT_FILE,
    btn_browse_samples => BTN_BROWSE_SAMPLES, btn_clear_search => BTN_CLEAR_SEARCH,
    badge_we => BADGE_WE, file_removed => FILE_REMOVED,
    menu_set_wallpaper => MENU_SET_WALLPAPER, menu_all_monitors => MENU_ALL_MONITORS,
    menu_open_folder => MENU_OPEN_FOLDER, menu_details => MENU_DETAILS, menu_remove => MENU_REMOVE,
    import_err_unsupported => IMPORT_ERR_UNSUPPORTED, import_err_mkv => IMPORT_ERR_MKV,
    import_err_hevc => IMPORT_ERR_HEVC, import_err_io => IMPORT_ERR_IO,
    dz_hint => DZ_HINT,
    kind_video => KIND_VIDEO, kind_gif => KIND_GIF, kind_photo => KIND_PHOTO,
    kind_shader => KIND_SHADER, kind_web => KIND_WEB,
    policy_pause => POLICY_PAUSE, policy_downclock => POLICY_DOWNCLOCK, policy_ignore => POLICY_IGNORE,
    tray_pause_all => TRAY_PAUSE_ALL, tray_random => TRAY_RANDOM,
    tray_main_window => TRAY_MAIN_WINDOW, tray_autostart => TRAY_AUTOSTART, tray_quit => TRAY_QUIT,
    mon_main => MON_MAIN, mon_main_short => MON_MAIN_SHORT, meta_builtin => META_BUILTIN,
    tip_replay_wizard => TIP_REPLAY_WIZARD, tip_toggle_theme => TIP_TOGGLE_THEME,
    // win_minimize/win_maximize/win_close 只在 Windows 编译进调用点（标题栏按钮），
    // 走 t().win_* 取文案，避免 macOS 构建下 dead_code 告警，故不列于此。
    menu_main_display => MENU_MAIN_DISPLAY, menu_second_display => MENU_SECOND_DISPLAY,
    filter_wallpapers => FILTER_WALLPAPERS, filter_we_project => FILTER_WE_PROJECT,
    err_wallpaper_missing => ERR_WALLPAPER_MISSING, err_asset_broken => ERR_ASSET_BROKEN,
    unknown_display => UNKNOWN_DISPLAY, unnamed => UNNAMED,
    url_import_title => URL_IMPORT_TITLE,
    url_import_desc => URL_IMPORT_DESC, url_import_placeholder => URL_IMPORT_PLACEHOLDER,
    url_import_invalid => URL_IMPORT_INVALID, btn_cancel => BTN_CANCEL,
    wizard_title_suffix => WIZARD_TITLE_SUFFIX, wizard_back => WIZARD_BACK,
    wizard_skip_toast => WIZARD_SKIP_TOAST,
    btn_redetect => BTN_REDETECT, monitor_hover_note => MONITOR_HOVER_NOTE,
    st_select_wallpaper => ST_SELECT_WALLPAPER, monitors_empty_title => MONITORS_EMPTY_TITLE,
    edid_note_short => EDID_NOTE_SHORT, btn_change => BTN_CHANGE,
    btn_pause => BTN_PAUSE, btn_resume => BTN_RESUME,
    st_playing => ST_PLAYING, st_paused => ST_PAUSED, st_fullscreen => ST_FULLSCREEN,
    st_battery => ST_BATTERY, st_unassigned => ST_UNASSIGNED,
    btn_go_assign => BTN_GO_ASSIGN, toast_redetect => TOAST_REDETECT, toast_change_hint => TOAST_CHANGE_HINT,
    group_perf => GROUP_PERF, group_startup => GROUP_STARTUP, group_linkage => GROUP_LINKAGE,
    group_experimental => GROUP_EXPERIMENTAL, group_advanced => GROUP_ADVANCED,
    set_fps_cap => SET_FPS_CAP, set_fullscreen => SET_FULLSCREEN,
    set_fullscreen_desc => SET_FULLSCREEN_DESC, set_battery => SET_BATTERY,
    set_idle_downclock => SET_IDLE_DOWNCLOCK, set_idle_desc => SET_IDLE_DESC,
    set_autolaunch => SET_AUTOLAUNCH, set_startup_behavior => SET_STARTUP_BEHAVIOR,
    startup_restore => STARTUP_RESTORE, startup_random => STARTUP_RANDOM,
    set_weather => SET_WEATHER, set_weather_desc => SET_WEATHER_DESC,
    weather_open_meteo => WEATHER_OPEN_METEO, weather_custom => WEATHER_CUSTOM,
    weather_key_placeholder => WEATHER_KEY_PLACEHOLDER, set_log_dir => SET_LOG_DIR,
    btn_open => BTN_OPEN,
    set_language => SET_LANGUAGE, language_auto => LANGUAGE_AUTO,
    language_zh => LANGUAGE_ZH, language_en => LANGUAGE_EN,
    set_reset => SET_RESET, btn_reset => BTN_RESET, btn_reset_confirm => BTN_RESET_CONFIRM,
    toast_reset => TOAST_RESET,
    wizard_title => WIZARD_TITLE, wizard_subtitle => WIZARD_SUBTITLE,
    wizard_start => WIZARD_START, wizard_skip => WIZARD_SKIP,
    wizard_pick_title => WIZARD_PICK_TITLE, wizard_pick_subtitle => WIZARD_PICK_SUBTITLE,
    wizard_import_own => WIZARD_IMPORT_OWN, wizard_done_title => WIZARD_DONE_TITLE,
    wizard_another => WIZARD_ANOTHER, wizard_finish => WIZARD_FINISH,
    wizard_sample_tag => WIZARD_SAMPLE_TAG, wizard_empty_library => WIZARD_EMPTY_LIBRARY,
}

// ── 带参文案：包内模板拼装，调用处不拼 ─────────────────────────────────────
pub fn toast_assign(name: &str, target: &str) -> String {
    if LANG.load(Ordering::Relaxed) == 1 {
        format!("Assigned \"{name}\" to {target}")
    } else {
        format!("已将「{name}」指派到{target}")
    }
}

pub fn toast_apply_main(name: &str) -> String {
    if LANG.load(Ordering::Relaxed) == 1 {
        format!("Applied \"{name}\" to the main display")
    } else {
        format!("已将「{name}」应用到主显示器")
    }
}

pub fn toast_removed(name: &str) -> String {
    if LANG.load(Ordering::Relaxed) == 1 {
        format!("Removed \"{name}\" from the library (files kept)")
    } else {
        format!("已从库中移除「{name}」（文件未删除）")
    }
}

pub fn toast_imported(name: &str) -> String {
    if LANG.load(Ordering::Relaxed) == 1 {
        format!("Imported \"{name}\"")
    } else {
        format!("已导入「{name}」")
    }
}

pub fn toast_url_imported(title: &str) -> String {
    if LANG.load(Ordering::Relaxed) == 1 {
        format!("Queued web page \"{title}\" — importing")
    } else {
        format!("已加入「{title}」，正在导入")
    }
}

pub fn notice_new_monitor(name: &str) -> String {
    if LANG.load(Ordering::Relaxed) == 1 {
        format!("New display detected: {name}")
    } else {
        format!("检测到新显示器「{name}」")
    }
}

/// 库页状态条计数：ZH「3 项 · WE 1」/ EN「3 items · WE 1」。
pub fn library_count(total: usize, we: usize) -> String {
    if LANG.load(Ordering::Relaxed) == 1 {
        format!("{total} items · WE {we}")
    } else {
        format!("{total} 项 · WE {we}")
    }
}

pub fn toast_assign_all(name: &str) -> String {
    if LANG.load(Ordering::Relaxed) == 1 {
        format!("Assigned \"{name}\" to all displays")
    } else {
        format!("已将「{name}」指派到全部显示器")
    }
}

pub fn search_no_match(query: &str) -> String {
    if LANG.load(Ordering::Relaxed) == 1 {
        format!("No wallpapers match \"{query}\"")
    } else {
        format!("没有匹配「{query}」的壁纸")
    }
}

/// 非主显示器的全名（快照注入 + 指派 toast 目标）。
pub fn monitor_name(index: usize) -> String {
    if LANG.load(Ordering::Relaxed) == 1 {
        format!("Display {index}")
    } else {
        format!("显示器 {index}")
    }
}

/// 非主显示器的短名（卡片角标/状态行）。
pub fn monitor_short(index: usize) -> String {
    if LANG.load(Ordering::Relaxed) == 1 {
        format!("D{index}")
    } else {
        format!("屏{index}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_language_and_formats() {
        set_lang(Language::Zh);
        assert_eq!(TAB_LIBRARY(), "壁纸库");
        assert!(toast_imported("X").contains("已导入"));

        set_lang(Language::En);
        assert_eq!(TAB_LIBRARY(), "Library");
        assert_eq!(toast_imported("X"), "Imported \"X\"");
        // 设置页策略下拉曾硬编码中文（data.rs），此处防回归
        assert_eq!(POLICY_PAUSE(), "Pause");
        assert_eq!(POLICY_IGNORE(), "Ignore");
        assert_eq!(KIND_VIDEO(), "Video");
    }

    #[test]
    fn every_field_filled_in_both_packs() {
        // 两套包都不得留空字段（漏翻译会是空串）
        let packs = [ZH, EN];
        let strings: Vec<fn(Texts) -> &'static str> = vec![
            |p| p.tab_library,
            |p| p.tab_monitors,
            |p| p.tab_settings,
            |p| p.set_language,
            |p| p.wizard_title,
            |p| p.hint_library,
            |p| p.policy_pause,
            |p| p.policy_downclock,
            |p| p.policy_ignore,
            |p| p.kind_video,
            |p| p.kind_web,
            |p| p.tray_pause_all,
            |p| p.tray_quit,
            |p| p.mon_main,
            |p| p.win_close,
        ];
        for p in packs {
            for f in &strings {
                assert!(!f(p).is_empty());
            }
        }
    }

    #[test]
    fn detects_english_from_env() {
        // LANG 是进程级变量，并行测试会互相覆盖——持全局锁
        let _g = crate::ENV_LOCK.lock().unwrap();
        let old = std::env::var("LANG").ok();
        unsafe {
            std::env::set_var("LANG", "en_US.UTF-8");
        }
        assert!(system_is_english());
        unsafe {
            std::env::set_var("LANG", "zh_CN.UTF-8");
        }
        assert!(!system_is_english());
        match old {
            Some(v) => unsafe { std::env::set_var("LANG", v) },
            None => unsafe { std::env::remove_var("LANG") },
        }
    }
}
