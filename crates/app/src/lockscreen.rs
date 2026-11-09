//! 锁屏快照同步（设计规格 §4.8/§4.8.1 / 技术方案 §9/§10）：opt-in，默认关。
//!
//! 打开后，锁屏显示当前壁纸的「封面帧」——入库时按主屏分辨率生成的固定帧
//! （`thumb_home/poster.jpg|.png`；jpg/png 静图直接引用主文件）。即指即得，
//! 零抓帧。
//!
//! - **Windows**：官方 `Windows.System.UserProfile.LockScreen` API 写独立锁屏
//!   表面。兜底：旧条目/封面缺失时，活窗 PrintWindow 抓一帧（存为该条目的
//!   应用缓存 `config_dir/lockscreen/<entry_id>.png`，下次直接用）。真活儿在
//!   专职后台线程（WinRT 阻塞等待），主循环 tick 只做纳秒级原子检查 + 回灌。
//! - **macOS（§4.8.1 路线 B）**：锁屏跟随桌面壁纸，把封面帧写上**被钉窗盖住的
//!   桌面壁纸层**（官方 `NSWorkspace` API，一次性低频写；实测 1–3s 生效）。
//!   NSWorkspace 是主线程 API 且单次调用毫秒级 IPC——全部内联在主循环 tick
//!   执行，无后台线程。无活窗兜底抓帧（封面缺失等存量补齐，重试窗口 30s）。
//!
//! 关闭开关 / 托盘「恢复系统壁纸」/ 退出时，恢复打开前的状态（Windows 记
//! 锁屏模式+图片路径；macOS 记原壁纸 URL+呈现选项，分列 `config.json` 两簿记）。
//!
//! 纪律（与 2026-10-13 桌面桥接被砍的结构对照）：
//! - 一次性后台写，**不进任何交互路径**（防抖 1s + 写入前复查开关）；
//! - Windows 不写系统桌面——锁屏图是独立 OS 表面；macOS 写的桌面层被钉窗
//!   盖住、四路还原（ROADMAP「明确不做」豁免注记）；
//! - 能力启动探测一次，失败即静默停用；运行期单次写入失败只落日志，下次
//!   指派照常（无状态机）。

// 非 Windows/非 macOS 平台的公共签名保留为 no-op（死代码豁免同 74996ee 先例）。
#![cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 指派后的防抖延迟（§4.8）：批量收敛保护。封面帧就绪时应用是毫秒级
/// WinRT 调用，1s 足够。
const DEBOUNCE: Duration = Duration::from_secs(1);
/// 兜底抓帧路径下，主屏窗口未就绪（收敛中 / 隐藏态）时的重试间隔与上限；
/// 超限放弃本轮，下次指派重来。
const RETRY_DELAY: Duration = Duration::from_millis(1500);
const MAX_RETRY: u32 = 20;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── macOS 实现（§4.8.1 路线 B）：封面帧写桌面壁纸层，主线程内联 ──────────

