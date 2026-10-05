//! 平台贴壁层：创建并管理"钉在桌面图标层之下"的壁纸窗口。
//!
//! M1.5 定稿架构（SPIKE-REPORT）：壁纸窗口是**本模块自有的原生窗口**
//! （macOS: 纯 AppKit NSWindow；Windows 将来: 自有 Win32 窗口），
//! **不经 GPUI 窗口管理**——GPUI 协调器与桌面级全屏几何必然冲突。
//! webview 由 wry 直挂窗口 contentView。

#[cfg(target_os = "macos")]
pub mod macos;

/// 显示器信息（枚举产物，稳定 ID 是配置映射的 key）。
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    /// 稳定 ID：macOS v1 用 CGDirectDisplayID（`cg-<id>`）；
    /// 升级路径：EDID 哈希（技术方案 §4.3）
    pub id: String,
    #[allow(dead_code)] // 展示名：macOS 枚举填充；UI 投影用自己的命名规则（主屏/屏N）
    pub name: String,
    /// AppKit 全局坐标（原点左下），含菜单栏/Dock 区域
    pub frame: (f64, f64, f64, f64), // x, y, w, h
    pub is_main: bool,
}

/// 壁纸窗口的统一操作面（由各平台模块实现）。
/// 部分方法当前调用方未接（Windows M1 / 露桌面回退），属平台面而非死代码。
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
    /// 当前 URL（诊断用：判断导航是否被 WKWebView 拒绝）
    fn current_url(&self) -> String;
    /// 透传 JS（M5 数据桥通道：时间 tick / 降帧 setFps；暂停仍走 set_paused）。
    fn evaluate(&mut self, _js: &str) {}
    /// 发送一条类型化宿主命令（默认走 evaluate；平台可覆写）。
    fn send(&mut self, cmd: crate::host_cmd::HostCommand) {
        self.evaluate(&cmd.to_js());
    }
    /// 延迟诊断（窗口/webview 状态真值）
    fn diag(&self, _tag: &str) {}
}

/// 枚举显示器（平台分派；M1 起 Windows 用 EnumDisplayMonitors 接入）。
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    #[cfg(target_os = "macos")]
    return macos::enumerate_monitors();
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

/// 平台分派（M1 起补 Windows 分支；Linux 按非目标返回 Unsupported）。
pub fn create_wallpaper_window(
    monitor: &MonitorInfo,
) -> gesso_core::Result<Box<dyn WallpaperWindow>> {
    #[cfg(target_os = "macos")]
    return macos::create(monitor).map(|w| Box::new(w) as Box<dyn WallpaperWindow>);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = monitor; // Windows 分支 M1 接入后消费
        Err(gesso_core::GessoError::UnsupportedPlatform(
            std::env::consts::OS.into(),
        ))
    }
}
