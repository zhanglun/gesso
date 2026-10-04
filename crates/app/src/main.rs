//! gesso —— M2/M3 装配：会话管理器 + 管理窗口三页签 UI（界面与交互设计 v1.0）。
//!
//! UI 数据链（API.md）：读 = 引擎快照 → GessoState（main.rs 装配处单向回灌）；
//! 写 = UI 把 EngineAction 入队（engine.rs），引擎 150ms 轮询执行——与托盘同一通道。
//! UI 不直接触碰 pin/protocol/壁纸窗口生命周期（sync_monitors 独占）。
mod engine;
mod pin;
mod protocol;
mod session;
mod thumb;
mod ui;

use std::time::Duration;

use gesso_core::{AppConfig, LibraryEntry, SessionState, StartupBehavior, WallpaperKind};

use gpui_kit::BorrowAppContext as _;
use ui::app_state::GessoState;

/// 托盘「暂停全部」的当前取向（菜单文案随之切换）。
static TRAY_PAUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 主窗口句柄（托盘「管理窗口…」激活用）。
static MAIN_WINDOW: std::sync::OnceLock<gpui_kit::AnyWindowHandle> = std::sync::OnceLock::new();
/// 托盘勾选镜像（自启翻转判定用；真源 = AppConfig.settings.autostart）。
static AUTOSTART_HINT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 托盘图标源（黑 = macOS template，随菜单栏亮暗自适应；白 = Windows 深色任务栏）。
/// 源文件与再生成见 assets/icons/tools/build.mjs（§DESIGN 品牌图形）。
#[cfg(target_os = "macos")]
const TRAY_PNG: &[u8] = include_bytes!("../assets/icons/tray/trayTemplate@2x.png");
#[cfg(not(target_os = "macos"))]
const TRAY_PNG: &[u8] = include_bytes!("../assets/icons/tray/tray-white-32.png");

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
fn spawn_thumb_job(bg: gpui_kit::gpui::BackgroundExecutor, dir: String) {
    bg.spawn(async move {
        let written = thumb::extract_frames(&dir).written;
        println!("[thumbs] {dir} → 新增 {written} 帧");
        engine::enqueue(engine::EngineAction::ThumbsDone { dir });
    })
    .detach();
}

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
    if !library.iter().any(|e| e.id == "builtin-testsrc") {
        let dst = protocol::library_dir().join("builtin-testsrc");
        std::fs::create_dir_all(&dst).ok();
        let src = protocol::assets_dir().join("samples/testsrc.mp4");
        if src.exists() {
            std::fs::copy(&src, dst.join("index.mp4")).ok();
            library.push(LibraryEntry {
                id: "builtin-testsrc".into(),
                kind: WallpaperKind::Video,
                title: "测试图源（内置）".into(),
                origin: "builtin".into(),
                source_dir: dst.display().to_string(),
            });
            println!("[boot] 内置样例已入库");
        }
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
                WallpaperKind::Image => ui::data::Kind::Gif,
                WallpaperKind::Shader => ui::data::Kind::Shader,
                WallpaperKind::Html => ui::data::Kind::Web,
            };
            ui::data::LibraryItem {
                id: e.id.clone().into(),
                name: e.title.clone().into(),
                kind,
                we: e.origin.starts_with("we-"),
                meta: if e.origin == "builtin" {
                    "内置样例".into()
                } else {
                    e.origin.clone().into()
                },
                assigned: None,
                broken: session::main_asset_name(&e.source_dir, e.kind).is_none(),
                real: true,
                art: kind_art(e.kind),
                thumbs: thumb::existing_frames(&e.source_dir),
            }
        })
        .collect();

    let monitors = sm.monitors();
    let states = sm.states();
    g.monitors = monitors
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let entry_id = sm.config().monitors.get(&m.id).cloned();
            let state = states
                .iter()
                .find(|(mid, _, _)| mid == &m.id)
                .map(|(_, _, s)| *s)
                .unwrap_or(SessionState::Idle);
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
                    SessionState::Autopause => ui::data::PlayState::FullscreenPaused,
                    _ => ui::data::PlayState::UserPaused,
                },
                fps: 60,
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