#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};

    use gesso_core::WallpaperKind;
    use gesso_core::config::MacLockScreenBackup;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{
        NSColor, NSScreen, NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey,
        NSWorkspaceDesktopImageFillColorKey, NSWorkspaceDesktopImageScalingKey,
    };
    use objc2_foundation::{NSDictionary, MainThreadMarker, NSNumber, NSString, NSURL};

    use super::{now_ms, DEBOUNCE, MAX_RETRY, RETRY_DELAY};
    use crate::session::SessionManager;

    /// 壁纸呈现选项字典（键类型 = NSString 别名，值混型）。
    type OptionsDict = NSDictionary<NSString, AnyObject>;

    /// 同步到期时刻（epoch ms；0 = 无待办）。
    static DUE_MS: AtomicU64 = AtomicU64::new(0);
    static RETRIES: AtomicU32 = AtomicU32::new(0);
    /// 即时应用诉求（开关打开）：贴现成封面帧，不抓帧。
    static APPLY_ONLY: AtomicBool = AtomicBool::new(false);
    static RESTORE_PENDING: AtomicBool = AtomicBool::new(false);
    /// 能力探测：0 = 未探测，1 = 可用，2 = 不可用（开关置灰依据）。
    static AVAIL: AtomicU8 = AtomicU8::new(0);

    pub fn available() -> bool {
        AVAIL.load(Ordering::Acquire) == 1
    }

    /// 探测是否已完成（未完成期间设置页不渲染开关行，避免闪烁）。
    pub fn probed() -> bool {
        AVAIL.load(Ordering::Acquire) != 0
    }

    /// 启动探测（主线程，bootstrap 调用）：NSWorkspace 桌面读写全系统可用，
    /// 读一次桌面 URL 即完成——无需 Windows 侧的后台类激活测试。
    pub fn probe_async() {
        let ok = MainThreadMarker::new()
            .and_then(|_| desktop_image_url())
            .is_some();
        AVAIL.store(if ok { 1 } else { 2 }, Ordering::Release);
        println!("[lockscreen] 能力探测：{ok}");
    }

    pub fn schedule_sync() {
        RETRIES.store(0, Ordering::Relaxed);
        DUE_MS.store(now_ms() + DEBOUNCE.as_millis() as u64, Ordering::Release);
    }

    /// 开关打开的即时应用（用户直接动作）：封面现成 → 直接贴上，亚秒级生效。
    pub fn schedule_sync_immediate() {
        APPLY_ONLY.store(true, Ordering::Release);
    }

    pub fn schedule_restore() {
        RESTORE_PENDING.store(true, Ordering::Release);
    }

    /// 条目移除：macOS 封面帧在条目自身目录（随条目一并删除），无独立应用缓存。
    pub fn discard(_entry_id: &str) {}

    /// 主循环每拍调用（主线程）。NSWorkspace 读写必须在主线程，而单次调用是
    /// 毫秒级 IPC（写入经壁纸代理异步生效，实测 1–3s）——直接内联执行，无需
    /// Windows 侧的后台线程 + 簿记回灌结构。
    pub fn tick(sm: &mut SessionManager) {
        // 还原优先（用户关闭开关 / 释放）
        if RESTORE_PENDING.swap(false, Ordering::AcqRel) {
            run_restore(sm);
            return;
        }
        // 即时应用（开关打开）
        if APPLY_ONLY.swap(false, Ordering::AcqRel) {
            if !sm.config().settings.lockscreen_sync || !available() {
                return;
            }
            if !apply_poster(sm) {
                // 封面缺失：兜底路径（防抖全量同步，main 侧同时排了队）随后重试
                println!("[lockscreen] 即时应用未就绪（封面缺失/会话未就绪）");
            }
            return;
        }
        // 防抖全量同步：到期 + 开关仍开 + 能力可用
        let due = DUE_MS.load(Ordering::Acquire);
        if due == 0 || now_ms() < due {
            return;
        }
        if !sm.config().settings.lockscreen_sync || !available() {
            DUE_MS.store(0, Ordering::Release);
            RETRIES.store(0, Ordering::Relaxed);
            return;
        }
        if apply_poster(sm) {
            DUE_MS.store(0, Ordering::Release);
            RETRIES.store(0, Ordering::Relaxed);
        } else {
            retry_or_drop();
        }
    }

    /// 重试或放弃：未超限则重排下一拍，超限放弃本轮（下次指派重来）。
    fn retry_or_drop() {
        if RETRIES.fetch_add(1, Ordering::Relaxed) + 1 >= MAX_RETRY {
            println!("[lockscreen] 封面持续未就绪，本轮放弃（下次指派重来）");
            DUE_MS.store(0, Ordering::Release);
            RETRIES.store(0, Ordering::Relaxed);
        } else {
            DUE_MS.store(now_ms() + RETRY_DELAY.as_millis() as u64, Ordering::Release);
        }
    }

    /// 当前主屏壁纸的可用封面：导入生成的 poster → jpg/png 静图主文件直出。
    /// 都无 = None（等存量补齐；macOS 无 Windows 侧的活窗兜底抓帧）。
    fn poster_path(sm: &SessionManager, entry_id: &str) -> Option<PathBuf> {
        let entry = sm.library().iter().find(|e| e.id == entry_id)?.clone();
        let home = crate::encoding::thumb_home(&entry);
        for name in ["poster.jpg", "poster.png"] {
            let p = Path::new(&home).join(name);
            if p.is_file() {
                return Some(p);
            }
        }
        // jpg/png 静图：主文件即封面（导入时不生成，直接引用，零拷贝）
        if entry.kind == WallpaperKind::Image {
            let main =
                Path::new(&entry.source_dir).join(crate::encoding::entry_main_source(&entry));
            let ext = main
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(ext.as_str(), "jpg" | "jpeg" | "png") && main.is_file() {
                return Some(main);
            }
        }
        None
    }

    /// 应用封面到桌面壁纸层（锁屏跟随）。首次写入前读取并记录原壁纸簿记。
    fn apply_poster(sm: &mut SessionManager) -> bool {
        let Some(entry_id) = sm.main_wallpaper_entry_id() else {
            return false; // 主屏会话未就绪
        };
        let Some(poster) = poster_path(sm, &entry_id) else {
            return false; // 封面缺失（存量补齐中/生成失败）
        };
        // 簿记必须先于首次写入读取（记录的是「打开前」状态）；读不到就不写——
        // 无还原凭据的写入 = 异常时无法收回的残留
        if sm.mac_lockscreen_backup().is_none() {
            match read_desktop_state() {
                Some(b) => {
                    println!("[lockscreen] 备份簿记落盘（{}）", b.image_url);
                    sm.record_mac_lockscreen_backup(Some(b));
                }
                None => {
                    println!("[lockscreen] 原壁纸状态读取失败，本轮不写入（稍后重试）");
                    return false;
                }
            }
        }
        if set_desktop_image(&poster) {
            println!("[lockscreen] 封面已应用（{}）", poster.display());
            true
        } else {
            println!("[lockscreen] 封面应用失败：{}", poster.display());
            false
        }
    }

    /// 退出路径的还原（Quit 动作内联调用；主线程，毫秒级）。
    pub fn restore_best_effort(sm: &mut SessionManager) {
        if sm.mac_lockscreen_backup().is_none() {
            return;
        }
        println!("[lockscreen] 退出还原（best-effort）");
        run_restore(sm);
    }

    /// 恢复打开前的桌面壁纸（URL + 呈现选项照原样写回）。写回成功才清簿记——
    /// 失败保留凭据，下次启动清理 / 下次还原重试（自愈）。
    fn run_restore(sm: &mut SessionManager) {
        DUE_MS.store(0, Ordering::Release);
        RETRIES.store(0, Ordering::Relaxed);
        let Some(b) = sm.mac_lockscreen_backup() else {
            return; // 无簿记 = 未覆盖过，无需还原
        };
        if set_desktop_url(&b) {
            println!("[lockscreen] 已恢复打开前的桌面壁纸");
            sm.record_mac_lockscreen_backup(None);
        } else {
            println!("[lockscreen] 原壁纸写回失败（{}），保留簿记待重试", b.image_url);
        }
    }

    // ── NSWorkspace 原子操作（全部主线程）────────────────────────────────

    /// 当前主屏桌面壁纸状态（URL + 呈现选项）。读不到 = None（不做备份）。
    fn read_desktop_state() -> Option<MacLockScreenBackup> {
        let mtm = MainThreadMarker::new()?;
        let screen = main_screen(&mtm)?;
        let ws = NSWorkspace::sharedWorkspace();
        let url = ws.desktopImageURLForScreen(&screen)?;
        let options = ws.desktopImageOptionsForScreen(&screen);
        // 选项逐项提取（dict 值为混型 AnyObject，键常量已知直接取）
        let get_num = |key: &'static NSString| -> Option<i64> {
            let obj = options.as_ref()?.objectForKey(key)?;
            let num: Retained<NSNumber> = obj.downcast().ok()?;
            Some(num.as_isize() as i64)
        };
        let get_bool = |key: &'static NSString| -> Option<bool> {
            let obj = options.as_ref()?.objectForKey(key)?;
            let num: Retained<NSNumber> = obj.downcast().ok()?;
            Some(num.as_bool())
        };
        let fill_color = options.as_ref().and_then(|d| {
            let obj = d.objectForKey(unsafe { NSWorkspaceDesktopImageFillColorKey })?;
            let color: Retained<NSColor> = obj.downcast().ok()?;
            let (r, g, bl, a) = (
                color.redComponent(),
                color.greenComponent(),
                color.blueComponent(),
                color.alphaComponent(),
            );
            Some([r, g, bl, a])
        });
        Some(MacLockScreenBackup {
            image_url: url.absoluteString()?.to_string(),
            scaling: get_num(unsafe { NSWorkspaceDesktopImageScalingKey }),
            allow_clipping: get_bool(unsafe { NSWorkspaceDesktopImageAllowClippingKey }),
            fill_color,
        })
    }

    /// 应用本地文件到桌面壁纸层（默认选项——与 spike 实测一致，OS 落自己的呈现默认）。
    fn set_desktop_image(path: &Path) -> bool {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_string_lossy().as_ref()));
        apply_desktop_url(&url, None)
    }

    /// 按簿记写回原壁纸（含原缩放/裁剪/底色选项）。
    fn set_desktop_url(b: &MacLockScreenBackup) -> bool {
        let Some(url) = NSURL::URLWithString(&NSString::from_str(&b.image_url)) else {
            return false;
        };
        apply_desktop_url(&url, Some(b))
    }

    /// 写桌面壁纸本体：backup = None 用默认呈现；Some(簿记) 按原选项写回。
    fn apply_desktop_url(url: &NSURL, backup: Option<&MacLockScreenBackup>) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let Some(screen) = main_screen(&mtm) else {
            return false;
        };
        let ws = NSWorkspace::sharedWorkspace();
        // options 参数非可空：无簿记时传空 dict（= 默认呈现，与 spike 传 [:] 一致）
        let options: Retained<OptionsDict> =
            backup.map(build_options_dict).unwrap_or_default();
        let result = unsafe {
            ws.setDesktopImageURL_forScreen_options_error(url, &screen, &options)
        };
        if let Err(e) = &result {
            println!(
                "[lockscreen] NSWorkspace 错误：{}",
                e.localizedDescription()
            );
        }
        result.is_ok()
    }

    /// 按簿记重建呈现选项 dict（键 = NSWorkspace 官方常量；各项 None 时跳过，
    /// OS 落默认）。NSNumber/NSColor 沿 into_super 链上转为字典的值类型。
    fn build_options_dict(b: &MacLockScreenBackup) -> Retained<OptionsDict> {
        let mut keys: Vec<&'static NSString> = Vec::new();
        let mut objs: Vec<Retained<AnyObject>> = Vec::new();
        if let Some(scaling) = b.scaling {
            keys.push(unsafe { NSWorkspaceDesktopImageScalingKey });
            objs.push(NSNumber::new_isize(scaling as isize).into_super().into());
        }
        if let Some(clipping) = b.allow_clipping {
            keys.push(unsafe { NSWorkspaceDesktopImageAllowClippingKey });
            objs.push(NSNumber::new_bool(clipping).into_super().into());
        }
        if let Some([r, g, bl, a]) = b.fill_color {
            keys.push(unsafe { NSWorkspaceDesktopImageFillColorKey });
            objs.push(
                NSColor::colorWithCalibratedRed_green_blue_alpha(r, g, bl, a)
                    .into_super()
                    .into(),
            );
        }
        let val_refs: Vec<&AnyObject> = objs.iter().map(|o| &**o).collect();
        NSDictionary::from_slices(&keys, &val_refs)
    }

    fn main_screen(mtm: &MainThreadMarker) -> Option<Retained<NSScreen>> {
        NSScreen::mainScreen(*mtm)
    }

    fn desktop_image_url() -> Option<String> {
        let mtm = MainThreadMarker::new()?;
        let screen = main_screen(&mtm)?;
        let ws = NSWorkspace::sharedWorkspace();
        let url = ws.desktopImageURLForScreen(&screen)?;
        Some(url.absoluteString()?.to_string())
    }
}

