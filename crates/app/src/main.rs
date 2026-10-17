//! gesso —— M2/M3 装配：会话管理器 + 管理窗口三页签 UI（界面与交互设计 v1.0）。
//!
//! release 改 GUI 子系统：不随启动弹终端黑窗（诊断已由 panic 落盘日志兜底，
//! debug 构建保留控制台承接 stdout 日志）。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//!
//! UI 数据链（API.md）：读 = 引擎快照 → GessoState（main.rs 装配处单向回灌）；
//! 写 = UI 把 EngineAction 入队（engine.rs），引擎 150ms 轮询执行——与托盘同一通道。
//! UI 不直接触碰 pin/protocol/壁纸窗口生命周期（sync_monitors 独占）。
mod bridge;
mod capture;
#[cfg(target_os = "windows")]
mod capture_win;
mod desktop_bridge;
mod encoding;
mod engine;
mod host_cmd;
mod pet_adoption_view;
mod pet_cache;
mod pet_store;
mod pet_window;
mod pin;
mod protocol;
mod session;
mod thumb;
mod ui;
mod update;
mod we;
mod we_shim;

use std::time::{Duration, Instant};

use gesso_core::{
    AppConfig, LibraryEntry, PetSave, PetSeed, SessionState, StartupBehavior, WallpaperKind,
};

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
fn dispatch_thumb(cx: &mut gpui_kit::gpui::AsyncApp, dir: String, kind: WallpaperKind) {
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

fn spawn_capture_job(cx: &mut gpui_kit::gpui::AsyncApp, dir: String, kind: WallpaperKind) {
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
                    // 采集队列键 = 缩略图家目录（本地/WE = source_dir；远端 = 库内 <id>/）
                    .find(|e| encoding::thumb_home(e) == dir)
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
    // 启动即按配置设定界面语言（UI 创建前；之后设置页切换实时更新）
    ui::strings::set_lang(config.settings.language);
    // 首启判定 = initialized 标志未落盘。显示器指派被清空不是首启——
    // 不重指派样例；老版本升级用户（无此键）按首启补一次样例指派。
    let first_run = !config.initialized;

    let mut library = {
        let p = protocol::library_dir().join("library.json");
        match gesso_core::LibraryManifest::load(&p) {
            Ok(m) => m.entries,
            Err(_) if !p.exists() => Vec::new(), // 真首启：无清单
            // 灾难护栏：清单存在但解析失败（损坏/手工编辑错）时，绝不能
            // 以空库继续启动——下方播种会把它覆写、用户条目全部丢失
            //（2026-10-10 实测事故）。保留原文件供修复，仅内存降级。
            Err(e) => {
                eprintln!("[boot] library.json 解析失败（{e}），跳过播种覆写：{p:?}");
                std::process::exit(78); // EX_CONFIG：带明确原因退出，交由用户修文件
            }
        }
    };
    // 预置壁纸：视频 1 + shader 8 + html 1（M4 DoD：shader 渲染，noiseflow 含 iChannel
    // 纹理，cursor 含 iMouse 光标桥；html 演示沙箱契约与 postMessage 暂停配合）。
    // 标题即产品文案：不带「内置/样例/测试」措辞；本表是预置条目标题的唯一事实源
    //（播种时对已存在条目做标题刷新，升级改名不残留旧文案）。
    let builtin_samples: &[(&str, WallpaperKind, &str, &str)] = &[
        (
            "builtin-testsrc",
            WallpaperKind::Video,
            "samples/testsrc.mp4",
            "Spectrum",
        ),
        (
            "builtin-shader-plasma",
            WallpaperKind::Shader,
            "samples/shader/plasma.glsl",
            "Plasma",
        ),
        (
            "builtin-shader-aurora",
            WallpaperKind::Shader,
            "samples/shader/aurora.glsl",
            "Aurora",
        ),
        (
            "builtin-shader-noiseflow",
            WallpaperKind::Shader,
            "samples/shader/noiseflow.glsl",
            "Noise Flow",
        ),
        (
            "builtin-shader-cursor",
            WallpaperKind::Shader,
            "samples/shader/cursor.glsl",
            "Cursor Glow",
        ),
        (
            "builtin-shader-tide",
            WallpaperKind::Shader,
            "samples/shader/tide.glsl",
            "Tide",
        ),
        (
            "builtin-shader-starfield",
            WallpaperKind::Shader,
            "samples/shader/starfield.glsl",
            "Starfield",
        ),
        (
            "builtin-shader-silk",
            WallpaperKind::Shader,
            "samples/shader/silk.glsl",
            "Silk",
        ),
        (
            "builtin-shader-ember",
            WallpaperKind::Shader,
            "samples/shader/ember.glsl",
            "Ember",
        ),
        (
            "builtin-html-clock",
            WallpaperKind::Html,
            "samples/html/clock.html",
            "Clock",
        ),
    ];
    for (id, kind, asset, title) in builtin_samples {
        let src = protocol::assets_dir().join(asset);
        if !src.exists() {
            // 资源缺失（安装包不完整/开发目录异常）——静默跳过会让「可用壁纸变少」
            // 且无处可查，至少留一行日志
            println!("[boot] 预置壁纸资源缺失，跳过入库：{asset}");
            continue;
        }
        // Html 条目的用户页面固定落为 wallpaper.html（index.html 留给宿主页，
        // ensure_entry_host 启动时覆盖写入）；其余类型保持 index.<ext>
        let asset_name = if *kind == WallpaperKind::Html {
            "wallpaper.html".to_string()
        } else {
            let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("bin");
            format!("index.{ext}")
        };
        // 已入库的预置条目：刷新标题 + 重拷资源——预置内容随包升级（预置条目
        // 用户不可改名/改内容，无覆盖风险；不重拷的话改版内容永远到不了老用户）
        if let Some(e) = library.iter_mut().find(|e| e.id == *id) {
            if e.origin == "builtin" {
                if e.title != *title {
                    e.title = (*title).into();
                    println!("[boot] 预置壁纸标题更新：{id} → {title}");
                }
                let dst = protocol::library_dir().join(id);
                std::fs::create_dir_all(&dst).ok();
                std::fs::copy(&src, dst.join(asset_name)).ok();
            }
            continue;
        }
        let dst = protocol::library_dir().join(id);
        std::fs::create_dir_all(&dst).ok();
        std::fs::copy(&src, dst.join(asset_name)).ok();
        library.push(LibraryEntry {
            id: (*id).into(),
            kind: *kind,
            title: (*title).into(),
            origin: "builtin".into(),
            source_dir: dst.display().to_string(),
            main_file: None,
            source_url: None,
        });
        println!("[boot] 预置壁纸已入库：{id}");
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
        config.initialized = true;
        // 首启一次性指派内置样例：仅当从未指派过（真实新装）。升级用户已有
        // 指派、或日后主动清空指派的配置，都不覆盖（§4.6 变更记录）。
        if config.monitors.is_empty() {
            config
                .monitors
                .insert("main".into(), "builtin-testsrc".into());
            println!("[boot] 首启：主屏指派内置样例");
        }
        config.save(&cfg_path).ok();
    }

    let mut sm = session::SessionManager::new(config, library);
    sm.sync_monitors();

    // 桌面桥接对账：硬杀残留的接管记录按当前活跃指派清理；开关关闭则全量
    // 恢复（desktop_bridge.rs 模块注释——所有权纪律与已知边界）
    let active: std::collections::BTreeSet<String> =
        sm.config().monitors.values().cloned().collect();
    let bridge_on = sm.config().settings.desktop_bridge;
    desktop_bridge::reconcile(&active, bridge_on);

    // 开发验证钩子（仅 debug 构建）：GESSO_DEV_URL=<https 地址> 启动即导入并指派
    // 主屏，用于无头验证远端网页链路（跨域 iframe 装载/缩略图），如
    // GESSO_DEV_URL=https://louie.co.nz/25th_hour/ GESSO_LOCK=dev ./target/debug/gesso
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("GESSO_DEV_URL") {
        match sm.import_url_entry(&url) {
            Ok(e) => {
                if let Some(m) = sm.monitors().first() {
                    println!("[boot][dev] 远端网页验证：{} → {}", e.id, m.id);
                    sm.assign(&m.id, &e.id);
                }
            }
            Err(err) => println!("[boot][dev] GESSO_DEV_URL 导入失败：{err:?}"),
        }
    }

    (sm, first_run)
}

/// 会话快照 → UI 状态（GessoState.demo=false）。库/显示器/指派/状态取自真源；
/// 条目失效按素材目录缺失判定（优雅降级：预览灰底 + 红字，永不白屏）。
fn snapshot_ui(
    sm: &session::SessionManager,
    thumbs: &engine::ThumbScheduler,
    pet: &PetSave,
) -> GessoState {
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
        language: match cs.language {
            gesso_core::config::Language::Auto => 0,
            gesso_core::config::Language::Zh => 1,
            gesso_core::config::Language::En => 2,
        },
        update_check: cs.update_check,
        desktop_bridge: cs.desktop_bridge,
        ..ui::data::Settings::default()
    };
    // 宠物投影：设置页只消费快照，不直接读取 AppState。
    let pet_save = sm.config();
    let _ = pet_save;
    // 宠物投影：设置页只消费快照，不直接读取 AppState。
    g.pet = ui::data::PetView {
        adopted: pet.adoption.is_some(),
        activity: pet.activity,
        size: pet.size,
        visible: pet.visible,
        stats: ui::data::PetStats {
            companion_seconds: pet.stats.companion_seconds,
            touch_count: pet.stats.touch_count,
            poke_count: pet.stats.poke_count,
        },
    };
    // 软件更新投影（§4.7；真源 = update 模块全局，随快照单向回灌）
    g.update = update::public_view();
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
                            .extension()
                            .and_then(|x| x.to_str())
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
            // 缩略图"生成中"投影：需生成的类型 + 当前缺帧 + 调度器仍会重试
            // （在途或次数未耗尽）——耗尽后退回静态占位，不假装在加载
            let home = encoding::thumb_home(e);
            let frames = thumb::preview_frames(&home, e.kind);
            ui::data::LibraryItem {
                thumb_pending: gesso_core::content_type(e.kind).thumb
                    != gesso_core::ThumbStrategy::Direct
                    && frames.len() < 2
                    && thumbs.pending(&home),
                id: e.id.clone().into(),
                name: e.title.clone().into(),
                kind,
                we: e.origin == "wallpaper-engine",
                // 远端网页条目：meta 行展示来源域名
                meta: if let Some(url) = &e.source_url {
                    encoding::host_of_url(url).unwrap_or("url").into()
                } else if e.origin == "builtin" {
                    ui::strings::META_BUILTIN().into()
                } else {
                    e.origin.clone().into()
                },
                assigned: None,
                // 远端条目无本地素材，不按文件缺失判失效（断网 = 空白帧降级）
                broken: e.source_url.is_none() && !encoding::main_asset_exists(e),
                remote: e.source_url.is_some(),
                real: true,
                art: kind_art(e.kind),
                thumbs: frames,
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
                    ui::strings::MON_MAIN().into()
                } else {
                    ui::strings::monitor_name(i + 1).into()
                },
                short: if m.is_main {
                    ui::strings::MON_MAIN_SHORT().into()
                } else {
                    ui::strings::monitor_short(i + 1).into()
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
                    SessionState::Autopause => match view.and_then(|v| v.autopause_reason) {
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
    /// 请求打开/激活「软件更新」小窗（§4.7 托盘入口；设置页在 UI 线程直接开）。
    open_update_modal: bool,
    /// 界面语言变化（托盘菜单文案需重建，见主循环）。
    lang_changed: bool,
    /// 请求打开宠物领养小窗。
    open_pet_adoption: bool,
    /// 互动统计已在引擎内更新，刷新宠物投影。
    refresh_pet: bool,
}

/// 托盘菜单（文案唯一出处 = ui::strings；语言切换后主循环重建整个菜单，
/// 事件经全局 handler 按 id 路由，重建不影响逻辑）。
fn build_tray_menu_with_pet(
    autostart_on: bool,
    adopted: bool,
    pet_visible: bool,
) -> tray_icon::menu::Menu {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
    let menu = Menu::new();
    let mi_pause = MenuItem::with_id("pause", ui::strings::TRAY_PAUSE_ALL(), true, None);
    let mi_cycle = MenuItem::with_id("cycle", ui::strings::TRAY_RANDOM(), true, None);
    let pet_label = if adopted {
        ui::strings::TRAY_PET_SETTINGS()
    } else {
        ui::strings::TRAY_PET_ADOPT()
    };
    let mi_pet = MenuItem::with_id("pet-adopt", pet_label, true, None);
    let mi_pet_visibility = adopted.then(|| {
        MenuItem::with_id(
            "pet-toggle",
            if pet_visible {
                ui::strings::TRAY_PET_HIDE()
            } else {
                ui::strings::TRAY_PET_SHOW()
            },
            true,
            None,
        )
    });
    let mi_main = MenuItem::with_id("main", ui::strings::TRAY_MAIN_WINDOW(), true, None);
    let mi_update = MenuItem::with_id("check-update", ui::strings::TRAY_CHECK_UPDATE(), true, None);
    let mi_auto = CheckMenuItem::with_id(
        "autostart",
        ui::strings::TRAY_AUTOSTART(),
        true,
        autostart_on,
        None,
    );
    let mi_quit = MenuItem::with_id("quit", ui::strings::TRAY_QUIT(), true, None);
    let mi_restore = MenuItem::with_id(
        "restore-wallpaper",
        ui::strings::TRAY_RESTORE_WALLPAPER(),
        true,
        None,
    );
    let sep_pet = PredefinedMenuItem::separator();
    let sep_settings = PredefinedMenuItem::separator();
    let sep_exit = PredefinedMenuItem::separator();
    let mut items: Vec<&dyn tray_icon::menu::IsMenuItem> =
        vec![&mi_pause, &mi_cycle, &sep_pet, &mi_pet];
    if let Some(item) = mi_pet_visibility.as_ref() {
        items.push(item);
    }
    items.push(&mi_main);
    items.push(&mi_update);
    items.push(&sep_settings);
    items.push(&mi_auto);
    items.push(&sep_exit);
    items.push(&mi_restore);
    items.push(&mi_quit);
    menu.append_items(&items).expect("菜单");
    menu
}

/// 托盘菜单事件（一次性注册；菜单重建后 id 不变，处理逻辑无需重挂）。
fn install_tray_menu_handler() {
    tray_icon::menu::MenuEvent::set_event_handler(Some(|e: tray_icon::menu::MenuEvent| {
        use engine::EngineAction;
        match e.id().as_ref() {
            // 菜单文案恒为「暂停全部壁纸」（muda handler 要求 Send，不能持菜单句柄改文案；
            // 当前状态经托盘图标/状态矩阵反映）
            "pause" => {
                // fetch_xor 翻转并返回旧值（swap(true) 会永远读到同一个旧值 →
                // 该项变成"只暂停不恢复"，M0.5 踩过同款坑）
                let paused = !TRAY_PAUSED.fetch_xor(true, std::sync::atomic::Ordering::SeqCst);
                engine::enqueue(EngineAction::PauseAll(paused));
            }
            "cycle" => engine::enqueue(EngineAction::CycleMain),
            "pet-adopt" => engine::enqueue(EngineAction::OpenPetAdoption),
            "pet-toggle" => engine::enqueue(EngineAction::TogglePetVisibility),
            "main" => engine::enqueue(EngineAction::FocusMainWindow),
            // 逃生门：全量恢复系统壁纸（重新指派壁纸会按设置再次接管）
            "restore-wallpaper" => engine::enqueue(EngineAction::RestoreSystemWallpaper),
            // 「检查更新…」：动作带开窗意图（托盘线程不能直接开 GPUI 窗口，
            // 主循环兑现）；检查本身在 update 模块后台线程
            "check-update" => engine::enqueue(EngineAction::UpdateCheck { open_modal: true }),
            "autostart" => {
                // CheckMenuItem 在 macOS 原生翻转；此处读翻转后的近似值
                let now = !AUTOSTART_HINT.load(std::sync::atomic::Ordering::SeqCst);
                AUTOSTART_HINT.store(now, std::sync::atomic::Ordering::SeqCst);
                engine::enqueue(EngineAction::SetAutostart(now));
            }
            // 退出必须走引擎动作（不能在此 process::exit）：要在引擎线程上先拆
            // 壁纸会话并通知 shell 重绘，直接退进程会让桌面留黑底
            "quit" => engine::enqueue(EngineAction::Quit),
            _ => {}
        }
    }));
}

/// 构建托盘本体（菜单文案取当前语言；图标平台差异见内注）。
fn build_tray_with_pet(
    autostart_on: bool,
    adopted: bool,
    pet_visible: bool,
) -> tray_icon::TrayIcon {
    let (rgba, w, h) = tray_icon_rgba();
    let icon = tray_icon::Icon::from_rgba(rgba, w, h).unwrap();
    let mut tray_builder = tray_icon::TrayIconBuilder::new()
        .with_tooltip("Gesso")
        .with_menu(Box::new(build_tray_menu_with_pet(
            autostart_on,
            adopted,
            pet_visible,
        )));
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
    // 左/右键都弹菜单（§4.1，2026-10-03 决策：纯菜单形态）
    tray_builder.build().expect("托盘")
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn persist_pet_adoption(
    app: &mut engine::AppState,
    candidates: gesso_core::CandidateSet,
) -> gesso_core::AdoptionState {
    let Some(index) = candidates.selected else {
        return gesso_core::AdoptionState::CandidateSelected(candidates);
    };
    if app.pet_storage_blocked {
        let message = "宠物存档不可写，原文件已保留".to_string();
        eprintln!("[pet] {message}");
        app.pet_error = Some(message);
        return gesso_core::adoption_transition(
            gesso_core::AdoptionState::Persisting { candidates },
            gesso_core::AdoptionEvent::PersistFailed,
        );
    }

    let candidate = candidates.candidates[index].clone();
    let pet_id = format!("pet-{}", PetSeed::random().to_hex());
    let pet = match gesso_core::PetIndividual::from_candidate(candidate, pet_id, unix_timestamp()) {
        Ok(pet) => pet,
        Err(error) => {
            let message = error.to_string();
            eprintln!("[pet] 领养个体创建失败：{message}");
            app.pet_error = Some(message);
            return gesso_core::adoption_transition(
                gesso_core::AdoptionState::Persisting { candidates },
                gesso_core::AdoptionEvent::PersistFailed,
            );
        }
    };
    let cache = pet_cache::PetCacheStore::new(protocol::pet_cache_dir());
    let cache_dir = match cache.commit(&pet) {
        Ok(path) => path,
        Err(error) => {
            let message = error.to_string();
            eprintln!("[pet] 领养缓存失败：{message}");
            app.pet_error = Some(message);
            return gesso_core::adoption_transition(
                gesso_core::AdoptionState::Persisting { candidates },
                gesso_core::AdoptionEvent::PersistFailed,
            );
        }
    };
    let mut save = app.pet_save.clone();
    save.adoption = Some(pet);
    match app.pet_store.save(&save) {
        Ok(()) => {
            app.pet_error = None;
            app.pet_save = save;
            gesso_core::adoption_transition(
                gesso_core::AdoptionState::Persisting { candidates },
                gesso_core::AdoptionEvent::PersistSucceeded,
            )
        }
        Err(error) => {
            let _ = cache.remove(&cache_dir);
            let message = error.to_string();
            eprintln!("[pet] 领养存档失败：{message}");
            app.pet_error = Some(message);
            gesso_core::adoption_transition(
                gesso_core::AdoptionState::Persisting { candidates },
                gesso_core::AdoptionEvent::PersistFailed,
            )
        }
    }
}

/// 执行一条引擎动作：只改引擎状态 + 返回需延后的副作用。
/// （窗口生命周期/cx 相关操作不在此处理。）
fn apply_engine_action(app: &mut engine::AppState, action: engine::EngineAction) -> ActionOutcome {
    let mut out = ActionOutcome::default();
    let sm = &mut app.sm;
    match action {
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
        engine::EngineAction::RestoreSystemWallpaper => {
            let n = desktop_bridge::restore_all();
            println!("[ui] 恢复系统壁纸：{n} 屏已还原");
        }
        engine::EngineAction::Import { path } => {
            // 选中 WE 的 project.json → 转 WE 整目录导入
            let p0 = std::path::Path::new(&path);
            let we_entry = if p0.file_name().and_then(|n| n.to_str()) == Some("project.json") {
                let dir = p0.parent().unwrap_or(p0);
                let wid = dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_string();
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
                    out.pending_thumbs.push((encoding::thumb_home(&e), e.kind));
                }
            } else if let Err(err) = result {
                println!("[ui] 导入失败：{err:?}");
            }
        }
        engine::EngineAction::ImportUrl { url } => {
            // 远端网页（§4.3 🔗）：UI 预检已过，引擎侧再走同一 parse 规则兜底
            match sm.import_url_entry(&url) {
                Ok(e) => {
                    println!("[ui] 已导入远端网页「{}」← {url}", e.title);
                    if gesso_core::content_type(e.kind).thumb != gesso_core::ThumbStrategy::Direct {
                        out.pending_thumbs.push((encoding::thumb_home(&e), e.kind));
                    }
                }
                Err(err) => println!("[ui] 远端网页导入失败：{err:?}"),
            }
        }
        engine::EngineAction::UpdateSettings(settings) => {
            println!("[ui] 设置更新并落盘");
            out.lang_changed = sm.config().settings.language != settings.language;
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
        engine::EngineAction::OpenPetAdoption => out.open_pet_adoption = true,
        engine::EngineAction::StartPetAdoption { species } => {
            let event = gesso_core::AdoptionEvent::Start {
                species,
                session_seed: PetSeed::random(),
                generator_version: gesso_core::GENERATOR_VERSION,
                asset_version: gesso_core::ASSET_VERSION,
            };
            app.pet_adoption = gesso_core::adoption_transition(
                std::mem::replace(&mut app.pet_adoption, gesso_core::AdoptionState::NotAdopted),
                event,
            );
        }
        engine::EngineAction::PetAdoption(event) => {
            let current =
                std::mem::replace(&mut app.pet_adoption, gesso_core::AdoptionState::NotAdopted);
            let next = gesso_core::adoption_transition(current, event);
            let gesso_core::AdoptionState::Persisting { candidates } = next else {
                app.pet_adoption = next;
                return out;
            };

            app.pet_adoption = persist_pet_adoption(app, candidates);
        }
        engine::EngineAction::PetInteraction(interaction) => {
            if app.pet_save.adoption.is_some() && !app.pet_storage_blocked {
                app.pet_save.record_interaction(interaction);
                app.pet_stats_dirty = true;
                out.refresh_pet = true;
            }
        }
        engine::EngineAction::PetBehavior(event) => {
            app.pet_behavior.handle(event);
        }
        engine::EngineAction::SavePetPosition(position) => {
            let mut save = app.pet_save.clone();
            save.position = Some(position);
            match app.pet_store.save(&save) {
                Ok(()) => {
                    app.pet_save = save;
                    app.pet_error = None;
                }
                Err(error) => {
                    let message = error.to_string();
                    eprintln!("[pet] 位置存档失败：{message}");
                    app.pet_error = Some(message);
                }
            }
        }
        engine::EngineAction::SetPetActivity(activity) => {
            if app.pet_save.adoption.is_some() {
                app.pet_save.activity = activity;
                app.pet_behavior.set_activity(activity);
                app.pet_stats_dirty = true;
                out.refresh_pet = true;
            }
        }
        engine::EngineAction::SetPetSize(size) => {
            if app.pet_save.adoption.is_some() {
                app.pet_save.size = size;
                app.pet_stats_dirty = true;
                out.refresh_pet = true;
            }
        }
        engine::EngineAction::TogglePetVisibility => {
            if app.pet_save.adoption.is_some() {
                let mut save = app.pet_save.clone();
                save.visible = !save.visible;
                match app.pet_store.save(&save) {
                    Ok(()) => {
                        app.pet_error = None;
                        app.pet_save = save;
                    }
                    Err(error) => {
                        let message = error.to_string();
                        eprintln!("[pet] 显示状态保存失败：{message}");
                        app.pet_error = Some(message);
                    }
                }
            }
        }
        engine::EngineAction::FocusMainWindow => out.focus_main = true,
        // 软件更新（§4.7）：检查/下载在 update 模块自有线程执行（阻塞 IO），
        // 进度与结果经状态代数回灌主循环（下方 update 代数观察），不走 ThumbsDone 式回执
        engine::EngineAction::UpdateCheck { open_modal } => {
            println!("[engine] 检查更新（open_modal={open_modal}）");
            out.open_update_modal = open_modal;
            update::spawn_check();
        }
        engine::EngineAction::UpdateDownload => {
            println!("[engine] 下载更新安装包");
            update::spawn_download();
        }
        engine::EngineAction::UpdateCancelDownload => {
            println!("[engine] 取消下载更新");
            update::cancel_download();
        }
        engine::EngineAction::ThumbsDone { dir } => {
            // 后台抽帧完成：释放在途标记（重试计数随之累加）；快照回灌由主循环触发
            app.thumbs.mark_finished(&dir);
        }
        // 退出：引擎轮询线程正是壁纸窗口创建/销毁约束的主线程（pin Drop 注），
        // 拆会话 → 通知 shell 重绘露出原壁纸 → 退进程（此前托盘线程直接
        // process::exit，Drop 全跳过，桌面留黑底）。
        engine::EngineAction::Quit => {
            app.sm.teardown_all();
            #[cfg(target_os = "windows")]
            pin::windows::restore_desktop();
            std::process::exit(0);
        }
    }
    out
}

/// 找主管理窗口 HWND：本进程 + 标题 Gesso + 无属主（壁纸 overlay 与托盘监听窗
/// 无标题，标题条件即排除）。EnumWindows 对隐藏窗口同样枚举，隐藏态找得到。
#[cfg(target_os = "windows")]
fn find_main_hwnd() -> Option<windows::Win32::Foundation::HWND> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowTextW, GetWindowThreadProcessId, GW_OWNER,
    };

    // 找到即写入槽位并停枚举（与 pin::windows find_defview_proc 同款通道）
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let slot = &mut *(lparam.0 as *mut Option<HWND>);
        if slot.is_some() {
            return BOOL(0);
        }
        let mut owner_pid = 0u32;
        let mut buf = [0u16; 32];
        let want: Vec<u16> = "Gesso".encode_utf16().collect();
        let len =
            // SAFETY: hwnd 由系统枚举给出；buf 为调用方栈上缓冲，只读查询
            unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
        let hit = unsafe {
            // SAFETY: 同上，GetWindowThreadProcessId/GetWindow 均为只读查询
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut owner_pid));
            let no_owner = GetWindow(hwnd, GW_OWNER)
                .map(|h| h.is_invalid())
                .unwrap_or(true);
            owner_pid == std::process::id()
                && len == want.len()
                && buf[..len] == want[..]
                && no_owner
        };
        if hit {
            *slot = Some(hwnd);
            return BOOL(0); // 找到即停
        }
        BOOL(1)
    }

    let mut found: Option<HWND> = None;
    // SAFETY: 回调经槽位指针写局部变量，枚举同步完成；失败时 found 保持 None
    unsafe {
        let _ = EnumWindows(
            Some(enum_proc),
            LPARAM(&mut found as *mut Option<HWND> as isize),
        );
    }
    found
}

