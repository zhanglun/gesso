//! 构建期资源内嵌：
//! 1. Web 控制台静态资产（web/ → OUT_DIR/web/）：纯静态文件直接复制，无 npm/Node 依赖。
//!    前端为 Preact + HTM 免构建形态（vendor/ 内置运行时），产物 = index.html + assets/ 四个 js/css。
//! 2. Windows 资源内嵌：把 win/gesso.ico 以资源 ID 1 塞进 exe。
//!
//! gpui-pre-windows 启动时经 `LoadImageW(module, MAKEINTRESOURCE(1), IMAGE_ICON)`
//! 取窗口类图标（platform.rs::load_icon），取不到就 `unwrap_or_default()` 静默降级
//! 为空图标 → 任务栏只能显示 Windows 通用图标（蓝条白窗）。没有内嵌资源时 GPUI
//! 不会报错，只有任务栏图标不对——所以这条接线必须在这里，且 ID 必须是 1。
//! （macOS 的 Dock 图标走 main.rs::apply_dock_icon 的 icns，与这里无关。）

fn main() {
    build_web_assets();

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/icons/win/gesso.ico");
        winresource::WindowsResource::new()
            .set_icon_with_id("assets/icons/win/gesso.ico", "1")
            .compile()
            .expect("内嵌 Windows 图标资源（win/gesso.ico → ID 1）");
    }
}

fn build_web_assets() {
    for path in [
        "web/index.html",
        "web/src/app.js",
        "web/src/styles.css",
        "web/vendor/preact.js",
        "web/vendor/hooks.js",
        "web/vendor/htm.js",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    let web_dir = std::path::Path::new("web");
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let output = out_dir.join("web");
    std::fs::create_dir_all(output.join("assets")).expect("创建 Web 资源输出目录");
    for (source, target) in [
        ("index.html", "index.html"),
        ("src/app.js", "assets/app.js"),
        ("src/styles.css", "assets/app.css"),
        ("vendor/preact.js", "assets/preact.js"),
        ("vendor/hooks.js", "assets/hooks.js"),
        ("vendor/htm.js", "assets/htm.js"),
    ] {
        std::fs::copy(web_dir.join(source), output.join(target))
            .unwrap_or_else(|error| panic!("复制 Web 资源 {source}: {error}"));
    }
}
