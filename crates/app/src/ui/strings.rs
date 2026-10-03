//! 文案集中地（§7 文案原则；中文默认，key 化预留 i18n）。
//!
//! M3 接入 core 后由 strings/zh.json + en.json 加载替换；
//! 现阶段以常量保证「文案只出现一处」的纪律。

pub const APP_NAME: &str = "Gesso";
pub const PROTO_TAG: &str = "原型 · 演示数据";

// 页签
pub const TAB_LIBRARY: &str = "壁纸库";
pub const TAB_MONITORS: &str = "显示器";
pub const TAB_SETTINGS: &str = "设置";

// 库页工具条
pub const SEARCH_PLACEHOLDER: &str = "搜索壁纸";
pub const BTN_IMPORT: &str = "导入";
pub const BTN_SCAN_WORKSHOP: &str = "扫描工坊";
pub const FILTER_ALL: &str = "全部";
pub const FILTER_WE: &str = "WE";

// 库页状态条
pub const HINT_LIBRARY: &str = "双击设为主显示器 · 拖入文件导入 · 拖拽卡片到「显示器」页指派";
pub const EMPTY_LIBRARY_TITLE: &str = "桌面还没动起来";
pub const EMPTY_LIBRARY_DESC: &str = "把第一张壁纸拖进来";
pub const BTN_IMPORT_FILE: &str = "导入文件";
pub const BTN_BROWSE_SAMPLES: &str = "浏览内置样例";
pub const BTN_CLEAR_SEARCH: &str = "清除搜索";

// 库卡片 / 右键菜单
pub const BADGE_WE: &str = "WE";
pub const FILE_REMOVED: &str = "文件已移除";
pub const MENU_SET_WALLPAPER: &str = "设为壁纸";
pub const MENU_ALL_MONITORS: &str = "全部";
pub const MENU_OPEN_FOLDER: &str = "打开所在目录";
pub const MENU_DETAILS: &str = "查看详情";
pub const MENU_REMOVE: &str = "从库中移除";
pub const TOAST_ASSIGN: &str = "已将「{}」指派到{}";
pub const TOAST_APPLY_MAIN: &str = "已将「{}」应用到主显示器";
pub const TOAST_IMPORT_DIALOG: &str = "已打开文件选择（演示）";
pub const TOAST_SCAN_FOUND: &str = "扫描 Steam 工坊：找到 2 个可导入项（演示）";
pub const TOAST_OPEN_FOLDER: &str = "已在访达中显示（演示）";
pub const TOAST_REMOVED: &str = "已从库中移除「{}」（文件未删除）";

// 拖拽投放区
pub const DZ_HINT: &str = "拖到目标显示器上放手";

// 显示器页
pub const BTN_REDETECT: &str = "重新检测";
pub const MONITOR_HOVER_NOTE: &str = "悬停库卡片时，对应显示器边框会亮起";
pub const BTN_CHANGE: &str = "更换";
pub const BTN_PAUSE: &str = "暂停";
pub const BTN_RESUME: &str = "恢复";
pub const ST_PLAYING: &str = "播放中";
pub const ST_PAUSED: &str = "已暂停";
pub const ST_FULLSCREEN: &str = "全屏应用中";
pub const ST_BATTERY: &str = "电池省电";
pub const ST_UNASSIGNED: &str = "未指派";
pub const NOTICE_NEW_MONITOR: &str = "检测到新显示器「DELL U2723QE」";
pub const BTN_GO_ASSIGN: &str = "去指派";
pub const EDID_NOTE: &str = "显示器标识取自 EDID；配置随显示器保留，重新接入自动恢复";
pub const TOAST_REDETECT: &str = "已重新检测显示器";
pub const TOAST_CHANGE_HINT: &str = "在壁纸库双击即设为主显示器";

// 设置页
pub const GROUP_PERF: &str = "性能";
pub const GROUP_STARTUP: &str = "启动";
pub const GROUP_LINKAGE: &str = "联动";
pub const GROUP_EXPERIMENTAL: &str = "实验";
pub const GROUP_ADVANCED: &str = "高级";
pub const SET_FPS_CAP: &str = "默认帧率上限";
pub const SET_FULLSCREEN: &str = "全屏应用时";
pub const SET_FULLSCREEN_DESC: &str = "前台应用进入全屏后自动暂停对应显示器";
pub const SET_BATTERY: &str = "使用电池时";
pub const SET_IDLE_DOWNCLOCK: &str = "空闲时自动降帧";
pub const SET_IDLE_DESC: &str = "光标不在该显示器且无音频时，帧率减半";
pub const SET_AUTOLAUNCH: &str = "开机自启";
pub const SET_STARTUP_BEHAVIOR: &str = "启动后";
pub const STARTUP_RESTORE: &str = "恢复上次";
pub const STARTUP_RANDOM: &str = "随机一张";
pub const SET_WEATHER: &str = "天气数据源";
pub const SET_WEATHER_DESC: &str = "供「网页」类壁纸读取；open-meteo 免密钥";
pub const WEATHER_OPEN_METEO: &str = "open-meteo 免密钥";
pub const WEATHER_CUSTOM: &str = "自定义 key…";
pub const WEATHER_KEY_PLACEHOLDER: &str = "粘贴 API key";
pub const SET_LOG_DIR: &str = "日志目录";
pub const BTN_OPEN: &str = "打开";
pub const SET_RESET: &str = "重置全部设置";
pub const BTN_RESET: &str = "重置…";
pub const BTN_RESET_CONFIRM: &str = "确认重置？";
pub const TOAST_LOG_DIR: &str = "已打开日志目录（演示）";
pub const TOAST_RESET: &str = "设置已恢复默认";

// 向导
pub const WIZARD_TITLE: &str = "让桌面动起来";
pub const WIZARD_SUBTITLE: &str = "视频、Shader、网页，都能钉在桌面图标层之下。";
pub const WIZARD_START: &str = "开始";
pub const WIZARD_SKIP: &str = "跳过（使用纯色桌面）";
pub const WIZARD_PICK_TITLE: &str = "选一张内置样例";
pub const WIZARD_PICK_SUBTITLE: &str = "双击即应用到主显示器";
pub const WIZARD_IMPORT_OWN: &str = "导入自己的文件…";
pub const WIZARD_DONE_TITLE: &str = "已应用到主显示器";
pub const WIZARD_ANOTHER: &str = "再配一块屏幕";
pub const WIZARD_FINISH: &str = "完成";
pub const WIZARD_SAMPLE_TAG: &str = "样例";
