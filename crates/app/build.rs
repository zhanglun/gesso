//! Windows 资源内嵌：把 win/gesso.ico 以资源 ID 1 塞进 exe。
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
        "web/package.json",
        "web/package-lock.json",
        "web/vite.config.js",
        "web/jsconfig.json",
        // 目录形式递归监听（新增组件文件也会触发重建）
        "web/src",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    let web_dir = std::path::Path::new("web");
    let node_modules = web_dir.join("node_modules");
    if !node_modules.exists() {
        run_web_command("npm", &["ci"], web_dir);
    }
    run_web_command("npm", &["run", "build"], web_dir);

    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let output = out_dir.join("web");
    std::fs::create_dir_all(output.join("assets")).expect("创建 Web 资源输出目录");
    for (source, target) in [
        ("dist/index.html", "index.html"),
        ("dist/assets/app.js", "assets/app.js"),
        ("dist/assets/app.css", "assets/app.css"),
    ] {
        std::fs::copy(web_dir.join(source), output.join(target))
            .unwrap_or_else(|error| panic!("复制 Web 资源 {source}: {error}"));
    }
}

fn run_web_command(program: &str, args: &[&str], dir: &std::path::Path) {
    let status = std::process::Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|error| panic!("运行 {program} {args:?} 失败：{error}"));
    assert!(status.success(), "运行 {program} {args:?} 失败：{status}");
}
