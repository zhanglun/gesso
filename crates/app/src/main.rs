//! gesso —— M2/M3 装配：会话管理器 + 管理窗口三页签 UI（界面与交互设计 v1.0）。
//!
//! UI 数据链（API.md）：读 = 引擎快照 → GessoState（main.rs 装配处单向回灌）；
//! 写 = UI 把 EngineAction 入队（engine.rs），引擎 150ms 轮询执行——与托盘同一通道。
//! UI 不直接触碰 pin/protocol/壁纸窗口生命周期（sync_monitors 独占）。
mod bridge;
mod encoding;
mod engine;
mod host_cmd;
mod pin;
mod protocol;
mod session;
mod capture;
#[cfg(target_os = "windows")]
mod capture_win;
mod thumb;
mod we;
mod we_shim;
mod ui;

use std::time::Duration;

use gesso_core::{AppConfig, LibraryEntry, SessionState, StartupBehavior, WallpaperKind};

use gpui_kit::BorrowAppContext as _;
use ui::app_state::GessoState;

/// 托盘「暂停全部」的当前取向（菜单文案随之切换）。
static TRAY_PAUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 主窗口句柄（托盘「管理窗口…」激活用）。窗口可被用户关闭（红点），
/// 关闭后 GPUI 会从注册表移除窗口 → 旧句柄失效，需重建而非激活。
static MAIN_WINDOW: std::sync::Mutex<Option<gpui_kit::AnyWindowHandle>> =
    std::sync::Mutex::new(None);
/// 托盘勾选镜像（自启翻转判定用；真源 = AppConfig.settings.autostart）。
static AUTOSTART_HINT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 托盘图标源（黑 = macOS template，随菜单栏亮暗自适应；彩色 compact 应用图标 = Windows，
/// 与任务栏/exe 图标同稿——描边字形缩到托盘 16px 只剩轮廓线，读不出，2026-10-06 实测）。
/// 源文件与再生成见 assets/icons/tools/build.mjs（§DESIGN 品牌图形）。
#[cfg(target_os = "macos")]
const TRAY_PNG: &[u8] = include_bytes!("../assets/icons/tray/trayTemplate@2x.png");
#[cfg(not(target_os = "macos"))]
const TRAY_PNG: &[u8] = include_bytes!("../assets/icons/tray/tray-app-32.png");

/// 解码内嵌 PNG 为托盘 RGBA。macOS 传 44px @2x：tray-icon 按菜单栏 22pt 约束尺寸，
/// 位图仍为 44px → Retina 下清晰。
fn tray_icon_rgba() -> (Vec<u8>, u32, u32) {
    let decoder = png::Decoder::new(std::io::Cursor::new(TRAY_PNG));
    let mut reader = decoder.read_info().expect("托盘 PNG 可解码");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("托盘 PNG 帧读取");
    buf.truncate(info.buffer_size());
    (buf, info.width, info.height)
}

/// cargo run 时让 Dock 显示真实应用图标（打包分发后由 bundle 的 Gesso.icns 接管）。
#[cfg(target_os = "macos")]
fn apply_dock_icon() {
    use objc2::AnyThread as _;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(include_bytes!("../assets/icons/mac/Gesso.icns"));
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    // SAFETY: 主线程（mtm）且 NSApplication 已由 GPUI 启动，官方约束满足
    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image));
    }
}

/// 起一个后台抽帧任务（完成经 `EngineAction::ThumbsDone` 回灌）。
///
/// 调用方必须先经 `ThumbScheduler`（should_start + mark_started）去重/限额——
/// 统一从这两个入口走：导入后立即补帧、30s 扫描兜底。
#[cfg(target_os = "macos")]
fn spawn_thumb_job(bg: gpui_kit::gpui::BackgroundExecutor, dir: String) {
    bg.spawn(async move {
        let written = thumb::extract_frames(&dir).written;
        println!("[thumbs] {dir} → 新增 {written} 帧");
        engine::enqueue(engine::EngineAction::ThumbsDone { dir });
    })
    .detach();
}

