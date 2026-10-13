//! 桌面桥接（Desktop Bridge）：指派壁纸时**临时**接管系统桌面为该壁纸的
//! 静态帧（约 5 秒，覆盖建窗/加载黑隙），随后在背后自动还原用户原壁纸并
//! 释放所有权——稳态下系统桌面永远是用户自己的壁纸，退出/恢复零成本
//! （详见末段「静帧是临时覆盖」）。
//!
//! 历史与动机：10-11 设计为常驻接管——显示器重配（唤醒/合盖开盖/插拔/
//! 改分辨率）走两段式提交，抑制窗口（≈0.5–1s）露出的底是系统桌面壁纸，
//! 预设为当前壁纸静帧可让过渡无缝（spike 实测有效；macOS 26 上
//! `osascript System Events` 设壁纸已失效，必须走
//! `NSWorkspace.setDesktopImageURL`）。10-13 实测壁纸代理换图 FIFO 排队 +
//! 每张沉降 5–16s，常驻接管的每次还原都把这份迟钝暴露给用户（静帧滞留/
//! 重放/闪烁），经用户决策改为临时覆盖——重配抑制窗口从此闪现用户静态
//! 壁纸（与 Lively/Plash/WE 等主流应用不接管系统桌面的惯例一致）。
//!
//! 所有权纪律（评审定稿，非补丁的三个支点）：
//! 1. **台账按【条目】键控**（`bridge/<entry_id>.<ext>`，entry id 跨显示器
//!    ID 重编稳定）：显示器键控会在唤醒重编后把我们自己设的桥接图误记为
//!    「用户原壁纸」，永久顶替原值；桥接图路径本身即恢复的连接键。
//! 2. **acquire 三态**：当前桌面已是本条目桥接图（重入，原值保持）／是其它
//!    条目桥接图（同屏换壁纸，继承那条记录的原值）／是用户壁纸（仅首次记录，
//!    首接管定原值——多屏同指派、重复指派都不覆盖）。
//! 3. **release 内容连接**：只恢复「当前桌面 URL 仍是我们的桥接图」的屏——
//!    用户运行期间手改过的壁纸不被覆盖；硬杀残留由下次启动对账兜底
//!    （台账落盘 sidecar `desktop_bridge.json`）。
//!
//! 已知边界（设计取舍，如实入档）：SIGKILL/断电来不及恢复——临时覆盖后
//! 残留窗口仅为指派后约 5s（FRAME_HOLD 内退出由拆窗前的持有档沉降兜底）；
//! 崩溃对账路径上多屏同指派一张壁纸时逐屏原值无法消歧（恢复为同一张）；
//! 硬杀时离线的屏其存储的桌面偏好仍是桥接图，无法远程修复。
//!
//! **台账原值可能死掉**（2026-10-12 实测）：壁纸轮换工具（如 pavo：10 分钟
//! 轮换 + 7 天清缓存）会在接管期间删掉被记录的原值文件——恢复时
//! `setDesktopImageURL` 对死路径必然失败（NSCocoaErrorDomain Code=4）且
//! 「永不覆盖」纪律会让死记录永续。两道自愈：acquire 时死记录视同无记录
//! （当前是活用户壁纸则重记）；restore 时死原值回退静态系统桌面图
//! （`FALLBACK_DESKTOPS` 探测）。
//!
//! **回退目标与台账原值都严禁航拍路径**（2026-10-13 实测「点击恢复黑底」）：
//! macOS 26 的 `DefaultDesktop.heic` 是航拍视频壁纸软链，静态 API 设置渲染
//! 为黑屏，且它文件存在、会被当作「活值」记进台账绕过回退——
//! `is_aerial_desktop` 在记录与恢复两侧都视同死值，强制走静态回退。
//!
//! **恢复先于拆窗**：setDesktopImageURL 的换图有沉降期（代理解码新图，
//! 首次未缓存时秒级），先拆窗用户会直视黑底间隙——恢复在会话窗仍盖着时
//! 执行，set 后 `wait_desktop_settled` 轮询读回等沉降（`SETTLE_QUICK` 800ms
//! / `SETTLE_HOLD` 10s 双档，见常量注释），换图全程藏在壁纸窗背后。
//!
//! **静帧是临时覆盖**（2026-10-13 用户方案）：接管只为覆盖指派后的建窗/
//! 加载黑隙，约 5 秒（`FRAME_HOLD`，见 session.rs）后由 patrol 调
//! `takeovers_older_than` + `release_entry` 在背后还原用户原壁纸并释放
//! 所有权——稳态下桌面永远是用户自己的壁纸，退出/恢复零成本。接管期间
//! 切换条目只记账不重设（防代理 FIFO 队列重放）；撤回遇读回滞后由
//! last_set 绑定兜底（LAG_WINDOW 30s）。

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use gesso_core::{LibraryEntry, WallpaperKind};

