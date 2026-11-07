//! 锁屏快照同步（设计规格 §4.8 / 技术方案 §9）：Windows 专属，opt-in，默认关。
//!
//! 打开后，指派收敛的壁纸以静态帧写入 Windows 锁屏图（官方
//! `Windows.System.UserProfile.LockScreen` API，WE「Override lock screen image」
//! 同款路线）；关闭开关 / 托盘「恢复系统壁纸」/ 退出时，恢复打开前的锁屏设置
//! （模式 + 图片路径，记在 `config.lockscreen_backup`）。
//!
//! 纪律（与 2026-10-13 桌面桥接被砍的结构对照）：
//! - 一次性后台写，**不进任何交互路径**（写入由主循环 150ms 拍消费到期任务，
//!   真活儿在专职后台线程；主线程只做纳秒级的原子检查）；
//! - **不写系统桌面**——锁屏图是独立 OS 表面；
//! - 能力（WinRT 缺失 / 组策略）启动探测一次，失败即静默停用；运行期单次写
//!   入失败只落日志，下次指派照常（无状态机）。
//!
//! macOS 全为 no-op：锁屏跟随桌面壁纸，而桌面壁纸代理正是桌面桥接被砍的病根。

// 跨平台 Clippy 的死代码豁免同 74996ee 先例（macOS 侧公共签名保留为 no-op）。
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 指派收敛后的防抖延迟（§4.8）：等两段式收敛与静帧稳定。
const DEBOUNCE: Duration = Duration::from_secs(3);
/// 主屏窗口未就绪（收敛中 / 隐藏态）时的重试间隔与上限；超限放弃本轮，
/// 下次指派重来。
const RETRY_DELAY: Duration = Duration::from_millis(1500);
const MAX_RETRY: u32 = 20;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── macOS / 非 Windows：全部 no-op ─────────────────────────────────────

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::SessionManager;

    pub fn schedule_sync() {}
    pub fn schedule_sync_immediate() {}
    pub fn schedule_restore() {}
    pub fn probe_async() {}
    pub fn available() -> bool {
        false
    }
    pub fn tick(_sm: &mut SessionManager) {}
    pub fn restore_best_effort(_sm: &mut SessionManager) {}
}

#[cfg(not(target_os = "windows"))]
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

    /// 开关打开的即时应用（用户直接动作）：贴现成缓存帧，亚秒级生效——
    /// 抓 4K 帧 + PNG 编码要 1–3s，现场抓会让打开后立刻锁屏的用户等旧状态。
    /// 随后的 schedule_sync（3s 全量）会补一帧更新鲜的。
    pub fn schedule_sync_immediate() {
        APPLY_ONLY.store(true, Ordering::Release);
    }

    /// 快照缓存路径：既是要贴上锁屏的图，也是随指派持续维护的帧缓存。
    fn cache_path() -> PathBuf {
        crate::protocol::config_dir().join("lockscreen.png")
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

        // 即时应用（开关打开）：缓存帧现成 → 直接贴上锁屏（亚秒级，不抓帧）。
        // 随后 schedule_sync 的 3s 全量刷新会补一帧更新鲜的；无缓存则落全量路径
        if APPLY_ONLY.swap(false, Ordering::AcqRel) {
            if IN_FLIGHT.swap(true, Ordering::AcqRel) {
                APPLY_ONLY.store(true, Ordering::Release); // 在途：诉求保持，下一拍再试
                return;
            }
            if !sm.config().settings.lockscreen_sync || AVAIL.load(Ordering::Acquire) == 2 {
                IN_FLIGHT.store(false, Ordering::Release);
                return;
            }
            if cache_path().is_file() {
                let need_backup = sm.lockscreen_backup().is_none();
                spawn_apply(need_backup);
            } else {
                IN_FLIGHT.store(false, Ordering::Release); // 无缓存 → 等 schedule_sync 全量抓帧
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
        // 主屏窗口未就绪：限次重试
        let Some(hwnd) = sm.main_wallpaper_hwnd() else {
            IN_FLIGHT.store(false, Ordering::Release);
            if RETRIES.fetch_add(1, Ordering::Relaxed) + 1 >= MAX_RETRY {
                println!("[lockscreen] 主屏窗口未就绪，本轮放弃（下次指派重来）");
                DUE_MS.store(0, Ordering::Release);
                RETRIES.store(0, Ordering::Relaxed);
            } else {
                DUE_MS.store(now_ms() + RETRY_DELAY.as_millis() as u64, Ordering::Release);
            }
            return;
        };
        DUE_MS.store(0, Ordering::Release);
        RETRIES.store(0, Ordering::Relaxed);
        let need_backup = sm.lockscreen_backup().is_none();
        spawn_write(hwnd, need_backup);
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

    fn spawn_write(hwnd: isize, need_backup: bool) {
        std::thread::spawn(move || {
            let out = cache_path();
            com_init(|| {
                let r = write_once(HWND(hwnd as _), need_backup, &out);
                println!(
                    "[lockscreen] 快照写入 {}（{}×{}）",
                    if r.0 { "成功" } else { "失败" },
                    r.1,
                    r.2
                );
            });
            IN_FLIGHT.store(false, Ordering::Release);
        });
    }

    /// 即时应用：缓存帧已存在，只调 WinRT 贴上锁屏（无抓帧无编码）。
    fn spawn_apply(need_backup: bool) {
        std::thread::spawn(move || {
            com_init(|| {
                // 备份仍须先于首次应用读取（记录「打开前」状态）
                let backup = if need_backup { read_state() } else { None };
                match set_image(&cache_path()) {
                    Ok(()) => {
                        println!("[lockscreen] 缓存帧已应用（即时）");
                        if let Some(b) = backup {
                            BACKUP_DONE
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .replace(b);
                        }
                    }
                    Err(e) => println!("[lockscreen] 缓存帧应用失败：{e}"),
                }
            });
            IN_FLIGHT.store(false, Ordering::Release);
        });
    }

    /// 抓主屏壁纸窗 → PNG → 写锁屏。返回 (成功, 宽, 高)。
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