/// shader/html 缩略图采集：入串行队列，惰性起一个主线程工作任务逐个消化。
/// 串行是崩溃纪律：并发多 webview + 快照 completion 是崩溃放大器（2026-10-04）。
/// 缩略图策略分派（两处调用共用）：Windows 视频走主线程采集队列（webview 抽帧，
/// M4-W），macOS 维持后台 AVFoundation 抽帧。
fn dispatch_thumb(
    cx: &mut gpui_kit::gpui::AsyncApp,
    dir: String,
    kind: WallpaperKind,
) {
    match gesso_core::content_type(kind).thumb {
        gesso_core::ThumbStrategy::Capture => spawn_capture_job(cx, dir, kind),
        #[cfg(target_os = "windows")]
        gesso_core::ThumbStrategy::Extract => spawn_capture_job(cx, dir, kind),
        #[cfg(target_os = "macos")]
        gesso_core::ThumbStrategy::Extract => {
            spawn_thumb_job(cx.background_executor().clone(), dir)
        }
        _ => {}
    }
}

fn spawn_capture_job(
    cx: &mut gpui_kit::gpui::AsyncApp,
    dir: String,
    kind: WallpaperKind,
) {
    use std::sync::atomic::Ordering;
    capture::CAPTURE_QUEUE.lock().unwrap().push((dir, kind));
    if capture::WORKER_RUNNING.swap(true, Ordering::SeqCst) {
        return; // 已有工作任务在消化队列
    }
    cx.spawn(async move |cx| {
        loop {
            let Some((dir, kind)) = capture::CAPTURE_QUEUE.lock().unwrap().pop() else {
                break;
            };
            let url = cx.update(|cx| {
                let app = cx.global::<engine::AppState>();
                app.sm
                    .library()
                    .iter()
                    .find(|e| e.source_dir == dir)
                    .map(|e| session::SessionManager::entry_host_url(e, 60))
            });
            let written = match url {
                Some(url) => {
                    let bg = cx.background_executor().clone();
                    capture::capture_entry(bg, url, dir.clone(), kind).await
                }
                None => 0,
            };
            println!("[thumbs] {kind:?} {dir} → 新增 {written} 帧");
            engine::enqueue(engine::EngineAction::ThumbsDone { dir });
        }
        capture::WORKER_RUNNING.store(false, Ordering::SeqCst);
    })
    .detach();
}

/// 共享测试锁：所有需要读写进程级环境变量（STEAM_DIR/HOME）的测试必须先持锁。
/// 这些变量是全局的，并行测试同时 set_var 会互相覆盖导致 flaky（见工程笔记）。
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn bootstrap() -> (session::SessionManager, bool) {
    let cfg_path = protocol::config_dir().join("config.json");
    let mut config = AppConfig::load(&cfg_path).unwrap_or_default();
    let first_run = config.monitors.is_empty();

    let mut library = {
        let p = protocol::library_dir().join("library.json");
        gesso_core::LibraryManifest::load(&p)
            .unwrap_or_default()
            .entries
    };
    // 内置样例：视频 1 + shader 4 + html 1（M4 DoD：shader 三样例渲染，noiseflow 含 iChannel 纹理；
    // html 样例演示沙箱契约与 postMessage 暂停配合）
    let builtin_samples: &[(&str, WallpaperKind, &str, &str)] = &[
        (
            "builtin-testsrc",
            WallpaperKind::Video,
            "samples/testsrc.mp4",
            "测试图源（内置）",
        ),
        (
            "builtin-shader-plasma",
            WallpaperKind::Shader,
            "samples/shader/plasma.glsl",
            "Plasma（内置 Shader）",
        ),
        (
            "builtin-shader-aurora",
            WallpaperKind::Shader,
            "samples/shader/aurora.glsl",
            "Aurora（内置 Shader）",
        ),
        (
            "builtin-shader-noiseflow",
            WallpaperKind::Shader,
            "samples/shader/noiseflow.glsl",
            "Noise Flow（内置 Shader · iChannel0）",
        ),
        (
            "builtin-shader-cursor",
            WallpaperKind::Shader,
            "samples/shader/cursor.glsl",
            "Cursor Glow（内置 Shader · 光标跟随）",
        ),
        (
            "builtin-html-clock",
            WallpaperKind::Html,
            "samples/html/clock.html",
            "Clock（内置 HTML）",
        ),
    ];
    for (id, kind, asset, title) in builtin_samples {
        if library.iter().any(|e| e.id == *id) {
            continue;
        }
        let src = protocol::assets_dir().join(asset);
        if !src.exists() {
            continue;
        }
        let dst = protocol::library_dir().join(id);
        std::fs::create_dir_all(&dst).ok();
        // Html 条目的用户页面固定落为 wallpaper.html（index.html 留给宿主页，
        // ensure_entry_host 启动时覆盖写入）；其余类型保持 index.<ext>
        let asset_name = if *kind == WallpaperKind::Html {
            "wallpaper.html".to_string()
        } else {
            let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("bin");
            format!("index.{ext}")
        };
        std::fs::copy(&src, dst.join(asset_name)).ok();
        library.push(LibraryEntry {
            id: (*id).into(),
            kind: *kind,
            title: (*title).into(),
            origin: "builtin".into(),
            source_dir: dst.display().to_string(),
            main_file: None,
        });
        println!("[boot] 内置样例已入库：{id}");
    }
    {
        let p = protocol::library_dir().join("library.json");
        gesso_core::LibraryManifest {
            entries: library.clone(),
        }
        .save(&p)
        .ok();
    }

    if first_run {
        config
            .monitors
            .insert("main".into(), "builtin-testsrc".into());
        config.save(&cfg_path).ok();
        println!("[boot] 首启：主屏指派内置样例");
    }

    let mut sm = session::SessionManager::new(config, library);
    sm.sync_monitors();
    (sm, first_run)
}

