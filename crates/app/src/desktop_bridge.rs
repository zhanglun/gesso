//! 桌面桥接（Desktop Bridge）：Gesso 在屏期间，系统桌面壁纸 = 当前壁纸的
//! 静态帧；释放所有权时恢复用户原壁纸。
//!
//! 动机：显示器重配（唤醒/合盖开盖/插拔/改分辨率）走两段式提交，抑制窗口
//! （≈0.5–1s）露出的底是系统桌面壁纸——把它提前设为当前壁纸的静帧，过渡即
//! 无缝（2026-10-11 spike 实测：过渡期显示的正是当前按屏设置的系统桌面图；
//! 注意 macOS 26 上 `osascript System Events` 设壁纸已失效，必须走
//! `NSWorkspace.setDesktopImageURL`）。
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
//! 已知边界（设计取舍，如实入档）：SIGKILL/断电来不及恢复，残留到下次启动；
//! 崩溃对账路径上多屏同指派一张壁纸时逐屏原值无法消歧（恢复为同一张）；
//! 硬杀时离线的屏其存储的桌面偏好仍是桥接图，无法远程修复。
//!
//! **台账原值可能死掉**（2026-10-12 实测）：壁纸轮换工具（如 pavo：10 分钟
//! 轮换 + 7 天清缓存）会在接管期间删掉被记录的原值文件——恢复时
//! `setDesktopImageURL` 对死路径必然失败（NSCocoaErrorDomain Code=4）且
//! 「永不覆盖」纪律会让死记录永续。两道自愈：acquire 时死记录视同无记录
//! （当前是活用户壁纸则重记）；restore 时死原值回退系统默认桌面
//! `DefaultDesktop.heic`（跨版本稳定的系统间接引用）。

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

/// 系统默认桌面图：跨版本稳定的系统间接引用（Sonoma+ 壁纸代理也认它），
/// 作死原值的回退恢复目标。
#[cfg(target_os = "macos")]
const SYSTEM_DEFAULT_DESKTOP: &str = "/System/Library/CoreServices/DefaultDesktop.heic";

/// 恢复目标决议：台账原值文件仍在 → 原值；已消失（轮换工具清理/用户删除）
/// → 回退系统默认桌面；默认也不在 → None（无恢复目标，落日志留台账）。
fn restore_target(original: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    if exists(original) {
        return Some(original.to_string());
    }
    #[cfg(target_os = "macos")]
    return exists(SYSTEM_DEFAULT_DESKTOP).then(|| SYSTEM_DEFAULT_DESKTOP.to_string());
    #[cfg(not(target_os = "macos"))]
    None
}

