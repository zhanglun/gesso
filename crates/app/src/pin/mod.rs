//! 平台贴壁层：创建并管理"钉在桌面图标层之下"的壁纸窗口。
//!
//! M1.5 定稿架构（SPIKE-REPORT）：壁纸窗口是**本模块自有的原生窗口**
//! （macOS: 纯 AppKit NSWindow；Windows: 自有 Win32 窗口 + WorkerW 挂载，见 windows.rs），
//! **不经 GPUI 窗口管理**——GPUI 协调器与桌面级全屏几何必然冲突。
//! webview 由 wry 直挂窗口 contentView。

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

/// 显示器信息（枚举产物，稳定 ID 是配置映射的 key）。
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    /// 稳定 ID：macOS v1 用 CGDirectDisplayID（`cg-<id>`）、Windows v1 用设备名
    /// （`win-<DISPLAYn>`）；升级路径：EDID 哈希（技术方案 §4.3）
    pub id: String,
    #[allow(dead_code)] // 展示名：平台枚举填充；UI 投影用自己的命名规则（主屏/屏N）
    pub name: String,
    /// 平台原生全局坐标（含菜单栏/任务栏等全部区域）：macOS = AppKit（原点左下）、
    /// Windows = 虚拟桌面物理像素（原点左上，可为负——左侧/上方屏幕）。
    /// 排序/归一化只做同平台内比较，不做跨平台换算。
    pub frame: (f64, f64, f64, f64), // x, y, w, h
    pub is_main: bool,
}

/// 壁纸窗口的统一操作面（由各平台模块实现）。
/// 部分方法当前调用方未接（露桌面回退等），属平台面而非死代码。
#[allow(dead_code)]
pub trait WallpaperWindow {
    /// 加载宿主页 URL（gesso://host/index.html?spec=...）。
    fn load(&mut self, url: &str);
    /// JS 层暂停/恢复（技术方案 §5.3：暂停 = 停 RAF/video.pause，窗口常驻）。
    fn set_paused(&mut self, paused: bool);
    /// 隐藏/显示整个壁纸窗口（露出系统桌面 = 回退静态壁纸语义）。
    fn set_visible(&mut self, visible: bool);
    /// 窗口几何跟随显示器（显示器热插拔/分辨率变化）。
    fn set_frame(&mut self, frame: (f64, f64, f64, f64));
    /// 显示器重配后的贴壁重申（合盖 clamshell / 主屏切换 / 分辨率变更）：
    /// 重申平台贴壁属性（macOS = level + collectionBehavior——WindowServer 可能
    /// 在重配后重置层级，表现为壁纸变普通浮动窗口）并按新几何对齐窗口。
    fn reassert_pinning(&mut self, frame: (f64, f64, f64, f64));
    /// 挂载是否落位（Windows：WorkerW/Progman = ok；BottomMost = explorer 未就绪，
    /// 调用方应重试。macOS 恒 ok）。
    fn mount_ok(&self) -> bool {
        true
    }
    /// 当前 URL（诊断用：判断导航是否被 WKWebView 拒绝）
    fn current_url(&self) -> String;
    /// 透传 JS（M5 数据桥通道：时间 tick / 降帧 setFps；暂停仍走 set_paused）。
    fn evaluate(&mut self, _js: &str) {}

    /// 带回调的 JS 求值（诊断探针用；默认 no-op，平台按需实现）。
    fn evaluate_with_callback(&mut self, _js: &str, _cb: Box<dyn Fn(String) + Send>) {}
    /// 发送一条类型化宿主命令（默认走 evaluate；平台可覆写）。
    fn send(&mut self, cmd: crate::host_cmd::HostCommand) {
        self.evaluate(&cmd.to_js());
    }
    /// 延迟诊断（窗口/webview 状态真值）
    fn diag(&self, _tag: &str) {}
}

/// 枚举显示器（平台分派）。
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    #[cfg(target_os = "macos")]
    return macos::enumerate_monitors();
    #[cfg(target_os = "windows")]
    return windows::enumerate_monitors();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Vec::new()
}

/// 平台分派（Linux 按非目标返回 Unsupported）。
pub fn create_wallpaper_window(
    monitor: &MonitorInfo,
    entry: &gesso_core::LibraryEntry,
) -> gesso_core::Result<Box<dyn WallpaperWindow>> {
    // macOS 视频：原生 AVPlayer 窗（webview 远端层树对视频上屏节奏不均，
    // 60fps 实测 rVFC 53~56/s 波动 = 卡顿；原生管线有显示锁相）。其余照旧 webview。
    #[cfg(target_os = "macos")]
    if entry.kind == gesso_core::WallpaperKind::Video {
        let rel = crate::encoding::entry_main_source(entry);
        let path = std::path::Path::new(&entry.source_dir).join(rel);
        return macos::create_video_window(monitor, &path.display().to_string())
            .map(|w| Box::new(w) as Box<dyn WallpaperWindow>);
    }
    #[cfg(target_os = "macos")]
    return macos::create(monitor).map(|w| Box::new(w) as Box<dyn WallpaperWindow>);
    #[cfg(target_os = "windows")]
    {
        let _ = entry;
        return windows::create(monitor).map(|w| Box::new(w) as Box<dyn WallpaperWindow>);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (monitor, entry);
        Err(gesso_core::GessoError::UnsupportedPlatform(
            "不支持的平台".into(),
        ))
    }
}