// ── 台账与决策（纯数据/纯函数，跨平台可测）─────────────────────────────

/// entry_id → 接管时该屏的原始桌面图（文件路径）。
#[derive(Default, Serialize, Deserialize)]
struct Ledger {
    entries: BTreeMap<String, String>,
}

/// 是否我们的桥接图（按路径前缀判定——桥接图只产出到 `bridge_dir` 之下，
/// 这同时是恢复的凭据：用户改过的屏桌面 URL 已不在此前缀下，自然跳过）。
fn is_bridge_path(bridge_dir: &Path, p: &str) -> bool {
    Path::new(p).starts_with(bridge_dir)
}

fn path_stem(p: &str) -> &str {
    Path::new(p)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
}

/// 死原值回退目标（按序探测取第一个存在的）：必须选**静态图片**——macOS 26
/// 的 `DefaultDesktop.heic` 已软链到航拍视频壁纸（`DefaultAerial.heic`），
/// 经静态图 API 设置渲染为黑屏（2026-10-13 实测「重置之后都是黑色的」；
/// 桥接帧 PNG/JPG 一直渲染正常，唯视频壁纸不行）。Sonoma.heic 是 macOS
/// 14–26 常驻的静态桌面图，纯色 PNG 兜底；全部缺失 → 留台账待对账。
const FALLBACK_DESKTOPS: &[&str] = &[
    "/System/Library/Desktop Pictures/Sonoma.heic",
    "/System/Library/Desktop Pictures/Mac Blue.heic",
    "/System/Library/Desktop Pictures/Solid Colors/Blue.png",
];

/// 系统航拍/默认桌面路径：macOS 26 上经静态图 API 设置渲染为**黑屏**
/// （DefaultDesktop.heic 是 DefaultAerial.heic 视频壁纸的软链）。台账记录
/// 与恢复目标两侧都视同死路径，强制走静态回退——否则「恢复原值 = 恢复
/// 黑屏」（2026-10-12 实测两屏全黑；台账曾被此类值污染）。
fn is_aerial_desktop(p: &str) -> bool {
    p == "/System/Library/CoreServices/DefaultDesktop.heic"
        || p.starts_with("/System/Library/Wallpapers/")
        || p.ends_with(".madesktop")
}

/// 原值/回退目标的可用判定：文件在，且非航拍路径（航拍必黑屏）。
/// 记录与恢复两侧共用这一个不变量。
fn restorable(p: &str) -> bool {
    Path::new(p).exists() && !is_aerial_desktop(p)
}

/// 静态回退目标解析（探测序取第一个存在的）；None = 无可回退。
fn static_fallback(exists: impl Fn(&str) -> bool) -> Option<String> {
    FALLBACK_DESKTOPS
        .iter()
        .copied()
        .find(|p| exists(p))
        .map(String::from)
}

/// 恢复目标决议：台账原值可用（`restorable`）→ 原值；否则 → 回退静态
/// 系统桌面图；全部不可用 → None（无恢复目标，落日志留台账）。
fn restore_target(original: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    if exists(original) && !is_aerial_desktop(original) {
        return Some(original.to_string());
    }
    static_fallback(exists)
}

/// acquire 后台账应持有的原值；None = 读不到现状且无记录，无法保证可恢复，
/// 跳过接管。规则见模块注释「acquire 三态」——核心是**首次记录定原值，
/// 之后永不覆盖**（重入/换壁纸/多屏同指派/用户手改都不改写）。
/// 例外：已记录的原值不可恢复（`original_alive` 判否——文件已消失，或为
/// 静态 API 渲染必黑的航拍路径）→ 视同无记录，有活值则重记——抱着死值
/// 只会把死记录永续。
fn acquire_original(
    current: Option<&str>,
    entry_id: &str,
    bridge_dir: &Path,
    ledger: &Ledger,
    original_alive: impl Fn(&str) -> bool,
) -> Option<String> {
    let known = ledger
        .entries
        .get(entry_id)
        .cloned()
        .filter(|p| original_alive(p));
    match current {
        None => known,
        Some(c) if is_bridge_path(bridge_dir, c) => {
            let stem = path_stem(c);
            let inherited = (stem != entry_id)
                .then(|| ledger.entries.get(stem).cloned())
                .flatten();
            inherited.or(known)
        }
        Some(c) => Some(known.unwrap_or_else(|| c.to_string())),
    }
}