fn main() {
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
            // Windows：白色字形常驻深色任务栏
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
                let mut tick: u32 = 0;
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(150))
                        .await;
                    // UI/托盘写动作入队（API.md §4）——引擎轮询统一执行
                    let engine_actions = engine::drain();
                    let mut refresh_ui = !engine_actions.is_empty();
                    let mut deferred_window_actions: Vec<engine::EngineAction> = Vec::new();
                    let mut deferred_thumb_requests: Vec<String> = Vec::new();
                    if !engine_actions.is_empty() {
                        cx.update(|cx| {
                            // app（整个全局）与 sm（字段）分开绑定：ThumbsDone 分支
                            // 要写 app.thumbs，与 &mut app.sm 是不相交字段借用
                            let app = cx.global_mut::<engine::AppState>();
                            let sm = &mut app.sm;
                            for a in engine_actions {
                                match a {
                                    engine::EngineAction::Assign {
                                        monitor_id,
                                        entry_id,
                                    } => {
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
                                        match sm.import_entry(std::path::Path::new(&path)) {
                                            Ok(e) => {
                                                println!("[ui] 已导入「{}」→ {}", e.title, e.id);
                                                // 导入后立即后台抽帧（v1 在 import_entry
                                                // 里主线程同步抽 ~1s，导入即卡顿）；统一
                                                // 走 ThumbScheduler 去重/限额
                                                if e.kind == WallpaperKind::Video {
                                                    deferred_thumb_requests
                                                        .push(e.source_dir);
                                                }
                                            }
                                            Err(err) => println!("[ui] 导入失败：{err:?}"),
                                        }
                                    }
                                    engine::EngineAction::UpdateSettings(settings) => {
                                        println!("[ui] 设置更新并落盘");
                                        sm.update_settings(settings);
                                    }
                                    engine::EngineAction::SetAutostart(enable) => {
                                        apply_autostart(enable);
                                    }
                                    engine::EngineAction::Remove { entry_id } => {
                                        println!("[engine] 从库移除 {entry_id}");
                                        sm.remove_entry(&entry_id);
                                    }
                                    engine::EngineAction::SetMonitorFps { monitor_id, fps } => {
                                        println!("[engine] {monitor_id} fps = {fps}");
                                        sm.set_fps(&monitor_id, fps);
                                    }
                                    engine::EngineAction::FocusMainWindow => {
                                        // 需要 cx 的窗口操作：sm 借用结束后在同一闭包内处理
                                        deferred_window_actions.push(a);
                                    }
                                    engine::EngineAction::ThumbsDone { dir } => {
                                        // 后台抽帧完成：释放在途标记（重试计数随之累加）；
                                        // 快照回灌由 refresh_ui 统一触发
                                        app.thumbs.mark_finished(&dir);
                                    }
                                }
                            }
                            for a in deferred_window_actions {
                                if let engine::EngineAction::FocusMainWindow = a {
                                    if let Some(h) = MAIN_WINDOW.get() {
                                        let _ = h.update(
                                            cx,
                                            |_: gpui_kit::gpui::AnyView,
                                             window,
                                             cx: &mut gpui_kit::gpui::App| {
                                                // 顺序关键：先激活 App 再上屏窗口。
                                                // App 未激活时 makeKeyAndOrderFront 会触发
                                                // GPUI 的幽灵 windowDidBecomeKey 处理
                                                // （gpui-pre-macos window.rs ~3135：非 key 态
                                                // 立即 resignKeyWindow），窗口上屏即被打回；
                                                // 先让 NSApp.active 再上屏则不会命中该分支。
                                                cx.activate(true);
                                                window.activate_window();
                                                // 再 defer 一帧补一次上屏：跨 Space 场景下
                                                // 首次 orderFront 可能只切 Space 不上屏
                                                window.defer(cx, |window, cx| {
                                                    window.activate_window();
                                                    cx.activate(true);
                                                });
                                            },
                                        );
                                    }
                                }
                            }
                        });
                    }
                    // 导入触发的立即补帧（已在 ThumbScheduler 标记在途，直接起任务）
                    for dir in deferred_thumb_requests {
                        cx.update(|cx| {
                            cx.global_mut::<engine::AppState>()
                                .thumbs
                                .mark_started(&dir);
                        });
                        spawn_thumb_job(cx.background_executor().clone(), dir);
                    }
                    tick += 1;
                    if tick == 20 || tick == 40 {
                        cx.update(|cx| {
                            cx.global_mut::<engine::AppState>()
                                .sm
                                .diagnose_all(&format!("t{}", tick))
                        });
                    }
                    if tick.is_multiple_of(13) {
                        cx.update(|cx| cx.global_mut::<engine::AppState>().sm.sync_monitors());
                        refresh_ui = true;
                    }
                    if refresh_ui {
                        // 会话 → UI 单向回灌（托盘/界面动作 / 显示器热插拔后的跨面同步）
                        cx.update(|cx| {
                            let next = snapshot_ui(&cx.global::<engine::AppState>().sm);
                            cx.update_global::<GessoState, _>(|g, _| {
                                let (tab, selected, query, filter, demo_imports) = (
                                    g.active_tab,
                                    g.selected.clone(),
                                    g.query.clone(),
                                    g.filter,
                                    g.import_counter,
                                );
                                *g = next;
                                g.active_tab = tab;
                                g.selected = selected;
                                g.query = query;
                                g.filter = filter;
                                g.import_counter = demo_imports;
                                g.demo = false;
                            });
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
                    // 扫描缺帧的视频条目：真源 = 库目录文件系统（existing_frames），
                    // 不依赖 UI 快照投影的新鲜度
                    let targets: Vec<String> = cx.update(|cx| -> Vec<String> {
                        let app = cx.global::<engine::AppState>();
                        app.sm
                            .library()
                            .iter()
                            .filter(|e| e.kind == WallpaperKind::Video)
                            .map(|e| e.source_dir.clone())
                            .filter(|dir| {
                                app.thumbs.should_start(dir)
                                    && thumb::existing_frames(dir).len() < 2
                            })
                            .collect()
                    });
                    for dir in targets {
                        cx.update(|cx| {
                            cx.global_mut::<engine::AppState>()
                                .thumbs
                                .mark_started(&dir);
                        });
                        spawn_thumb_job(cx.background_executor().clone(), dir);
                    }
                }
            })
            .detach();

            // 管理窗口：三页签 UI（§4.3–4.5；44px 顶栏 + 键盘模型 + 双主题）
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
                let _ = MAIN_WINDOW.set(*handle);
            })
            .ok();

            // 首启（config.monitors 为空）：一次性打开向导（§4.6）
            if first_run {
                let _ = ui::first_run::FirstRun::open(cx);
            }

            cx.activate(true);
        });
}