/// 托盘/自绘 ✕ 共用的「关闭=隐藏」（设计规格：管理窗口 关闭=隐藏）。
/// 进程退出只经托盘「退出 Gesso」→ EngineAction::Quit（先拆壁纸会话再退）。
#[cfg(target_os = "windows")]
pub(crate) fn hide_main_window() {
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    if let Some(h) = find_main_hwnd() {
        // SAFETY: hwnd 属本进程主窗口
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
        println!("[ui] 主窗口已隐藏（托盘常驻）");
    }
}

/// 托盘「管理窗口…」重显隐藏态主窗口（SW_HIDE 后 activate_window 无效，先 SW_SHOW）。
#[cfg(target_os = "windows")]
fn show_main_window() {
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOW};
    if let Some(h) = find_main_hwnd() {
        // SAFETY: hwnd 属本进程主窗口
        unsafe {
            let _ = ShowWindow(h, SW_SHOW);
        }
    }
}

/// 首实例侧：守着激活事件，第二实例一出现就把管理窗口带回前台。
/// enqueue 是 Mutex 队列，后台线程安全；真正上屏由引擎轮询在主线程完成。
#[cfg(target_os = "windows")]
fn watch_first_instance_activate(suffix: String) {
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};
    let name = windows::core::HSTRING::from(format!(r"Local\gesso-activate{suffix}"));
    // SAFETY: 命名事件为本应用私有（同名约定）；CreateEventW 对已存在事件等同打开
    let h = unsafe { CreateEventW(None, false, false, &name) };
    let Ok(h) = h else {
        return;
    };
    loop {
        // SAFETY: h 为本线程持有的事件句柄，无限等待；WAIT_OBJECT_0 = 0
        if unsafe { WaitForSingleObject(h, INFINITE) }.0 == 0 {
            println!("[boot] 第二实例请求上屏主窗口");
            engine::enqueue(engine::EngineAction::FocusMainWindow);
        }
    }
}