// ── 主体 ────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct DesktopBridge {
    ledger: Ledger,
    /// 桥接图目录（`<config_dir>/bridge/`）。
    dir: PathBuf,
    /// 本进程的最后写入绑定：monitor_id → (entry_id, 时刻)。`setDesktopImageURL`
    /// 的读回有秒级滞后（spike 实测 set 后立即读回还是旧值）——恢复扫描可能
    /// 读到陈旧值而漏判「是我们的」。绑定用于滞后窗口内的兜底恢复。
    last_set: BTreeMap<String, (String, std::time::Instant)>,
}

impl DesktopBridge {
    fn with_dir(dir: PathBuf) -> Self {
        let mut b = Self {
            ledger: Ledger::default(),
            dir,
            last_set: BTreeMap::new(),
        };
        b.load();
        b
    }

    fn ledger_path(&self) -> PathBuf {
        self.dir
            .parent()
            .unwrap_or(Path::new("/"))
            .join("desktop_bridge.json")
    }

    fn load(&mut self) {
        if let Ok(s) = std::fs::read_to_string(self.ledger_path()) {
            if let Ok(l) = serde_json::from_str::<Ledger>(&s) {
                self.ledger = l;
            }
        }
    }

    fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(&self.ledger) {
            let _ = std::fs::create_dir_all(&self.dir);
            let tmp = self.ledger_path().with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, self.ledger_path());
            }
        }
    }

    /// 桥接图路径：`<dir>/<entry_id>.<ext>`。图片类沿用源扩展名（系统按
    /// 内容/扩展名双保险识别），其余统一 png。
    fn static_path(&self, entry: &LibraryEntry) -> PathBuf {
        let ext = match entry.kind {
            WallpaperKind::Image => crate::encoding::main_asset_name(&entry.source_dir, entry.kind)
                .and_then(|n| {
                    Path::new(&n)
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_ascii_lowercase())
                })
                .unwrap_or_else(|| "png".into()),
            _ => "png".into(),
        };
        self.dir.join(format!("{}.{}", entry.id, ext))
    }

    /// 静帧供给（按类型分派，对齐 ThumbStrategy 的表驱动思路）：
    /// 视频 = AVAssetImageGenerator 首帧全分辨率；图片 = 源文件拷贝（刻意的
    /// 拷贝——绝不把用户文件路径写进系统设置）；shader/html = 现成缩略图档
    /// （过渡零点几秒内肉眼难辨，全分辨率快照留后续）。
    fn produce_frame(&self, entry: &LibraryEntry, out: &Path) -> bool {
        // 桥接目录可能尚不存在（首次 acquire 早于任何 save() 的 create_dir_all）
        let _ = std::fs::create_dir_all(&self.dir);
        match entry.kind {
            WallpaperKind::Image => {
                let src = crate::encoding::entry_main_source(entry);
                let src = Path::new(&entry.source_dir).join(&src);
                src.exists() && std::fs::copy(&src, out).is_ok()
            }
            WallpaperKind::Video => {
                let src = crate::encoding::entry_main_source(entry);
                let src = Path::new(&entry.source_dir).join(&src);
                src.exists() && {
                    #[cfg(target_os = "macos")]
                    {
                        extract_video_first_frame(&src, out)
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        false
                    }
                }
            }
            _ => {
                let home = crate::encoding::thumb_home(entry);
                let frames = crate::thumb::preview_frames(&home, entry.kind);
                frames
                    .first()
                    .is_some_and(|f| std::fs::copy(f, out).is_ok())
            }
        }
    }

    /// 接管：记原值（三态）→ 已持有所有权则只记账返回 → 否则产出静帧 →
    /// 设系统桌面 → 落台账。必须主线程（AppKit）。
    #[cfg(target_os = "macos")]
    fn acquire_inner(&mut self, entry: &LibraryEntry, monitor_id: &str) {
        use objc2_foundation::NSURL;

        // NSWorkspace 桌面图 API 在 objc2 0.2 绑定里声明为安全（文档语义
        // 无 UB 面）；AVFoundation 的资产/帧提取仍按 unsafe 纪律走
        let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let Some(screen) = crate::pin::macos::screens()
            .into_iter()
            .find(|(m, _)| m.id == monitor_id)
            .map(|(_, s)| s)
        else {
            println!("[bridge] {monitor_id} 未找到对应系统屏，跳过接管");
            return;
        };
        let current = ws
            .desktopImageURLForScreen(&screen)
            .and_then(|u| u.path())
            .map(|p| p.to_string());
        // 原值判定先于任何写入；无法保证可恢复（读不到且无记录）则不接管。
        // 存活判定 = 文件在且非航拍——台账里的航拍旧值视同死值，有活值则重记
        let Some(original) = acquire_original(
            current.as_deref(),
            &entry.id,
            &self.dir,
            &self.ledger,
            restorable,
        ) else {
            println!(
                "[bridge] {} 原壁纸不可读且无记录，跳过接管（安全优先）",
                entry.id
            );
            return;
        };
        // 当前桌面本身是航拍（用户用系统默认壁纸）时记录的是航拍路径——
        // 同样不可渲染，直接记静态回退替代
        let original = if is_aerial_desktop(&original) {
            println!(
                "[bridge] {} 原值为系统航拍桌面（{original}），记静态回退替代",
                entry.id
            );
            static_fallback(|p| Path::new(p).exists()).unwrap_or(original)
        } else {
            original
        };
        // 已持有该屏桌面所有权（本进程写过且未释放）→ 不再重设静帧、不产出
        // 桥接帧，只记账：壁纸代理换图是 FIFO 排队 + 沉降 5–16s，逐次重设会
        // 让「恢复系统壁纸」时按历史逐张重放静帧（2026-10-13 反馈）；顺带
        // 消除「循环指派顶掉用户手改桌面图」的隐性覆盖
        if self.last_set.contains_key(monitor_id) {
            self.ledger.entries.insert(entry.id.clone(), original);
            self.save();
            println!(
                "[bridge] {monitor_id} ← {}（已持有桌面所有权，静帧不重设）",
                entry.id
            );
            return;
        }
        let path = self.static_path(entry);
        if !path.exists() && !self.produce_frame(entry, &path) {
            println!("[bridge] {} 桥接帧产出失败，跳过接管", entry.id);
            return;
        }
        let url = NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(
            &path.display().to_string(),
        ));
        let empty = objc2_foundation::NSDictionary::new();
        // SAFETY: 主线程；screen 来自当前枚举的存活 NSScreen，url 指向有效文件
        if let Err(e) =
            unsafe { ws.setDesktopImageURL_forScreen_options_error(&url, &screen, &empty) }
        {
            println!("[bridge] 设系统桌面失败：{e:?}");
            return;
        }
        self.ledger.entries.insert(entry.id.clone(), original);
        self.last_set.insert(
            monitor_id.to_string(),
            (entry.id.clone(), std::time::Instant::now()),
        );
        self.save();
        println!(
            "[bridge] {} ← {}（接管系统桌面，桥接帧 {}）",
            monitor_id,
            entry.id,
            path.display()
        );
    }

    /// 恢复：扫描当前所有屏，凡桌面图是（限定条目的）桥接图 → 恢复该条目
    /// 记录的原值；再对「读回滞后」的本进程写入做绑定兜底。返回成功恢复的
    /// 条目集合（调用方据此决定台账清理——没恢复成的不清，留对账）。
    /// 必须主线程。
    #[cfg(target_os = "macos")]
    fn restore(&mut self, only: Option<&str>, settle: std::time::Duration) -> BTreeSet<String> {
        /// set 后读回的滞后窗口：窗口内读不到「是我们的」按我们刚写的算
        ///（spike 实测 set 后立即读回还是旧值）。2026-10-13 实测 macOS 26
        /// 换图读回沉降 **5–16s**（连续采样），窗口必须盖过它——此前 10s
        /// 会在「随机换一张后马上恢复」时漏判：第①步读到陈旧旧值认不出
        /// 桥接帧、第②步又超窗，桌面停在静帧上。
        const LAG_WINDOW: std::time::Duration = std::time::Duration::from_secs(30);

        let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let screens = crate::pin::macos::screens();
        let mut restored: BTreeSet<String> = BTreeSet::new();
        // 第①步实际发起过恢复（成功或失败，失败必落日志）的条目——第②步
        // 对它们保持沉默，不再打出「疑似用户已改」的误导消息
        let mut attempted: BTreeSet<String> = BTreeSet::new();
        // ① 内容扫描：桌面图仍是我们的桥接图 → 恢复（用户手改过的屏 URL
        //    已不是我们的 → 自然跳过，绝不覆盖，评审 B2）
        for (info, screen) in &screens {
            let Some(url) = ws
                .desktopImageURLForScreen(screen)
                .and_then(|u| u.path())
                .map(|p| p.to_string())
            else {
                continue;
            };
            if !is_bridge_path(&self.dir, &url) {
                continue;
            }
            let stem = path_stem(&url).to_string();
            if only.is_some_and(|o| stem != o) || restored.contains(&stem) {
                continue;
            }
            let Some(original) = self.ledger.entries.get(&stem).cloned() else {
                println!("[bridge] 桥接图无台账记录（硬杀丢台账），无法恢复：{url}");
                continue;
            };
            attempted.insert(stem.clone());
            let Some(target) = restore_target(&original, |p| Path::new(p).exists()) else {
                println!(
                    "[bridge] {stem} 原壁纸已不存在（{original}）且无静态桌面图可回退，留台账待对账"
                );
                continue;
            };
            if target != original {
                println!("[bridge] {stem} 原壁纸不可用（{original}），回退静态系统桌面图（{target}）");
            }
            if apply_desktop(&ws, screen, &target, settle) {
                println!("[bridge] {} 恢复系统壁纸 → {target}", info.id);
                restored.insert(stem);
            }
        }
        // ② 绑定兜底：本进程刚写入（LAG_WINDOW 内）但读回滞后的屏——
        //    按显示器 ID 直接恢复；窗口外读不到我们的图视为用户已改，不动
        for (monitor_id, (entry_id, at)) in self.last_set.clone() {
            if only.is_some_and(|o| entry_id != o)
                || restored.contains(&entry_id)
                || attempted.contains(&entry_id)
            {
                continue;
            }
            let recent = at.elapsed() < LAG_WINDOW;
            let Some((info, screen)) = screens.iter().find(|(m, _)| m.id == monitor_id) else {
                continue; // 屏已离线：记录保留，交下次启动对账
            };
            let Some(original) = self.ledger.entries.get(&entry_id).cloned() else {
                continue;
            };
            if !recent {
                println!("[bridge] {monitor_id} 桌面图非桥接帧（读回已沉降，疑似用户已改），跳过恢复");
                continue;
            }
            let Some(target) = restore_target(&original, |p| Path::new(p).exists()) else {
                println!(
                    "[bridge] {entry_id} 原壁纸不可用（{original}）且无静态桌面图可回退，留台账待对账"
                );
                continue;
            };
            if apply_desktop(&ws, screen, &target, settle) {
                println!(
                    "[bridge] {monitor_id} 读回滞后兜底恢复（{}）→ {target}",
                    info.id
                );
                restored.insert(entry_id);
            }
        }
        restored
    }
}