/// 会话快照 → UI 状态（GessoState.demo=false）。库/显示器/指派/状态取自真源；
/// 条目失效按素材目录缺失判定（优雅降级：预览灰底 + 红字，永不白屏）。
fn snapshot_ui(sm: &session::SessionManager) -> GessoState {
    let mut g = GessoState {
        demo: false, // 快照覆盖本地投影；调用方回灌时保留浏览状态
        ..GessoState::default()
    };
    // 设置真源 = AppConfig.settings（设置页写经 UpdateSettings 动作落盘）
    let cs = sm.config().settings.clone();
    g.settings = ui::data::Settings {
        fps_cap: cs.fps_cap_default as u32,
        fullscreen: map_policy(cs.fullscreen_policy),
        battery: map_policy(cs.battery_policy),
        idle_downclock: cs.idle_downscale,
        autolaunch: cs.autostart,
        startup_random: cs.startup_behavior == StartupBehavior::Random,
        ..ui::data::Settings::default()
    };
    g.library = sm
        .library()
        .iter()
        .map(|e| {
            let kind = match e.kind {
                WallpaperKind::Video => ui::data::Kind::Video,
                WallpaperKind::Image => {
                    // 底层同一 Image 渲染器；UI 按主资源扩展名细分动图/静态图
                    if let Some(name) = encoding::main_asset_name(&e.source_dir, e.kind) {
                        let ext = std::path::Path::new(&name)
                            .extension().and_then(|x| x.to_str())
                            .unwrap_or("");
                        if gesso_core::is_animated_image_ext(ext) {
                            ui::data::Kind::Gif
                        } else {
                            ui::data::Kind::Photo
                        }
                    } else {
                        ui::data::Kind::Photo
                    }
                }
                WallpaperKind::Shader => ui::data::Kind::Shader,
                WallpaperKind::Html => ui::data::Kind::Web,
            };
            ui::data::LibraryItem {
                id: e.id.clone().into(),
                name: e.title.clone().into(),
                kind,
                we: e.origin == "wallpaper-engine",
                meta: if e.origin == "builtin" {
                    "内置样例".into()
                } else {
                    e.origin.clone().into()
                },
                assigned: None,
                broken: encoding::main_asset_name(&e.source_dir, e.kind).is_none(),
                real: true,
                art: kind_art(e.kind),
                thumbs: thumb::preview_frames(&e.source_dir, e.kind),
            }
        })
        .collect();

    let monitors = sm.monitors();
    let views = sm.views();
    g.monitors = monitors
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let entry_id = sm.config().monitors.get(&m.id).cloned();
            let view = views.iter().find(|v| v.monitor_id == m.id);
            let state = view.map(|v| v.state).unwrap_or(SessionState::Idle);
            ui::data::MonitorEntry {
                name: if m.is_main {
                    "主显示器".into()
                } else {
                    format!("显示器 {}", i + 1).into()
                },
                short: if m.is_main {
                    "主屏".into()
                } else {
                    format!("屏{}", i + 1).into()
                },
                label: format!("{:.0}×{:.0}", m.frame.2.max(1.), m.frame.3.max(1.)).into(),
                rect: (
                    m.frame.0 as f32,
                    m.frame.1 as f32,
                    m.frame.2 as f32,
                    m.frame.3 as f32,
                ),
                is_main: m.is_main,
                wallpaper: entry_id.map(Into::into),
                state: match state {
                    SessionState::Playing | SessionState::Loading => ui::data::PlayState::Playing,
                    SessionState::PausedUser => ui::data::PlayState::UserPaused,
                    SessionState::Autopause => match
                        view.and_then(|v| v.autopause_reason)
                    {
                        Some(session::AutopauseReason::Battery) => {
                            ui::data::PlayState::BatteryPaused
                        }
                        _ => ui::data::PlayState::FullscreenPaused,
                    },
                    _ => ui::data::PlayState::UserPaused,
                },
                fps: sm.fps_for(&m.id) as u32,
                real_id: m.id.clone(),
            }
        })
        .collect();

    // 指派关系：config.monitors（monitor id → entry id）→ LibraryItem.assigned
    for (mid, eid) in sm.config().monitors.iter() {
        let mi = g.monitors.iter().position(|m| &m.real_id == mid);
        if let (Some(mi), Some(item)) = (mi, g.library.iter_mut().find(|w| w.id.as_ref() == eid)) {
            item.assigned = Some(mi);
        }
    }
    g
}

