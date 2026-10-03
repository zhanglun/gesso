//! gesso —— M2/M3 装配：会话管理器 + 管理窗口三页签 UI（界面与交互设计 v1.0）。
//!
//! UI 数据链（API.md）：读 = 引擎快照 → GessoState（main.rs 装配处单向回灌）；
//! 写 = UI 把 EngineAction 入队（engine.rs），引擎 150ms 轮询执行——与托盘同一通道。
//! UI 不直接触碰 pin/protocol/壁纸窗口生命周期（sync_monitors 独占）。
mod engine;
mod pin;
mod protocol;
mod session;
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

fn tray_icon_rgba() -> Vec<u8> {
    let (w, h) = (32usize, 32usize);
    let mut v = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let edge = x < 2 || y < 2 || x >= w - 2 || y >= h - 2;
            let bar = (8..24).contains(&x) && (14..18).contains(&y);
            let (r, g, b) = if edge {
                (0x1E, 0x3B, 0x8F)
            } else if bar {
                (0xFF, 0xFF, 0xFF)
            } else {
                (0x31, 0x6E, 0xF5)
            };
            let i = (y * w + x) * 4;
            v[i] = r;
            v[i + 1] = g;
            v[i + 2] = b;
            v[i + 3] = 0xFF;
        }
    }
    v
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
            let icon = tray_icon::Icon::from_rgba(tray_icon_rgba(), 32, 32).unwrap();
            let tray = tray_icon::TrayIconBuilder::new()
                .with_tooltip("Gesso")
                .with_icon(icon)
                .with_menu(Box::new(menu))
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
                    if !engine_actions.is_empty() {
                        cx.update(|cx| {
                            let sm = &mut cx.global_mut::<engine::AppState>().sm;
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
                                                println!("[ui] 已导入「{}」→ {}", e.title, e.id)
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
                                    engine::EngineAction::FocusMainWindow => {
                                        // 需要 cx 的窗口操作：sm 借用结束后在同一闭包内处理
                                        deferred_window_actions.push(a);
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
                                                window.activate_window();
                                                cx.activate(true);
                                            },
                                        );
                                    }
                                }
                            }
                        });
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