/// 把一屏的桌面图设为 target 并等换图沉降；成功返回 true，失败落日志
/// （此前第②步失败静默，排查时无迹可寻）。恢复路径的唯一写入点。
#[cfg(target_os = "macos")]
fn apply_desktop(
    ws: &objc2_app_kit::NSWorkspace,
    screen: &objc2_app_kit::NSScreen,
    target: &str,
    settle: std::time::Duration,
) -> bool {
    use objc2_foundation::{NSDictionary, NSURL, NSString};

    let url = NSURL::fileURLWithPath(&NSString::from_str(target));
    let empty = NSDictionary::new();
    // SAFETY: 主线程，存活 screen，target 指向已确认存在的文件
    match unsafe { ws.setDesktopImageURL_forScreen_options_error(&url, screen, &empty) } {
        Ok(()) => {
            wait_desktop_settled(ws, screen, target, settle);
            true
        }
        Err(e) => {
            println!("[bridge] 恢复失败（{target}）：{e:?}");
            false
        }
    }
}

/// set 后等壁纸代理完成换图：读回反映新 URL 即视为配置沉降完成（spike
/// 实测读回滞后秒级，正好当完成信号用；模块纪律「set 后不要立即读回验证」
/// 指的是别用它判对错，这里只用作让步等待）。封顶 `settle`——读不回也
/// 继续，代理终会渲染；恢复先于拆窗执行时这段等待藏在壁纸窗背后。
/// 读回确认后再留 `RENDER_GRACE`：配置沉降先于渲染完成，这段宽限让代理
/// 的渲染与过渡动画大概率在壁纸窗背后播完，拆窗后用户不再看到二次闪烁
/// （2026-10-13 反馈「退出后壁纸被重新设置了一次」——即渲染阶段的过渡
/// 动画，系统行为，宽限只能覆盖其典型时序）。
#[cfg(target_os = "macos")]
fn wait_desktop_settled(
    ws: &objc2_app_kit::NSWorkspace,
    screen: &objc2_app_kit::NSScreen,
    target: &str,
    settle: std::time::Duration,
) {
    const POLL: std::time::Duration = std::time::Duration::from_millis(60);
    const RENDER_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);
    let deadline = std::time::Instant::now() + settle;
    while std::time::Instant::now() < deadline {
        let settled = ws
            .desktopImageURLForScreen(screen)
            .and_then(|u| u.path())
            .is_some_and(|p| p.to_string() == target);
        if settled {
            std::thread::sleep(RENDER_GRACE);
            return;
        }
        std::thread::sleep(POLL);
    }
}