/// 快照回灌：保留 UI 本地的浏览/悬停态，其余字段以引擎快照为准。
/// （AGENTS 规则 2；hovered/selected 丢失曾导致卡片高亮/轮播闪烁。）
fn merge_snapshot(g: &mut GessoState, next: GessoState) {
    let GessoState {
        active_tab,
        selected,
        query,
        filter,
        import_counter,
        hovered,
        hover_frame,
        ..
    } = g;
    let local = (
        *active_tab,
        selected.clone(),
        query.clone(),
        *filter,
        *import_counter,
        hovered.clone(),
        *hover_frame,
    );
    *g = next;
    g.active_tab = local.0;
    g.selected = local.1;
    g.query = local.2;
    g.filter = local.3;
    g.import_counter = local.4;
    g.hovered = local.5;
    g.hover_frame = local.6;
    g.demo = false;
}

fn map_policy(p: gesso_core::PausePolicy) -> ui::data::SuspendPolicy {
    match p {
        gesso_core::PausePolicy::Pause => ui::data::SuspendPolicy::Pause,
        gesso_core::PausePolicy::Downscale => ui::data::SuspendPolicy::Downclock,
        gesso_core::PausePolicy::Ignore => ui::data::SuspendPolicy::Ignore,
    }
}

fn kind_art(kind: WallpaperKind) -> ui::data::Art {
    match kind {
        WallpaperKind::Video => ui::data::Art {
            from: 0x24345C,
            to: 0x0E0F13,
        },
        WallpaperKind::Image => ui::data::Art {
            from: 0x2E3E50,
            to: 0x0E0F13,
        },
        WallpaperKind::Shader => ui::data::Art {
            from: 0x3E2A5E,
            to: 0x0E0F13,
        },
        WallpaperKind::Html => ui::data::Art {
            from: 0x2A2A30,
            to: 0x101012,
        },
    }
}

/// 开机自启（auto-launch：macOS LaunchAgent / Win 注册表 Run 键）。
/// 一条引擎动作执行后，需要主循环继续处理的副作用。
#[derive(Default)]
struct ActionOutcome {
    /// 导入后待生成缩略图的条目（Extract/Capture；Direct 不入列）。
    pending_thumbs: Vec<(String, WallpaperKind)>,
    /// 请求激活/重建主管理窗口（需 window/cx，不能在纯动作处理里完成）。
    focus_main: bool,
}

