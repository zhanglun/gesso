//! Windows 资源内嵌：把 win/gesso.ico 以资源 ID 1 塞进 exe。
//!
//! gpui-pre-windows 启动时经 `LoadImageW(module, MAKEINTRESOURCE(1), IMAGE_ICON)`
//! 取窗口类图标（platform.rs::load_icon），取不到就 `unwrap_or_default()` 静默降级
//! 为空图标 → 任务栏只能显示 Windows 通用图标（蓝条白窗）。没有内嵌资源时 GPUI
//! 不会报错，只有任务栏图标不对——所以这条接线必须在这里，且 ID 必须是 1。
//! （macOS 的 Dock 图标走 main.rs::apply_dock_icon 的 icns，与这里无关。）

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icons/win/gesso.ico");
        winresource::WindowsResource::new()
            .set_icon_with_id("assets/icons/win/gesso.ico", "1")
            .compile()
            .expect("内嵌 Windows 图标资源（win/gesso.ico → ID 1）");
    }
}