/// 视频首帧全分辨率提取（复用 thumb.rs 的 objc2 所有权纪律与 ImageIO 写盘）。
#[cfg(target_os = "macos")]
fn extract_video_first_frame(src: &Path, out: &Path) -> bool {
    use objc2_av_foundation::{AVAssetImageGenerator, AVURLAsset};
    use objc2_core_media::CMTime;
    use objc2_foundation::{NSString, NSURL};

    objc2::rc::autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&src.display().to_string()));
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
        let gen = unsafe { AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset) };
        unsafe {
            gen.setRequestedTimeToleranceBefore(CMTime::new(0, 600));
            gen.setRequestedTimeToleranceAfter(CMTime::new(0, 600));
            gen.setAppliesPreferredTrackTransform(true);
        }
        // 同步版被标 deprecated（Apple 推异步回调）——后台有限帧场景同步最简，
        // 同 thumb.rs 的取舍得罪
        #[allow(deprecated)]
        let frame = unsafe {
            gen.copyCGImageAtTime_actualTime_error(
                CMTime::with_seconds(0.0, 600),
                std::ptr::null_mut(),
            )
        };
        match frame {
            Ok(image) => crate::thumb::write_png(&image, out),
            Err(e) => {
                println!("[bridge] 视频首帧提取失败 {src:?}：{e:?}");
                false
            }
        }
    })
}