/// 执行一条引擎动作：只改引擎状态 + 返回需延后的副作用。
/// （窗口生命周期/cx 相关操作不在此处理。）
fn apply_engine_action(
    app: &mut engine::AppState,
    action: engine::EngineAction,
) -> ActionOutcome {
    let mut out = ActionOutcome::default();
    let sm = &mut app.sm;
    match action {
        engine::EngineAction::Assign { monitor_id, entry_id } => {
            println!("[ui] 指派 {entry_id} → {monitor_id}");
            sm.assign(&monitor_id, &entry_id);
        }
        engine::EngineAction::PauseAll(p) => {
            println!("[ui] 暂停全部 = {p}");
            sm.pause_all(p);
        }
        engine::EngineAction::PauseOne { monitor_id, paused } => {
            println!("[ui] 单屏暂停 = {paused}（{monitor_id}）");
            sm.pause_one(&monitor_id, paused);
        }
        engine::EngineAction::SyncMonitors => {
            println!("[ui] 重新检测显示器");
            sm.sync_monitors();
        }
        engine::EngineAction::CycleMain => {
            println!("[ui] 随机换一张（主屏）");
            sm.cycle_main();
        }
        engine::EngineAction::Import { path } => {
            // 选中 WE 的 project.json → 转 WE 整目录导入
            let p0 = std::path::Path::new(&path);
            let we_entry = if p0.file_name().and_then(|n| n.to_str()) == Some("project.json") {
                let dir = p0.parent().unwrap_or(p0);
                let wid = dir.file_name().and_then(|n| n.to_str())
                    .unwrap_or_default().to_string();
                we::import_we_at(dir, &wid)
            } else {
                None
            };
            let result = match we_entry {
                Some(we_e) => sm.import_we_entry(&we_e),
                None => sm.import_entry(p0),
            };
            if let Ok(e) = result {
                println!("[ui] 已导入「{}」→ {}", e.title, e.id);
                if gesso_core::content_type(e.kind).thumb != gesso_core::ThumbStrategy::Direct {
                    out.pending_thumbs.push((e.source_dir, e.kind));
                }
            } else if let Err(err) = result {
                println!("[ui] 导入失败：{err:?}");
            }
        }
        engine::EngineAction::UpdateSettings(settings) => {
            println!("[ui] 设置更新并落盘");
            sm.update_settings(settings);
        }
        engine::EngineAction::SetAutostart(enable) => apply_autostart(enable),
        engine::EngineAction::Remove { entry_id } => {
            println!("[engine] 从库移除 {entry_id}");
            sm.remove_entry(&entry_id);
        }
        engine::EngineAction::SetMonitorFps { monitor_id, fps } => {
            println!("[engine] {monitor_id} fps = {fps}");
            sm.set_fps(&monitor_id, fps);
        }
        engine::EngineAction::FocusMainWindow => out.focus_main = true,
        engine::EngineAction::ThumbsDone { dir } => {
            // 后台抽帧完成：释放在途标记（重试计数随之累加）；快照回灌由主循环触发
            app.thumbs.mark_finished(&dir);
        }
    }
    out
}

/// 激活/重建主管理窗口（FocusMainWindow 副作用的兑现）。
fn focus_or_reopen_main(cx: &mut gpui_kit::gpui::App) {
    let existing = MAIN_WINDOW.lock().ok().and_then(|g| g.clone());
    let mut activated = false;
    if let Some(h) = existing {
        activated = h
            .update(
                cx,
                |_: gpui_kit::gpui::AnyView, window, cx: &mut gpui_kit::gpui::App| {
                    // 顺序关键：先激活 App 再上屏窗口。App 未激活时
                    // makeKeyAndOrderFront 会触发 GPUI 幽灵 windowDidBecomeKey
                    // 处理（非 key 态立即 resignKeyWindow），上屏即被打回。
                    cx.activate(true);
                    window.activate_window();
                    // 跨 Space 场景首次 orderFront 可能只切 Space 不上屏，defer 补一次
                    window.defer(cx, |window, cx| {
                        window.activate_window();
                        cx.activate(true);
                    });
                },
            )
            .is_ok();
    }
    if !activated {
        open_main_window(cx);
        println!("[ui] 管理窗口已重建");
    }
}