/// 第二实例侧：置位激活事件后立刻退出（本进程）。
#[cfg(target_os = "windows")]
fn signal_first_instance(suffix: &str) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{CreateEventW, SetEvent};
    let name = windows::core::HSTRING::from(format!(r"Local\gesso-activate{suffix}"));
    // SAFETY: 同名事件，打开（不存在则创建）→ 置位 → 关句柄
    unsafe {
        if let Ok(h) = CreateEventW(None, false, false, &name) {
            let _ = SetEvent(h);
            let _ = CloseHandle(h);
        }
    }
}

/// 激活/重建主管理窗口（FocusMainWindow 副作用的兑现）。
fn focus_or_reopen_main(cx: &mut gpui_kit::gpui::App) {
    let existing = MAIN_WINDOW.lock().ok().and_then(|g| *g);
    let mut activated = false;
    if let Some(h) = existing {
        // 关闭=隐藏的窗口先 SW_SHOW，否则 activate_window 对隐藏窗口无效
        #[cfg(target_os = "windows")]
        show_main_window();
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
                traffic_light_position: Some(gpui_kit::point(gpui_kit::px(14.), gpui_kit::px(15.))),
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
            // 关闭=隐藏（设计规格「管理窗口…关闭=隐藏」）：Alt+F4/任务栏关闭拦下
            // 藏回托盘；自绘 ✕ 走同一 hide 路径（ui/shell.rs）。不拦的话 Windows 上
            // 最后一个窗口关闭 = gpui 事件循环退出 = 整个进程退出。macOS 红绿灯
            // 关窗后应用本就存活，维持默认。返回 false = 否决关闭。
            #[cfg(target_os = "windows")]
            window.on_window_should_close(cx, |_, _| {
                hide_main_window();
                false
            });
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

/// panic 落盘：GUI 双击闪退（尤其 Windows console 子系统黑窗一闪而过）
/// 无任何痕迹，无法远程诊断（issue #2）。把 panic 追加写进配置目录 gesso.log，
/// 带版本与时间戳；stderr 照田输出。
fn install_panic_logger() {
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let entry = format!(
            "gesso {} @ unix {ts}\n[panic] {info}\n{bt}\n---\n",
            env!("CARGO_PKG_VERSION")
        );
        eprintln!("{entry}");
        let log = protocol::config_dir().join("gesso.log");
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
        {
            use std::io::Write as _;
            let _ = f.write_all(entry.as_bytes());
        }
        // 异常退出尽力清场（Windows）：销毁存活的壁纸窗口 + 触发桌面重绘，
        // 让用户壁纸回来而非黑屏。任何失败静默——进程随后即死。
        #[cfg(target_os = "windows")]
        pin::windows::emergency_teardown();
        // 桌面桥接恢复（macOS）：AppKit 只能主线程调——主线程 panic 直接
        // 恢复；后台线程 panic 跳过，留待下次启动对账（desktop_bridge.rs）
        #[cfg(target_os = "macos")]
        if objc2::MainThreadMarker::new().is_some() {
            desktop_bridge::restore_all();
        }
    }));
}