// ── 全局入口（app 侧统一走这里；非 macOS 平台 no-op）──────────────────

static BRIDGE: OnceLock<Mutex<DesktopBridge>> = OnceLock::new();

fn global() -> &'static Mutex<DesktopBridge> {
    BRIDGE.get_or_init(|| {
        Mutex::new(DesktopBridge::with_dir(
            crate::protocol::config_dir().join("bridge"),
        ))
    })
}

/// 接管一个屏（assign 成功后调用）。非 macOS 平台 no-op。
pub fn acquire(entry: &LibraryEntry, monitor_id: &str) {
    #[cfg(target_os = "macos")]
    global()
        .lock()
        .expect("桌面桥接")
        .acquire_inner(entry, monitor_id);
    #[cfg(not(target_os = "macos"))]
    let _ = (entry, monitor_id);
}

/// 取消指派/移除条目：恢复显示该条目桥接图的屏并清其记录（未恢复成则保留，
/// 交启动对账）、删桥接图。
pub fn release_entry(entry_id: &str) {
    #[cfg(target_os = "macos")]
    {
        let mut b = global().lock().expect("桌面桥接");
        // 持有档：调用方（remove_entry）在其壁纸窗仍上播时调用，换图沉降
        // 藏在背后完成；无命中的常见路径零等待
        let restored = b.restore(Some(entry_id), SETTLE_HOLD);
        if restored.contains(entry_id) {
            b.ledger.entries.remove(entry_id);
            // 所有权一并释放：绑定残留会让下一次指派的接管被跳过，
            // 静帧覆盖就此失效（临时覆盖方案的生命周期闭环）
            b.last_set.retain(|_, (e, _)| e != entry_id);
            b.save();
        }
    }
    let _ = entry_id;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = entry_id;
    }
}

/// 沉降等待上限的两档。持有档（SETTLE_HOLD）= 拆窗前用户会直视桌面的路径
/// （「恢复系统壁纸」、按条目删除）——换图藏在仍上播的壁纸窗背后完成再拆，
/// 提前拆窗会让旧静帧在眼前滞留 5–16s（实测读回/渲染滞后）；快速档
/// （SETTLE_QUICK）= 秒退优先的退出路径（用户明确要求即点即退，未沉降的
/// 换图由代理在退出后自行完成），以及窗口不在场/进程即死（panic 钩子、
/// 启动对账、关开关）。
const SETTLE_QUICK: std::time::Duration = std::time::Duration::from_millis(800);
const SETTLE_HOLD: std::time::Duration = std::time::Duration::from_secs(10);

/// 全量恢复（退出 / 关闭开关 / 启动对账）：快速档——进程随即终止或壁纸窗
/// 常驻不拆，换图沉降在背后完成即可。返回恢复的屏数；没恢复成的记录保留
/// 在台账里，交下次启动对账（绝不「清了记录却没恢复」——那是原壁纸丢失
/// 的通路）。
pub fn restore_all() -> usize {
    restore_all_inner(SETTLE_QUICK)
}