fn apply_autostart(enable: bool) {
    let exe = std::env::current_exe().unwrap_or_default();
    let app = auto_launch::AutoLaunchBuilder::new()
        .set_app_name("Gesso")
        .set_app_path(&exe.display().to_string())
        .build();
    let outcome = match app {
        Ok(al) => {
            let r = if enable { al.enable() } else { al.disable() };
            r.map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    };
    println!("[autostart] enable={enable} → {outcome:?}");
}
fn open_main_window(cx: &mut gpui_kit::gpui::App) {
    gpui_kit::open_window(
        gpui_kit::WindowOptions {
            window_bounds: Some(gpui_kit::WindowBounds::Windowed(
                gpui_kit::Bounds::centered(
                    None,
                    gpui_kit::size(gpui_kit::px(880.), gpui_kit::px(600.)),
                    cx,
                ),
            )),
            // 顶栏自绘：产品名 + 三页签；窗口控制（红绿灯）由系统提供
            titlebar: Some(gpui_kit::TitlebarOptions {
                title: Some("Gesso".into()),
                appears_transparent: true,
                traffic_light_position: Some(gpui_kit::point(
                    gpui_kit::px(14.),
                    gpui_kit::px(15.),
                )),
            }),
            ..Default::default()
        },
        cx,
        |window, cx| {
            // 亮暗跟随系统（DESIGN.md「GPUI 落地」）
            // Subscription 必须 detach 保活，否则跟随系统主题立即失效
            window
                .observe_window_appearance(|window, cx| {
                    ui::theme::sync_on_appearance_change(window, cx);
                })
                .detach();
            use gpui_kit::AppContext as _;
            cx.new(|cx| ui::shell::Shell::new(window, cx))
        },
    )
    .inspect(|(handle, _)| {
        if let Ok(mut g) = MAIN_WINDOW.lock() {
            *g = Some(*handle);
        }
    })
    .ok();
}

fn main() {
    // Windows 平台面（M1，技术方案 §4.1）：进程必须先于任何窗口创建声明 PerMonitorV2，
    // 否则显示器枚举/窗口定位拿到的是虚拟化坐标，与 DPI-aware 的 explorer/WorkerW
    // 无法像素对齐。GPUI 与 wry 均不设置（实测 0.3.7 快照），此处运行时调用等效于
    // manifest 声明，必须放在 gpui_kit::application() 之前。
    // GPUI DirectX 渲染器默认走 DirectComposition 视觉树，与 WebView2 同进程共存
    // 有合成冲突风险（ROADMAP M1 配方）——禁用之（仅影响管理窗口渲染路径）。
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "1");
        // SAFETY: 进程级一次性设置，任何窗口创建之前；已设置时失败无害
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    }

    // 单实例（§4.1 对策 5）。GESSO_LOCK 供开发期多实例并存（UI 验收 vs 会话调试）。
    let lock_name = std::env::var("GESSO_LOCK").unwrap_or_else(|_| "gesso-app-lock".into());
    {
        let si = single_instance::SingleInstance::new(&lock_name).unwrap();
        if !si.is_single() {
            eprintln!("[boot] 已有实例运行，退出");
            return;
        }
        std::mem::forget(si);
    }

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            ui::theme::init(cx);
            #[cfg(target_os = "macos")]
            apply_dock_icon();

            // 顺序关键：NSApplication 就绪后才能建 AppKit 壁纸窗口
            let (sm, first_run) = bootstrap();
            println!(
                "[boot] 会话数：{}（first_run={first_run}）",
                sm.session_count()
            );

            // 引擎全局（API.md §4：cx.set_global(AppState::new(sm))）+ UI 快照注入
            let autostart_on = sm.config().settings.autostart;
            AUTOSTART_HINT.store(autostart_on, std::sync::atomic::Ordering::SeqCst);
            cx.set_global(engine::AppState::new(sm));
            cx.set_global(snapshot_ui(&cx.global::<engine::AppState>().sm));

            // 托盘（§4.1/§4.2）：左键 = 快速面板；右键 = 菜单
            use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
            let menu = Menu::new();
            let mi_pause = MenuItem::with_id("pause", "暂停全部壁纸", true, None);
            let mi_cycle = MenuItem::with_id("cycle", "随机换一张", true, None);
            let mi_main = MenuItem::with_id("main", "管理窗口…", true, None);
            let mi_auto = CheckMenuItem::with_id("autostart", "开机自启", true, autostart_on, None);
            let mi_quit = MenuItem::with_id("quit", "退出 Gesso", true, None);
            menu.append_items(&[
                &mi_pause,
                &mi_cycle,
                &PredefinedMenuItem::separator(),
                &mi_main,
                &PredefinedMenuItem::separator(),
                &mi_auto,
                &PredefinedMenuItem::separator(),
                &mi_quit,
            ])
            .expect("菜单");
            MenuEvent::set_event_handler(Some(|e: MenuEvent| {
                use engine::EngineAction;
                match e.id().as_ref() {
                    // 菜单文案恒为「暂停全部壁纸」（muda handler 要求 Send，不能持菜单句柄改文案；
                    // 当前状态经托盘图标/状态矩阵反映）
                    "pause" => {
                        // fetch_xor 翻转并返回旧值（swap(true) 会永远读到同一个旧值 →
                        // 该项变成"只暂停不恢复"，M0.5 踩过同款坑）
                        let paused =
                            !TRAY_PAUSED.fetch_xor(true, std::sync::atomic::Ordering::SeqCst);
                        engine::enqueue(EngineAction::PauseAll(paused));
                    }
                    "cycle" => engine::enqueue(EngineAction::CycleMain),
                    "main" => engine::enqueue(EngineAction::FocusMainWindow),
                    "autostart" => {
                        // CheckMenuItem 在 macOS 原生翻转；此处读翻转后的近似值
                        let now = !AUTOSTART_HINT.load(std::sync::atomic::Ordering::SeqCst);
                        AUTOSTART_HINT.store(now, std::sync::atomic::Ordering::SeqCst);
                        engine::enqueue(EngineAction::SetAutostart(now));
                    }
                    "quit" => std::process::exit(0),
                    _ => {}
                }
            }));
            let (rgba, w, h) = tray_icon_rgba();
            let icon = tray_icon::Icon::from_rgba(rgba, w, h).unwrap();
            let mut tray_builder = tray_icon::TrayIconBuilder::new()
                .with_tooltip("Gesso")
                .with_menu(Box::new(menu));
            // macOS：黑字形 + template 标志，随菜单栏亮暗自适应；
            // Windows：彩色 compact 应用图标（与任务栏图标同稿，描边字形在托盘尺寸读不出）
            #[cfg(target_os = "macos")]
            {
                tray_builder = tray_builder.with_icon_templated(icon);
            }
            #[cfg(not(target_os = "macos"))]
            {
                tray_builder = tray_builder.with_icon(icon);
            }
            let tray = tray_builder
                // 左/右键都弹菜单（§4.1，2026-10-03 决策：纯菜单形态）
                .build()
                .expect("托盘");
            Box::leak(Box::new(tray));

            cx.spawn(async move |cx| {
                // 定时基准：150ms 一拍。各周期用拍数命名，不再出现裸魔数。
                const MONITOR_SYNC_EVERY: u32 = 13; // 显示器热插拔检测 ≈2.0s
                const AUTOPAUSE_OFFSET: u32 = 6; // 自动暂停检测，与同步错开半拍
                const TIME_TICK_EVERY: u32 = 7; // 挂钟脉冲 ≈1.05s
                const DIAGNOSE_AT: [u32; 2] = [20, 40]; // 启动后诊断快照（3s/6s）
                let mut tick: u32 = 0;
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(150))
                        .await;
                    // UI/托盘写动作入队（API.md §4）——引擎轮询统一执行
                    let engine_actions = engine::drain();
                    let mut refresh_ui = !engine_actions.is_empty();
                    let mut pending_thumbs: Vec<(String, WallpaperKind)> = Vec::new();
                    if !engine_actions.is_empty() {
                        let mut focus_main = false;
                        cx.update(|cx| {
                            for a in engine_actions {
                                let out = apply_engine_action(
                                    cx.global_mut::<engine::AppState>(),
                                    a,
                                );
                                pending_thumbs.extend(out.pending_thumbs);
                                if out.focus_main {
                                    focus_main = true;
                                }
                            }
                        });
                        // FocusMainWindow 需 window/cx：借用结束后兑现
                        if focus_main {
                            cx.update(|cx| focus_or_reopen_main(cx));
                        }
                    }

                    // 导入触发的缩略图生成：统一标记在途，按策略起对应任务
                    // （Direct 图片类型不在此列——源文件直接可显示）
                    for (dir, kind) in pending_thumbs {
                        cx.update(|cx| {
                            cx.global_mut::<engine::AppState>()
                                .thumbs
                                .mark_started(&dir);
                        });
                        dispatch_thumb(cx, dir, kind);
                    }
                    tick += 1;
                    if DIAGNOSE_AT.contains(&tick) {
                        cx.update(|cx| {
                            cx.global_mut::<engine::AppState>()
                                .sm
                                .diagnose_all(&format!("t{}", tick))
                        });
                    }
                    if tick.is_multiple_of(MONITOR_SYNC_EVERY) {
                        cx.update(|cx| {
                            // M1 Windows：explorer 重启自愈——TaskbarCreated 广播置位后，
                            // 各会话壁纸窗口重走挂载阶梯 + 重建 webview（合成随 SetParent
                            // 失效）。广播意味着 explorer 壳层已就绪（Progman 必在），
                            // 重挂最差落到 Progman 兜底，无需额外重试节奏。
                            #[cfg(target_os = "windows")]
                            if pin::windows::take_remount_pending() {
                                // 整窗重建；explorer 未就绪（BottomMost）时重新置位，下轮重试
                                let ok = cx.global_mut::<engine::AppState>().sm.remount_all();
                                if !ok {
                                    pin::windows::rearm_remount();
                                }
                            }
                            cx.global_mut::<engine::AppState>().sm.sync_monitors();
                        });
                        refresh_ui = true;
                    }
                    // M5 数据桥（与显示器同步同节奏，错开半拍）：全屏检测 + 电源态
                    // → 策略解析 → 自动暂停/降帧；有状态变化才刷新 UI
                    if tick != 0 && tick % MONITOR_SYNC_EVERY == AUTOPAUSE_OFFSET {
                        let changed = cx.update(|cx| {
                            let snap = bridge::sample();
                            cx.global_mut::<engine::AppState>()
                                .sm
                                .apply_autopause(&snap.fullscreen, snap.on_battery)
                        });
                        refresh_ui |= changed;
                    }
                    // 时间脉冲（≈1.05s）：时钟类壁纸的挂钟由引擎驱动
                    if tick.is_multiple_of(TIME_TICK_EVERY) {
                        cx.update(|cx| {
                            cx.global_mut::<engine::AppState>()
                                .sm
                                .broadcast_time_tick()
                        });
                    }
                    if refresh_ui {
                        // 会话 → UI 单向回灌（托盘/界面动作 / 显示器热插拔后的跨面同步）
                        cx.update(|cx| {
                            let next = snapshot_ui(&cx.global::<engine::AppState>().sm);
                            cx.update_global::<GessoState, _>(|g, _| merge_snapshot(g, next));
                            cx.refresh_windows();
                        });
                    }
                }
            })
            .detach();

            // 缩略图抽帧扫描（独立任务，30s 一轮兜底：启动时/导入外的缺帧补齐）。
            // v1 把它挂在 150ms tick 的 tick%200 上并用 timer(2s) 猜"生成完了没"，
            // 且无限重试——现在完成事件（ThumbsDone）驱动回灌，重试上限与在途
            // 去重收敛在 ThumbScheduler。
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(30)).await;
                    // 缺帧兜底：统一按策略扫描——Direct（图片）帧发现本就直引源文件，
                    // 只对需"生成"且当前缺帧的 Extract/Capture 起任务。
                    // 真源 = 库目录文件系统，不依赖 UI 快照新鲜度。
                    let targets: Vec<(String, WallpaperKind)> = cx.update(|cx| -> _ {
                        let app = cx.global::<engine::AppState>();
                        app.sm
                            .library()
                            .iter()
                            .filter(|e| {
                                gesso_core::content_type(e.kind).thumb != gesso_core::ThumbStrategy::Direct
                            })
                            .filter(|e| {
                                app.thumbs.should_start(&e.source_dir)
                                    && thumb::preview_frames(&e.source_dir, e.kind).len() < 2
                            })
                            .map(|e| (e.source_dir.clone(), e.kind))
                            .collect()
                    });
                    for (dir, kind) in targets {
                        cx.update(|cx| {
                            cx.global_mut::<engine::AppState>()
                                .thumbs
                                .mark_started(&dir);
                        });
                        dispatch_thumb(cx, dir, kind);
                    }
                }
            })
            .detach();

            // M5 光标 feed：后台只出 33ms 节拍，读 AppKit 位置 + 比对 + 推送
            // 全在主线程。根因教训：NSEvent.mouseLocation 从后台线程读在快速
            // 更新时是陈旧值（光团卡住、停后才闪现到终点）——AppKit 界面状态
            // 必须主线程读。poll_mouse 内部量化去重，静止不产生 evaluate。
            // Windows（M5-W）：GetCursorPos + GetAsyncKeyState + GetLastInputInfo
            // 同语义实现（bridge/windows.rs），左下契约翻转在 poll_mouse 边界。
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(33))
                        .await;
                    let sample = cx.update(|_| bridge::sample_mouse());
                    cx.update(|cx| {
                        cx.global_mut::<engine::AppState>()
                            .sm
                            .poll_mouse(&sample)
                    });
                }
            })
            .detach();

            // 管理窗口：三页签 UI（§4.3–4.5；44px 顶栏 + 键盘模型 + 双主题）
            open_main_window(cx);

            // 首启（config.monitors 为空）：一次性打开向导（§4.6）
            if first_run {
                let _ = ui::first_run::FirstRun::open(cx);
            }

            cx.activate(true);
        });
}