#[cfg(target_os = "macos")]
pub use imp::*;

// ── 其他平台：全部 no-op ────────────────────────────────────────────────

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod imp {
    use super::SessionManager;

    pub fn schedule_sync() {}
    pub fn schedule_sync_immediate() {}
    pub fn schedule_restore() {}
    pub fn probe_async() {}
    pub fn discard(_entry_id: &str) {}
    pub fn available() -> bool {
        false
    }
    pub fn tick(_sm: &mut SessionManager) {}
    pub fn restore_best_effort(_sm: &mut SessionManager) {}
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub use imp::*;

// ── Windows 实现 ───────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod imp {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
    use std::sync::Mutex;

    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Storage::StorageFile;
    use windows::System::UserProfile::LockScreen;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    use windows::Win32::System::Registry::{
        RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_DWORD,
        RRF_RT_REG_DWORD,
    };

    use gesso_core::config::LockScreenBackup;
    use gesso_core::WallpaperKind;

    use super::{now_ms, DEBOUNCE, MAX_RETRY, RETRY_DELAY};
    use crate::session::SessionManager;

    const CDM_SUBKEY: PCWSTR =
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager");
    const POLICY_SUBKEY: PCWSTR = w!("SOFTWARE\\Policies\\Microsoft\\Windows\\Personalization");

    /// 同步到期时刻（epoch ms；0 = 无待办）。
    static DUE_MS: AtomicU64 = AtomicU64::new(0);
    static RETRIES: AtomicU32 = AtomicU32::new(0);
    /// 即时应用诉求（开关打开）：贴现成缓存帧，不抓帧。
    static APPLY_ONLY: AtomicBool = AtomicBool::new(false);
    static RESTORE_PENDING: AtomicBool = AtomicBool::new(false);
    /// 写入/还原串行化：同一时刻至多一个后台任务。
    static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
    /// 能力探测结果：0 = 未探测，1 = 可用，2 = 不可用（开关置灰依据）。
    static AVAIL: AtomicU8 = AtomicU8::new(0);
    /// 写入线程产出的备份，由主循环 tick 回灌 config（后台线程不碰 config）。
    static BACKUP_DONE: Mutex<Option<LockScreenBackup>> = Mutex::new(None);
    /// 还原线程完成标志：tick 回灌时清掉备份簿记。
    static RESTORE_DONE: AtomicBool = AtomicBool::new(false);

    pub fn available() -> bool {
        AVAIL.load(Ordering::Acquire) == 1
    }

    /// 探测是否已完成（未完成期间设置页不渲染开关行，避免闪烁）。
    pub fn probed() -> bool {
        AVAIL.load(Ordering::Acquire) != 0
    }

    pub fn schedule_sync() {
        RETRIES.store(0, Ordering::Relaxed);
        DUE_MS.store(now_ms() + DEBOUNCE.as_millis() as u64, Ordering::Release);
    }

    /// 开关打开的即时应用（用户直接动作）：封面帧/缓存现成 → 直接贴上锁屏，
    /// 亚秒级生效。无可用帧时回落 schedule_sync 的兜底抓帧路径（main 侧两个都排）。
    pub fn schedule_sync_immediate() {
        APPLY_ONLY.store(true, Ordering::Release);
    }

    /// 兜底抓帧的应用缓存：`config_dir/lockscreen/<entry_id>.png`（按条目隔离，
    /// 防止 A 壁纸的旧帧贴到 B 上）。
    fn apply_cache_path(entry_id: &str) -> PathBuf {
        crate::protocol::config_dir()
            .join("lockscreen")
            .join(format!("{entry_id}.png"))
    }

    /// 条目移除时清理其应用缓存（Remove 动作调用）。
    pub fn discard(entry_id: &str) {
        let p = apply_cache_path(entry_id);
        if p.is_file() {
            let _ = std::fs::remove_file(&p);
            println!("[lockscreen] 已清理 {} 的应用缓存", entry_id);
        }
    }

    pub fn schedule_restore() {
        RESTORE_PENDING.store(true, Ordering::Release);
    }

    /// 启动能力探测（一次）：组策略 + LockScreen 类激活。后台线程，不碰主线程。
    pub fn probe_async() {
        std::thread::spawn(|| {
            let ok = com_init(||
                // 激活测试：读一次静态属性即完成工厂激活（探针已验证的判定法）
                LockScreen::OriginalImageFile().is_ok() && !policy_locks_lockscreen());
            AVAIL.store(if ok { 1 } else { 2 }, Ordering::Release);
            println!("[lockscreen] 能力探测：{ok}");
        });
    }

    /// 主循环每拍调用（主线程）：消费到期任务 + 回灌后台结果。
    pub fn tick(sm: &mut SessionManager) {
        // 后台结果回灌（写完 → 记簿记；还原完 → 清簿记）
        if let Some(b) = BACKUP_DONE.lock().unwrap_or_else(|e| e.into_inner()).take() {
            println!("[lockscreen] 备份簿记落盘（rotating={}）", b.rotating);
            sm.record_lockscreen_backup(Some(b));
        }
        if RESTORE_DONE.swap(false, Ordering::AcqRel) {
            println!("[lockscreen] 已恢复打开前的锁屏设置");
            sm.record_lockscreen_backup(None);
        }

        // 还原优先（用户关闭开关 / 释放），且等在途写入结束
        if RESTORE_PENDING.load(Ordering::Acquire) {
            if IN_FLIGHT.swap(true, Ordering::AcqRel) {
                return; // 写入中：还原诉求保持，下一拍再试
            }
            RESTORE_PENDING.store(false, Ordering::Release);
            DUE_MS.store(0, Ordering::Release);
            match sm.lockscreen_backup() {
                Some(b) => spawn_restore(b),
                None => IN_FLIGHT.store(false, Ordering::Release), // 无簿记 = 无需还原
            }
            return;
        }

        // 即时应用（开关打开）：封面帧/缓存现成 → 直接贴上（亚秒级，不抓帧）。
        // 无可用帧时不在此处抓——schedule_sync 的兜底抓帧路径随后处理
        if APPLY_ONLY.swap(false, Ordering::AcqRel) {
            if IN_FLIGHT.swap(true, Ordering::AcqRel) {
                APPLY_ONLY.store(true, Ordering::Release); // 在途：诉求保持，下一拍再试
                return;
            }
            if !sm.config().settings.lockscreen_sync || AVAIL.load(Ordering::Acquire) == 2 {
                IN_FLIGHT.store(false, Ordering::Release);
                return;
            }
            match sm.main_wallpaper_entry_id().and_then(|id| {
                poster_or_cache(sm, &id).map(|p| (p, sm.lockscreen_backup().is_none(), id))
            }) {
                Some((path, need_backup, _)) => spawn_apply(path, need_backup),
                None => IN_FLIGHT.store(false, Ordering::Release), // 无帧 → 兜底路径随后
            }
            return;
        }

        // 全量同步：到期 + 开关仍开 + 能力可用
        let due = DUE_MS.load(Ordering::Acquire);
        if due == 0 || now_ms() < due {
            return;
        }
        if IN_FLIGHT.swap(true, Ordering::AcqRel) {
            return;
        }
        // 写入前复查开关（防抖间隙内可能被关掉——规格 §4.8）
        if !sm.config().settings.lockscreen_sync || AVAIL.load(Ordering::Acquire) == 2 {
            DUE_MS.store(0, Ordering::Release);
            RETRIES.store(0, Ordering::Relaxed);
            IN_FLIGHT.store(false, Ordering::Release);
            return;
        }
        // 封面帧优先（导入时已生成）：零抓帧，即指即得
        let Some(entry_id) = sm.main_wallpaper_entry_id() else {
            IN_FLIGHT.store(false, Ordering::Release);
            retry_or_drop("主屏会话未就绪");
            return;
        };
        if let Some(p) = poster_or_cache(sm, &entry_id) {
            DUE_MS.store(0, Ordering::Release);
            RETRIES.store(0, Ordering::Relaxed);
            let need_backup = sm.lockscreen_backup().is_none();
            spawn_apply(p, need_backup);
            return;
        }
        // 兜底：封面缺失（旧条目/生成失败）→ 活窗 PrintWindow 抓一帧，
        // 产物存为该条目的应用缓存，下次直接用
        let Some(hwnd) = sm.main_wallpaper_hwnd() else {
            IN_FLIGHT.store(false, Ordering::Release);
            retry_or_drop("主屏窗口未就绪");
            return;
        };
        DUE_MS.store(0, Ordering::Release);
        RETRIES.store(0, Ordering::Relaxed);
        let need_backup = sm.lockscreen_backup().is_none();
        spawn_write(hwnd, &entry_id, need_backup);
    }

    /// 重试或放弃（IN_FLIGHT 已持有时调用）：未超限则重排下一拍，超限放弃本轮。
    fn retry_or_drop(reason: &str) -> bool {
        if RETRIES.fetch_add(1, Ordering::Relaxed) + 1 >= MAX_RETRY {
            println!("[lockscreen] {reason}，本轮放弃（下次指派重来）");
            DUE_MS.store(0, Ordering::Release);
            RETRIES.store(0, Ordering::Relaxed);
            IN_FLIGHT.store(false, Ordering::Release);
            false
        } else {
            DUE_MS.store(now_ms() + RETRY_DELAY.as_millis() as u64, Ordering::Release);
            IN_FLIGHT.store(false, Ordering::Release);
            true
        }
    }

    /// 当前主屏壁纸的可用封面：导入生成的 poster → jpg/png 静图主文件直出 →
    /// 活窗兜底缓存。都无 = None（走兜底抓帧）。
    fn poster_or_cache(sm: &SessionManager, entry_id: &str) -> Option<PathBuf> {
        let entry = sm.library().iter().find(|e| e.id == entry_id)?.clone();
        let home = crate::encoding::thumb_home(&entry);
        for name in ["poster.jpg", "poster.png"] {
            let p = Path::new(&home).join(name);
            if p.is_file() {
                return Some(p);
            }
        }
        // jpg/png 静图：主文件即封面（导入时不生成，直接引用，零拷贝）
        if entry.kind == WallpaperKind::Image {
            let main =
                Path::new(&entry.source_dir).join(crate::encoding::entry_main_source(&entry));
            let ext = main
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(ext.as_str(), "jpg" | "jpeg" | "png") && main.is_file() {
                return Some(main);
            }
        }
        let cache = apply_cache_path(entry_id);
        (cache.is_file()).then_some(cache)
    }

    /// 退出路径的阻塞还原（Quit 动作内联调用；WinRT 单次调用毫秒级）。
    pub fn restore_best_effort(sm: &mut SessionManager) {
        let Some(b) = sm.lockscreen_backup() else {
            return;
        };
        println!("[lockscreen] 退出还原（阻塞，best-effort）");
        run_restore(&b);
        sm.record_lockscreen_backup(None);
    }

    /// 兜底抓帧：活窗 PrintWindow → PNG 存为该条目的应用缓存 → 贴上锁屏。
    fn spawn_write(hwnd: isize, entry_id: &str, need_backup: bool) {
        let out = apply_cache_path(entry_id);
        std::thread::spawn(move || {
            com_init(|| {
                if let Some(parent) = out.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let r = write_once(HWND(hwnd as _), need_backup, &out);
                println!(
                    "[lockscreen] 兜底抓帧 {}（{}×{}）",
                    if r.0 { "成功" } else { "失败" },
                    r.1,
                    r.2
                );
            });
            IN_FLIGHT.store(false, Ordering::Release);
        });
    }

    /// 应用现成帧（封面/缓存）到锁屏：只调 WinRT，无抓帧无编码。
    fn spawn_apply(path: PathBuf, need_backup: bool) {
        std::thread::spawn(move || {
            com_init(|| {
                // 备份仍须先于首次应用读取（记录「打开前」状态）
                let backup = if need_backup { read_state() } else { None };
                match set_image(&path) {
                    Ok(()) => {
                        println!("[lockscreen] 封面已应用（{:?}）", path);
                        if let Some(b) = backup {
                            BACKUP_DONE
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .replace(b);
                        }
                    }
                    Err(e) => println!("[lockscreen] 封面应用失败：{e}"),
                }
            });
            IN_FLIGHT.store(false, Ordering::Release);
        });
    }

    /// 兜底抓帧本体：活窗 → PNG → 写锁屏。返回 (成功, 宽, 高)。
    fn write_once(hwnd: HWND, need_backup: bool, out: &Path) -> (bool, i32, i32) {
        // 备份必须先于首次写入读取（记录的是「打开前」状态）
        let backup = if need_backup { read_state() } else { None };
        let Some((rgba, w, h)) = crate::capture_win::screenshot_window(hwnd) else {
            println!("[lockscreen] 壁纸窗抓帧失败（PrintWindow）");
            return (false, 0, 0);
        };
        if !crate::capture_win::encode_png_rgba(&rgba, w as u32, h as u32, out) {
            println!("[lockscreen] PNG 落盘失败");
            return (false, w, h);
        }
        if let Err(e) = set_image(out) {
            println!("[lockscreen] 写锁屏失败：{e}");
            return (false, w, h);
        }
        if let Some(b) = backup {
            BACKUP_DONE
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .replace(b);
        }
        (true, w, h)
    }

    fn spawn_restore(b: LockScreenBackup) {
        std::thread::spawn(move || {
            com_init(|| {
                run_restore(&b);
                RESTORE_DONE.store(true, Ordering::Release);
            });
            IN_FLIGHT.store(false, Ordering::Release);
        });
    }

    /// 恢复打开前的锁屏设置：先写回原图片（把记录路径清干净），聚焦模式再回切。
    fn run_restore(b: &LockScreenBackup) {
        let orig = PathBuf::from(&b.image_path);
        if !b.image_path.is_empty() && orig.is_file() {
            if let Err(e) = set_image(&orig) {
                println!("[lockscreen] 原锁屏图写回失败：{e}");
            }
        } else {
            println!(
                "[lockscreen] 原锁屏图不存在（{}），仅回切模式",
                b.image_path
            );
        }
        if b.rotating {
            set_rotating(true);
        }
    }

    fn com_init<T>(f: impl FnOnce() -> T) -> T {
        // 后台线程需要 COM 套间（WinRT 异步等待）；已初始化则忽略返回值
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        f()
    }

    // ── WinRT / 注册表原子操作 ──────────────────────────────────────────

    /// 当前锁屏状态：(聚焦模式, 原图片路径)。读不到 = None（不做备份）。
    fn read_state() -> Option<LockScreenBackup> {
        let rotating = rotating_enabled()?;
        let image_path = current_image_path().unwrap_or_default();
        Some(LockScreenBackup {
            rotating,
            image_path,
        })
    }

    fn current_image_path() -> Option<String> {
        let uri = LockScreen::OriginalImageFile().ok()?;
        // file:///C:/x/y.jpg → C:\x\y.jpg（Uri.Path 已百分号解码）
        let p = uri.Path().ok()?.to_string();
        let p = p.strip_prefix('/').unwrap_or(&p).replace('/', "\\");
        Some(p)
    }

    fn set_image(path: &Path) -> Result<(), String> {
        // WinRT GetFileFromPathAsync 拒收混合分隔符（config_dir 拼出
        // `C:\Users\x/.gesso\...`）——统一成正斜杠前的反斜杠形态
        let normalized: String = path.to_string_lossy().replace('/', "\\");
        let hstr = HSTRING::from(normalized.as_str());
        let file = StorageFile::GetFileFromPathAsync(&hstr)
            .map_err(|e| format!("GetFileFromPathAsync: {e}"))?
            .join()
            .map_err(|e| format!("GetFileFromPathAsync 等待: {e}"))?;
        LockScreen::SetImageFileAsync(&file)
            .map_err(|e| format!("SetImageFileAsync: {e}"))?
            .join()
            .map_err(|e| format!("SetImageFileAsync 等待: {e}"))
    }

    fn rotating_enabled() -> Option<bool> {
        Some(
            reg_dword(
                HKEY_CURRENT_USER,
                CDM_SUBKEY,
                w!("RotatingLockScreenEnabled"),
            )? == 1,
        )
    }

    fn set_rotating(on: bool) {
        let v: u32 = u32::from(on);
        let err = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                CDM_SUBKEY,
                w!("RotatingLockScreenEnabled"),
                REG_DWORD.0,
                Some(&v as *const u32 as *const _),
                std::mem::size_of::<u32>() as u32,
            )
        };
        if err.0 != 0 {
            println!("[lockscreen] 聚焦模式回切失败（RegSetKeyValueW {}）", err.0);
        }
    }

    /// 组策略「阻止更改锁屏图像」（企业机；个人机通常无此键）。
    fn policy_locks_lockscreen() -> bool {
        reg_dword(
            HKEY_LOCAL_MACHINE,
            POLICY_SUBKEY,
            w!("NoChangingLockScreen"),
        )
        .is_some_and(|v| v == 1)
    }

    fn reg_dword(
        hkey: windows::Win32::System::Registry::HKEY,
        subkey: PCWSTR,
        value: PCWSTR,
    ) -> Option<u32> {
        let mut v: u32 = 0;
        let mut cb = std::mem::size_of::<u32>() as u32;
        let err = unsafe {
            RegGetValueW(
                hkey,
                subkey,
                value,
                RRF_RT_REG_DWORD,
                None,
                Some(&mut v as *mut u32 as *mut _),
                Some(&mut cb),
            )
        };
        (err.0 == 0 && cb as usize == std::mem::size_of::<u32>()).then_some(v)
    }
}

#[cfg(target_os = "windows")]
pub use imp::*;