fn main() {
    install_panic_logger();
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
    let lock_suffix = std::env::var("GESSO_LOCK").unwrap_or_default();
    // 锁名按平台给：Windows 的 single-instance 0.3 把名字原样传 CreateMutexW——
    // 那是内核对象名，`\` 会被当命名空间分隔符且中间组件必须已存在，
    // 文件路径形态（无论 "." 还是 %USERPROFILE%）一律 ERROR_PATH_NOT_FOUND(3)
    // → unwrap panic → 双击秒退（issue #2 闪退根因）。给扁平会话局部名；
    // 非 Windows 走文件锁，仍须绝对路径：Finder/open 启动 cwd=/（只读），
    // 相对路径 create 会 EPERM（macOS 双击没反应的旧坑）。
    #[cfg(windows)]
    let lock_name = format!(r"Local\gesso-app-lock{lock_suffix}");
    #[cfg(not(windows))]
    let lock_name = protocol::config_dir()
        .join(format!("gesso-app-lock{lock_suffix}"))
        .display()
        .to_string();
    // 真新装时 config 目录尚不存在（bootstrap 在锁之后才落 config）：
    // 不先建目录文件锁会 NotFound → unwrap 恐慌 → 首启秒退
    std::fs::create_dir_all(protocol::config_dir()).ok();
    {
        let si = single_instance::SingleInstance::new(&lock_name).unwrap();
        if !si.is_single() {
            // 关闭=隐藏后主窗口常驻托盘：再次双击图标/开始菜单启动 = 第二实例，
            // 唤醒首实例把管理窗口带回前台（否则关进托盘后没有回头路）
            #[cfg(target_os = "windows")]
            signal_first_instance(&lock_suffix);
            eprintln!("[boot] 已有实例运行，已请求其上屏主窗口，本实例退出");
            return;
        }
        std::mem::forget(si);
    }

    // 视频媒体回环服务：会话构建（ContentSpec → media_url）前必须就绪
    protocol::start_media_server();

    // 更新下载目录清场（§4.7）：.part 半成品与已交付的旧安装包不跨会话保留
    update::cleanup_download_dir();

    // Windows：监听后续第二实例的激活信号（自动重置事件，快速连点也不丢信号：
    // 无等待者时 SetEvent 会记忆信号态）
    #[cfg(target_os = "windows")]
    {
        let suffix = lock_suffix.clone();
        std::thread::spawn(move || watch_first_instance_activate(suffix));
    }

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            ui::theme::init(cx);
            #[cfg(target_os = "macos")]
            apply_dock_icon();

            // 显示器拓扑监听先于会话建立：唤醒/重配事件驱动收敛（ARCHITECTURE §2），
            // 事件只翻转纪元代数，下方引擎心跳去抖消费
            pin::install_topology_watch();

            // 顺序关键：NSApplication 就绪后才能建 AppKit 壁纸窗口
            let (sm, first_run) = bootstrap();
            println!(
                "[boot] 会话数：{}（first_run={first_run}）",
                sm.session_count()
            );

            // 引擎全局（API.md §4：cx.set_global(AppState::new(sm))）+ UI 快照注入
            let autostart_on = sm.config().settings.autostart;
            AUTOSTART_HINT.store(autostart_on, std::sync::atomic::Ordering::SeqCst);
            let pet_store = pet_store::PetStore::new(protocol::pet_save_path());
            let (pet_save, pet_storage_blocked) = match pet_store.load() {
                Ok(save) => (save, false),
                Err(error @ pet_store::PetStoreError::CorruptBackedUp { .. }) => {
                    eprintln!("[pet] 存档已备份，使用空存档启动：{error}");
                    (PetSave::default(), false)
                }
                Err(error) => {
                    eprintln!("[pet] 存档未加载，宠物存档进入只读保护：{error}");
                    (PetSave::default(), true)
                }
            };
            cx.set_global(engine::AppState::new(
                sm,
                pet_store,
                pet_save,
                pet_storage_blocked,
            ));
            cx.set_global(pet_window::PetWindowManager::default());
            cx.set_global(snapshot_ui(
                &cx.global::<engine::AppState>().sm,
                &cx.global::<engine::AppState>().thumbs,
                &cx.global::<engine::AppState>().pet_save,
            ));

            // 托盘（§4.1/§4.2）：左键 = 快速面板；右键 = 菜单。
            // 事件处理器全局按 id 路由，一次注册即可；语言切换后主循环
            // 用同一 build_tray 重建菜单（id 不变，处理逻辑无需重挂）。
            install_tray_menu_handler();
            let (pet_adopted, pet_visible) = {
                let app = cx.global::<engine::AppState>();
                (app.pet_save.adoption.is_some(), app.pet_save.visible)
            };
            let tray: &'static tray_icon::TrayIcon = Box::leak(Box::new(build_tray_with_pet(
                autostart_on,
                pet_adopted,
                pet_visible,
            )));

            cx.spawn(async move |cx| {
                // 定时基准：150ms 一拍。各周期用拍数命名，不再出现裸魔数。
                const PATROL_EVERY: u32 = 33; // 稳态巡检（深收敛）≈5s：事件丢失/静默篡改的安全网
                const AUTOPAUSE_EVERY: u32 = 13; // 自动暂停检测 ≈2.0s（全屏/电池/锁屏采样）
                const AUTOPAUSE_OFFSET: u32 = 6; // 自动暂停检测，与巡检错开半拍
                const TIME_TICK_EVERY: u32 = 7; // 挂钟脉冲 ≈1.05s
                const DIAGNOSE_AT: [u32; 2] = [20, 40]; // 启动后诊断快照（3s/6s）
                const UPDATE_FIRST_TICK: u32 = 100; // 更新首查 ≈启动后 15s（避开启动 I/O 风暴）
                const UPDATE_AUTO_PROBE: u32 = 6000; // 更新探测节拍 ≈15min（模块内按 24h 真抑制）
                                                     // 静默期 1.5s：显示握手（模式集/EDID/排布）通常 1–2s 内走完，
                                                     // 首个采样要大概率落在终态上——300ms 时采到的是过渡几何
                                                     //（实机伪影：开盖小窗、合盖超大图一闪）。陈旧期 4s 封顶。
                const TOPOLOGY_QUIET: Duration = Duration::from_millis(1500);
                const TOPOLOGY_MAX_WAIT: Duration = Duration::from_millis(4000);
                const RECONFIRM_EVERY: u32 = 3; // commit 收敛：settle 突变轮后 ≈450ms
                let mut tick: u32 = 0;
                let mut last_update_gen: u64 = update::generation();
                // 拓扑纪元消费状态：首/末事件时刻（去抖窗口）。事件驱动的收敛
                // 触发，替代旧「2s 轮询发现拓扑变化」——轮询既有延迟（错误中间态
                // 肉眼可见）又会采到重配序列的半程状态。
                let mut last_epoch = pin::topology_epoch();
                let mut topo_first: Option<Instant> = None;
                let mut topo_last: Option<Instant> = None;
                // 两段式提交的 commit 段调度：settle 轮（拓扑收敛）有突变时不
                // 复显，等这一拍到期再收敛一次——枚举已稳定则统一点亮。过渡
                // 几何（半程枚举）从构造上不可见。
                let mut reconfirm_at: Option<u32> = None;
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(150))
                        .await;
                    // debug 专用模拟钩子：无头验证退出/异常退出路径（托盘退出
                    // 走同一 EngineAction::Quit；panic 走同一 panic 钩子）。
                    // release 构建不存在，无环境变量时零开销。
                    #[cfg(debug_assertions)]
                    {
                        static SIM_T0: std::sync::OnceLock<std::time::Instant> =
                            std::sync::OnceLock::new();
                        let elapsed = SIM_T0
                            .get_or_init(std::time::Instant::now)
                            .elapsed()
                            .as_secs();
                        let env_secs =
                            |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u64>().ok());
                        if env_secs("GESSO_QUIT_AFTER_SECS").is_some_and(|n| elapsed >= n) {
                            engine::enqueue(engine::EngineAction::Quit);
                        }
                        if env_secs("GESSO_PANIC_AFTER_SECS").is_some_and(|n| elapsed >= n) {
                            panic!("GESSO_PANIC_AFTER_SECS 模拟异常退出（壁纸播放中 panic）");
                        }
                    }
                    // UI/托盘写动作入队（API.md §4）——引擎轮询统一执行
                    let engine_actions = engine::drain();
                    let mut refresh_ui = !engine_actions.is_empty();
                    let mut pending_thumbs: Vec<(String, WallpaperKind)> = Vec::new();
                    if !engine_actions.is_empty() {
                        let mut focus_main = false;
                        let mut open_update_modal = false;
                        let mut open_pet_adoption = false;
                        let mut lang_changed = false;
                        let mut refresh_pet = false;
                        cx.update(|cx| {
                            for a in engine_actions {
                                let out =
                                    apply_engine_action(cx.global_mut::<engine::AppState>(), a);
                                pending_thumbs.extend(out.pending_thumbs);
                                if out.focus_main {
                                    focus_main = true;
                                }
                                if out.open_update_modal {
                                    open_update_modal = true;
                                }
                                if out.open_pet_adoption {
                                    open_pet_adoption = true;
                                }
                                if out.lang_changed {
                                    lang_changed = true;
                                }
                                if out.refresh_pet {
                                    refresh_pet = true;
                                }
                            }
                            // 语言切换：以新语言重建托盘菜单（勾选态和宠物状态取真源）。
                            if lang_changed {
                                let (autostart_on, adopted, pet_visible) = {
                                    let app = cx.global::<engine::AppState>();
                                    (
                                        app.sm.config().settings.autostart,
                                        app.pet_save.adoption.is_some(),
                                        app.pet_save.visible,
                                    )
                                };
                                tray.set_menu(Some(Box::new(build_tray_menu_with_pet(
                                    autostart_on,
                                    adopted,
                                    pet_visible,
                                ))));
                            }
                        });
                        refresh_ui |= refresh_pet;
                        // FocusMainWindow 需 window/cx：借用结束后兑现
                        if focus_main {
                            cx.update(focus_or_reopen_main);
                        }
                        // 更新小窗同理（托盘「检查更新…」入口的 open_modal 兑现）
                        if open_update_modal {
                            cx.update(ui::update_view::open);
                        }
                        if open_pet_adoption {
                            cx.update(pet_adoption_view::open);
                        }
                    }

                    // 互动统计低频落盘：内存先更新，约 1s 一次合并写盘。
                    if tick.is_multiple_of(TIME_TICK_EVERY) {
                        cx.update(|cx| {
                            let app = cx.global_mut::<engine::AppState>();
                            if !app.pet_stats_dirty || app.pet_storage_blocked {
                                return;
                            }
                            match app.pet_store.save(&app.pet_save) {
                                Ok(()) => {
                                    app.pet_stats_dirty = false;
                                    app.pet_error = None;
                                }
                                Err(error) => {
                                    let message = error.to_string();
                                    eprintln!("[pet] 互动统计存档失败：{message}");
                                    app.pet_error = Some(message);
                                }
                            }
                        });
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
                    // 拓扑纪元：事件驱动的收敛触发。事件在静默期（300ms 无新事件）
                    // 或陈旧期上限（2s，防事件连发饿死）到达时兑现——等重配序列
                    // 走完再采样。收敛走两段式：settle 轮（拓扑/commit 轮）突变
                    // 不复显，安排 commit 轮（≈450ms 后）在枚举稳定时统一点亮；
                    // 巡检每 ≈5s 深收敛兜底（稳定语境，突变即复显）。
                    let epoch = pin::topology_epoch();
                    if epoch != last_epoch {
                        last_epoch = epoch;
                        let now = Instant::now();
                        topo_first.get_or_insert(now);
                        topo_last = Some(now);
                        // 消失屏的熄窗由 30ms 快速观察器执行（本拍的对齐延迟
                        // 会让残窗可见一闪）；此处只负责去抖计时。
                    }
                    let reconfirm_now = matches!(reconfirm_at, Some(t) if tick >= t);
                    if reconfirm_now {
                        reconfirm_at = None;
                    }
                    let patrol_due = tick.is_multiple_of(PATROL_EVERY);
                    let topo_fire = topo_last
                        .is_some_and(|l| Instant::now().duration_since(l) >= TOPOLOGY_QUIET)
                        || topo_first
                            .is_some_and(|f| Instant::now().duration_since(f) >= TOPOLOGY_MAX_WAIT);
                    if patrol_due || topo_fire || reconfirm_now {
                        let settling = topo_fire || reconfirm_now;
                        let deep = patrol_due && !settling;
                        topo_first = None;
                        topo_last = None;
                        let mut mutated = false;
                        cx.update(|cx| {
                            let sm = &mut cx.global_mut::<engine::AppState>().sm;
                            mutated = if deep {
                                sm.patrol();
                                false
                            } else {
                                sm.sync_monitors_settling()
                            };
                        });
                        if settling && mutated {
                            reconfirm_at = Some(tick + RECONFIRM_EVERY);
                        }
                        refresh_ui = true;
                    }
                    // M1 Windows：explorer 重启自愈——TaskbarCreated 广播置位后即刻
                    // 重钉（原 2s 轮询取走，现在每拍检查，延迟 ≤150ms）。广播意味着
                    // explorer 壳层已就绪（Progman 必在），重挂最差落到 Progman 兜底。
                    // 各会话壁纸窗口重走挂载阶梯 + 重建 webview（合成随 SetParent 失效）；
                    // explorer 未就绪（BottomMost）时重新置位，下拍重试。
                    #[cfg(target_os = "windows")]
                    if pin::windows::take_remount_pending() {
                        let ok = cx.update(|cx| {
                            let sm = &mut cx.global_mut::<engine::AppState>().sm;
                            let ok = sm.remount_all();
                            if ok {
                                sm.sync_monitors(); // 重钉后对齐一轮（原同分支语义）
                            }
                            ok
                        });
                        if !ok {
                            pin::windows::rearm_remount();
                        }
                    }
                    // M5 数据桥（≈2s 节拍，错开半拍）：锁屏 + 全屏 + 电源态
                    // → 策略解析 → 自动暂停/降帧；有状态变化才刷新 UI
                    if tick != 0 && tick % AUTOPAUSE_EVERY == AUTOPAUSE_OFFSET {
                        let changed = cx.update(|cx| {
                            let snap = bridge::sample();
                            let app = cx.global_mut::<engine::AppState>();
                            let pet_fullscreen = !snap.fullscreen.is_empty();
                            let pet_changed = app.pet_fullscreen != pet_fullscreen;
                            let lock_changed = app.pet_locked != snap.locked;
                            app.pet_fullscreen = pet_fullscreen;
                            app.pet_locked = snap.locked;
                            app.sm
                                .apply_autopause(snap.locked, &snap.fullscreen, snap.on_battery)
                                || pet_changed
                                || lock_changed
                        });
                        refresh_ui |= changed;
                    }
                    // 时间脉冲（≈1.05s）：时钟类壁纸的挂钟由引擎驱动；同时推进
                    // 宠物陪伴时间。使用真实单调时间差，不把心跳次数当作秒数。
                    if tick.is_multiple_of(TIME_TICK_EVERY) {
                        cx.update(|cx| {
                            let app = cx.global_mut::<engine::AppState>();
                            app.sm.broadcast_time_tick();
                            let active = app.pet_save.adoption.is_some()
                                && app.pet_save.visible
                                && !app.pet_fullscreen
                                && !app.pet_locked;
                            let now = Instant::now();
                            if app
                                .pet_companion_clock
                                .advance(now, active, &mut app.pet_save.stats)
                            {
                                app.pet_stats_dirty = true;
                            }
                            app.pet_behavior.tick(Duration::from_millis(1050));
                        });
                    }
                    // 软件更新自动检查节拍（§4.7）：开关读引擎真源，发起与否由
                    // update 模块按 24h 抑制窗口判定（忙时忽略）
                    if tick == UPDATE_FIRST_TICK
                        || (tick > UPDATE_FIRST_TICK && tick.is_multiple_of(UPDATE_AUTO_PROBE))
                    {
                        let enabled = cx.update(|cx| {
                            cx.global::<engine::AppState>()
                                .sm
                                .config()
                                .settings
                                .update_check
                        });
                        update::maybe_auto_check(enabled);
                    }
                    // 更新状态代数变化：回灌快照 + 托盘 tooltip 跟随；首次「发现新
                    // 版本」补应用内 toast（系统通知已在检查线程先行发出）
                    let update_gen = update::generation();
                    if update_gen != last_update_gen {
                        last_update_gen = update_gen;
                        refresh_ui = true;
                        let view = update::public_view();
                        let tip = if view.phase == ui::data::UpdatePhase::Available {
                            format!(
                                "Gesso — {}",
                                ui::strings::badge_new_version_value(&view.version)
                            )
                        } else {
                            "Gesso".to_string()
                        };
                        let _ = tray.set_tooltip(Some(tip));
                        // 开发验证钩子（仅 debug 构建）：GESSO_UPDATE_AUTO_DL=1 时
                        // 「发现新版本」即自动确认下载，无头跑通检查→下载→就绪全链路
                        #[cfg(debug_assertions)]
                        if view.phase == ui::data::UpdatePhase::Available
                            && std::env::var("GESSO_UPDATE_AUTO_DL").is_ok()
                        {
                            engine::enqueue(engine::EngineAction::UpdateDownload);
                        }
                        if let Some(ver) = update::consume_available_toast() {
                            use gpui_kit::component::WindowExt as _;
                            cx.update(|cx| {
                                if let Some(h) = MAIN_WINDOW.lock().ok().and_then(|g| *g) {
                                    let _ = h.update(cx, |_, window, cx| {
                                        window.push_notification(
                                            gpui_kit::component::notification::Notification::info(
                                                ui::strings::toast_update_available(&ver),
                                            ),
                                            cx,
                                        );
                                    });
                                }
                            });
                        }
                    }
                    let pet_position = cx.update(|cx| {
                        let (mut save, monitors) = {
                            let app = cx.global::<engine::AppState>();
                            (app.pet_save.clone(), app.sm.monitors())
                        };
                        if cx.global::<engine::AppState>().pet_fullscreen {
                            save.visible = false;
                        }
                        let behavior = cx.global::<engine::AppState>().pet_behavior;
                        cx.global_mut::<pet_window::PetWindowManager>()
                            .sync(&save, &monitors, behavior)
                    });
                    if let Some(position) = pet_position {
                        engine::enqueue(engine::EngineAction::SavePetPosition(position));
                        refresh_ui = true;
                    }
                    if refresh_ui {
                        let (autostart_on, adopted, pet_visible) = cx.update(|cx| {
                            let app = cx.global::<engine::AppState>();
                            (
                                app.sm.config().settings.autostart,
                                app.pet_save.adoption.is_some(),
                                app.pet_save.visible,
                            )
                        });
                        tray.set_menu(Some(Box::new(build_tray_menu_with_pet(
                            autostart_on,
                            adopted,
                            pet_visible,
                        ))));
                        // 会话 → UI 单向回灌（托盘/界面动作 / 显示器热插拔后的跨面同步）
                        cx.update(|cx| {
                            let next = snapshot_ui(
                                &cx.global::<engine::AppState>().sm,
                                &cx.global::<engine::AppState>().thumbs,
                                &cx.global::<engine::AppState>().pet_save,
                            );
                            cx.update_global::<GessoState, _>(|g, _| merge_snapshot(g, next));
                            cx.refresh_windows();
                        });
                    }
                }
            })
            .detach();

            // 快速清扫观察器（30ms 级）：拓扑事件 → 立刻熄灭与现实脱节的
            // 窗口（屏消失的残窗 + 几何过时的旧帧窗——合盖模式切换
            // 1440p→1080p 时旧帧表现为「壁纸先放大再恢复」）。熄窗由桥接帧
            // 兜底，几何收敛仍走主拍的 1.5s 去抖 + 延迟重申。
            cx.spawn(async move |cx| {
                let mut seen = pin::topology_epoch();
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(30))
                        .await;
                    let epoch = pin::topology_epoch();
                    if epoch != seen {
                        seen = epoch;
                        cx.update(|cx| {
                            cx.global_mut::<engine::AppState>().sm.sweep_stale();
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
                // 首轮立即扫：新装/首开不用干等 30s 才开始补帧（库卡片此时正转
                // spinner）；后续 30s 一轮兜底。should_start 在途去重保证与
                // 导入路径的即时派发互不重复。
                let mut first = true;
                loop {
                    if !first {
                        cx.background_executor()
                            .timer(Duration::from_secs(30))
                            .await;
                    }
                    first = false;
                    // 缺帧兜底：统一按策略扫描——Direct（图片）帧发现本就直引源文件，
                    // 只对需"生成"且当前缺帧的 Extract/Capture 起任务。
                    // 真源 = 库目录文件系统，不依赖 UI 快照新鲜度。
                    // 键 = 缩略图家目录（远端 URL 条目 = 库内 <id>/，source_dir 为空）
                    let targets: Vec<(String, WallpaperKind)> = cx.update(|cx| -> _ {
                        let app = cx.global::<engine::AppState>();
                        app.sm
                            .library()
                            .iter()
                            .filter(|e| {
                                gesso_core::content_type(e.kind).thumb
                                    != gesso_core::ThumbStrategy::Direct
                            })
                            .filter(|e| {
                                let home = encoding::thumb_home(e);
                                app.thumbs.should_start(&home)
                                    && thumb::preview_frames(&home, e.kind).len() < 2
                            })
                            .map(|e| (encoding::thumb_home(e), e.kind))
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
            cx.spawn(async move |cx| loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let sample = cx.update(|_| bridge::sample_mouse());
                cx.update(|cx| cx.global_mut::<engine::AppState>().sm.poll_mouse(&sample));
            })
            .detach();

            // 开发验证钩子（仅 debug 构建）：GESSO_UPDATE_MODAL=1 启动即走托盘
            // 同款「检查更新 + 开小窗」动作，无头验证更新链路（弹窗六态/托盘 tooltip/
            // 快照回灌），如 GESSO_UPDATE_MODAL=1 GESSO_LOCK=dev ./target/debug/gesso
            #[cfg(debug_assertions)]
            if std::env::var("GESSO_UPDATE_MODAL").is_ok() {
                engine::enqueue(engine::EngineAction::UpdateCheck { open_modal: true });
            }

            // 管理窗口：三页签 UI（§4.3–4.5；44px 顶栏 + 键盘模型 + 双主题）
            open_main_window(cx);

            cx.activate(true);
        });
}