/// 待撤回的接管条目：本进程写入且已超过 `min_hold` 的（静帧只需覆盖指派
/// 后的建窗/加载间隙，就绪后即应由调用方在背后撤回——见模块注释）。
/// 返回去重后的条目 id。
pub fn takeovers_older_than(min_hold: std::time::Duration) -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        let b = global().lock().expect("桌面桥接");
        let mut due: BTreeSet<String> = BTreeSet::new();
        for (entry_id, at) in b.last_set.values() {
            if at.elapsed() >= min_hold {
                due.insert(entry_id.clone());
            }
        }
        due.into_iter().collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = min_hold;
        Vec::new()
    }
}

/// 全量恢复（释放路径专用）：换图沉降完成（读回确认）后才返回，调用方
/// 在此之后才拆壁纸窗——用户看到的是「动态壁纸继续播 → 直接变系统壁纸」，
/// 中间不出现旧静帧滞留。
pub fn restore_all_holding_windows() -> usize {
    restore_all_inner(SETTLE_HOLD)
}

fn restore_all_inner(settle: std::time::Duration) -> usize {
    #[cfg(target_os = "macos")]
    {
        let mut b = global().lock().expect("桌面桥接");
        let restored = b.restore(None, settle);
        // 只清「确实恢复了」的记录；没恢复成的保留（启动对账重试）——
        // 反过来就是「清了记录却没恢复」的原壁纸丢失通路（E2E 实测教训）
        b.ledger.entries.retain(|k, _| !restored.contains(k));
        b.last_set.retain(|_, (e, _)| !restored.contains(e));
        b.save();
        restored.len()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = settle;
        0
    }
}

/// 启动对账：硬杀残留的接管记录按当前活跃指派清理；开关关闭则全量恢复。
/// `active` = 本次启动有会话的条目集合。
pub fn reconcile(active: &BTreeSet<String>, enabled: bool) {
    if !enabled {
        restore_all();
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let mut b = global().lock().expect("桌面桥接");
        let inactive: Vec<String> = b
            .ledger
            .entries
            .keys()
            .filter(|e| !active.contains(*e))
            .cloned()
            .collect();
        for e in inactive {
            let restored = b.restore(Some(&e), SETTLE_QUICK);
            if restored.contains(&e) {
                b.ledger.entries.remove(&e);
            }
        }
        b.save();
    }
    let _ = active;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (active, enabled);
    }
}