/// acquire 后台账应持有的原值；None = 读不到现状且无记录，无法保证可恢复，
/// 跳过接管。规则见模块注释「acquire 三态」——核心是**首次记录定原值，
/// 之后永不覆盖**（重入/换壁纸/多屏同指派/用户手改都不改写）。
/// 例外：已记录的原值文件已不存在（`original_alive` 判否）→ 视同无记录，
/// 有活值则重记——死路径恢复必然失败，抱着只会把死记录永续。
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

    /// 接管：产出静帧 → 记原值（三态）→ 设系统桌面 → 落台账。
    /// 必须主线程（AppKit）。
    #[cfg(target_os = "macos")]
    fn acquire_inner(&mut self, entry: &LibraryEntry, monitor_id: &str) {
        use objc2_foundation::NSURL;

        let path = self.static_path(entry);
        if !path.exists() && !self.produce_frame(entry, &path) {
            println!("[bridge] {} 桥接帧产出失败，跳过接管", entry.id);
            return;
        }
        let Some(screen) = crate::pin::macos::screens()
            .into_iter()
            .find(|(m, _)| m.id == monitor_id)
            .map(|(_, s)| s)
        else {
            println!("[bridge] {monitor_id} 未找到对应系统屏，跳过接管");
            return;
        };
        // NSWorkspace 桌面图 API 在 objc2 0.2 绑定里声明为安全（文档语义
        // 无 UB 面）；AVFoundation 的资产/帧提取仍按 unsafe 纪律走
        let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let current = ws
            .desktopImageURLForScreen(&screen)
            .and_then(|u| u.path())
            .map(|p| p.to_string());
        // 原值判定先于任何写入；无法保证可恢复（读不到且无记录）则不接管
        let Some(original) = acquire_original(
            current.as_deref(),
            &entry.id,
            &self.dir,
            &self.ledger,
            |p| Path::new(p).exists(),
        ) else {
            println!(
                "[bridge] {} 原壁纸不可读且无记录，跳过接管（安全优先）",
                entry.id
            );
            return;
        };
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
    fn restore(&mut self, only: Option<&str>) -> BTreeSet<String> {
        use objc2_foundation::NSURL;

        /// set 后读回的滞后窗口：窗口内读不到「是我们的」按我们刚写的算
        ///（spike 实测 set 后立即读回还是旧值，秒级沉降）。
        const LAG_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

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
                    "[bridge] {stem} 原壁纸已不存在（{original}）且无系统默认桌面可回退，留台账待对账"
                );
                continue;
            };
            if target != original {
                println!("[bridge] {stem} 原壁纸已消失（{original}），回退系统默认桌面");
            }
            let orig_url = NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(&target));
            let empty = objc2_foundation::NSDictionary::new();
            // SAFETY: 主线程，存活 screen，target 指向已确认存在的文件
            match unsafe { ws.setDesktopImageURL_forScreen_options_error(&orig_url, screen, &empty) }
            {
                Ok(()) => {
                    println!("[bridge] {} 恢复系统壁纸 → {target}", info.id);
                    restored.insert(stem);
                }
                Err(e) => println!("[bridge] {} 恢复失败（{target}）：{e:?}", info.id),
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
                println!("[bridge] {monitor_id} 桌面图非桥接帧（疑似用户已改），跳过恢复");
                continue;
            }
            let Some(target) = restore_target(&original, |p| Path::new(p).exists()) else {
                println!(
                    "[bridge] {entry_id} 原壁纸已不存在（{original}）且无系统默认桌面可回退，留台账待对账"
                );
                continue;
            };
            let orig_url = NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(&target));
            let empty = objc2_foundation::NSDictionary::new();
            // SAFETY: 同上
            let ok =
                unsafe { ws.setDesktopImageURL_forScreen_options_error(&orig_url, screen, &empty) }
                    .is_ok();
            if ok {
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
        let restored = b.restore(Some(entry_id));
        if restored.contains(entry_id) {
            b.ledger.entries.remove(entry_id);
            b.save();
        }
    }
    let _ = entry_id;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = entry_id;
    }
}

/// 全量恢复（退出 / 关闭开关 / 托盘逃生门）。返回恢复的屏数；没恢复成的
/// 记录保留在台账里，交下次启动对账（绝不「清了记录却没恢复」——那是
/// 原壁纸丢失的通路）。
pub fn restore_all() -> usize {
    #[cfg(target_os = "macos")]
    {
        let mut b = global().lock().expect("桌面桥接");
        let restored = b.restore(None);
        // 只清「确实恢复了」的记录；没恢复成的保留（启动对账重试）——
        // 反过来就是「清了记录却没恢复」的原壁纸丢失通路（E2E 实测教训）
        b.ledger.entries.retain(|k, _| !restored.contains(k));
        b.last_set.retain(|_, (e, _)| !restored.contains(e));
        b.save();
        restored.len()
    }
    #[cfg(not(target_os = "macos"))]
    0
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
            let restored = b.restore(Some(&e));
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
    }

    #[test]
    fn restore_target_falls_back_to_system_default() {
        let default = "/System/Library/CoreServices/DefaultDesktop.heic";
        // 原值在 → 原值
        assert_eq!(
            restore_target("/u/wall.jpg", |p| p != default),
            Some("/u/wall.jpg".to_string())
        );
        // 原值死 + 默认在 → 回退默认
        assert_eq!(
            restore_target("/u/gone.jpg", |p| p == default),
            Some(default.to_string())
        );
        // 原值死 + 默认也不在 → 无恢复目标
        assert_eq!(restore_target("/u/gone.jpg", |_| false), None);
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