/// 移除条目时顺手清桥接图（台账记录由 release_entry 清理）。
pub fn discard_file(entry_id: &str) {
    #[cfg(target_os = "macos")]
    {
        let b = global().lock().expect("桌面桥接");
        if let Ok(entries) = std::fs::read_dir(&b.dir) {
            for e in entries.flatten() {
                if e.path().file_stem().is_some_and(|s| s == entry_id) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    let _ = entry_id;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger(pairs: &[(&str, &str)]) -> Ledger {
        Ledger {
            entries: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn acquire_original_three_states() {
        let dir = Path::new("/cfg/bridge");
        let user_wp = "/Users/u/Pictures/wall.jpg";
        let e1_img = "/cfg/bridge/e1.png";
        let e2_img = "/cfg/bridge/e2.png";
        let alive = |_: &str| true;

        // 首接管用户壁纸 → 记录
        assert_eq!(
            acquire_original(Some(user_wp), "e1", dir, &Ledger::default(), alive),
            Some(user_wp.to_string())
        );
        // 重入（已是本条目桥接图）→ 保持已有记录
        let l = ledger(&[("e1", user_wp)]);
        assert_eq!(
            acquire_original(Some(e1_img), "e1", dir, &l, alive),
            Some(user_wp.to_string())
        );
        // 同屏换壁纸（屏上还是旧条目 e1 的桥接图，接管新条目 e2）→ 继承
        // e1 记录的原值
        assert_eq!(
            acquire_original(Some(e1_img), "e2", dir, &l, alive),
            Some(user_wp.to_string())
        );
        // 屏上是本条目桥接图但台账无记录（硬杀丢台账）→ 无法保证可恢复，
        // 跳过接管（安全优先）
        assert_eq!(acquire_original(Some(e2_img), "e2", dir, &l, alive), None);
        // 已有记录时永不覆盖（多屏同指派第二次 acquire / 用户手改后重放）
        let other_user_wp = "/Users/u/Pictures/other.jpg";
        assert_eq!(
            acquire_original(Some(other_user_wp), "e1", dir, &l, alive),
            Some(user_wp.to_string())
        );
        // 读不到当前桌面且有记录 → 沿用；无记录 → 跳过接管（安全优先）
        assert_eq!(
            acquire_original(None, "e1", dir, &l, alive),
            Some(user_wp.to_string())
        );
        assert_eq!(acquire_original(None, "e2", dir, &l, alive), None);
    }

    #[test]
    fn acquire_original_dead_record_refreshes() {
        let dir = Path::new("/cfg/bridge");
        let dead = "/Users/u/Pictures/gone.jpg";
        let live = "/Users/u/Pictures/fresh.jpg";
        let l = ledger(&[("e1", dead)]);
        let alive = |p: &str| p == live;

        // 台账原值文件已消失（轮换工具清缓存）+ 当前是活用户壁纸 → 重记当前
        assert_eq!(
            acquire_original(Some(live), "e1", dir, &l, alive),
            Some(live.to_string())
        );
        // 记录存活时永不覆盖语义不变（原值文件还在）
        let all_alive = |_: &str| true;
        assert_eq!(
            acquire_original(Some(live), "e1", dir, &l, all_alive),
            Some(dead.to_string())
        );
        // 当前读不到 + 记录已死 → 无可恢复保证，跳过接管（安全优先）
        assert_eq!(acquire_original(None, "e1", dir, &l, alive), None);
        // 屏上是我们的桥接图（重入）+ 记录已死且无继承 → 跳过接管
        let bridge_img = "/cfg/bridge/e1.png";
        assert_eq!(acquire_original(Some(bridge_img), "e1", dir, &l, alive), None);
        // 调用点真实谓词（restorable = 文件在且非航拍）：台账航拍旧值视同
        // 死值 → 重记当前活值（死值抱住会让恢复原值 = 恢复黑屏）
        let aerial = "/System/Library/CoreServices/DefaultDesktop.heic";
        let poisoned = ledger(&[("e1", aerial)]);
        let restorable = |p: &str| p != aerial;
        assert_eq!(
            acquire_original(Some(live), "e1", dir, &poisoned, restorable),
            Some(live.to_string())
        );
    }

    #[test]
    fn restore_target_falls_back_to_static_system_wallpaper() {
        let sonoma = "/System/Library/Desktop Pictures/Sonoma.heic";
        let aerial = "/System/Library/CoreServices/DefaultDesktop.heic";
        // 原值在 → 原值
        assert_eq!(
            restore_target("/u/wall.jpg", |p| p != sonoma),
            Some("/u/wall.jpg".to_string())
        );
        // 原值死 + 静态默认在 → 回退（按探测序取第一个存在的）
        assert_eq!(
            restore_target("/u/gone.jpg", |p| p == sonoma),
            Some(sonoma.to_string())
        );
        // 原值死 + 全部缺失 → 无恢复目标
        assert_eq!(restore_target("/u/gone.jpg", |_| false), None);
        // 探测序：前面的不在则取下一个
        let blue = "/System/Library/Desktop Pictures/Solid Colors/Blue.png";
        assert_eq!(
            restore_target("/u/gone.jpg", |p| p == blue),
            Some(blue.to_string())
        );
        // 航拍路径即使文件存在也视同死值（静态 API 设置必黑屏）→ 回退静态
        assert_eq!(
            restore_target(aerial, |p| p == sonoma || p == aerial),
            Some(sonoma.to_string())
        );
        // 航拍路径 + 无静态可回退 → 无恢复目标（宁可不恢复也不设黑屏值）
        assert_eq!(restore_target(aerial, |p| p == aerial), None);
    }

    #[test]
    fn aerial_desktop_detection() {
        assert!(is_aerial_desktop(
            "/System/Library/CoreServices/DefaultDesktop.heic"
        ));
        assert!(is_aerial_desktop(
            "/System/Library/Wallpapers/.default/DefaultAerial.heic"
        ));
        assert!(is_aerial_desktop(
            "/System/Library/Desktop Pictures/Big Sur.madesktop"
        ));
        // 桥接图与用户壁纸不受牵连
        assert!(!is_aerial_desktop(
            "/cfg/bridge/dbabf70afb07cdab.png"
        ));
        assert!(!is_aerial_desktop(
            "/Users/u/Pictures/wall.jpg"
        ));
        assert!(!is_aerial_desktop(
            "/System/Library/Desktop Pictures/Sonoma.heic"
        ));
    }

    #[test]
    fn bridge_path_detection_by_prefix() {
        let dir = Path::new("/cfg/bridge");
        assert!(is_bridge_path(dir, "/cfg/bridge/e1.png"));
        assert!(!is_bridge_path(dir, "/Users/u/wall.jpg"));
        // 前缀同名的兄弟目录不算（组件级比较，非字符串前缀）
        assert!(!is_bridge_path(dir, "/cfg/bridge-extra/e1.png"));
    }
}
