//! 桌面桥接（Desktop Bridge）：重配/建窗过渡的静帧供给。
//!
//! **2026-10-13 终局重构**（三轮卡顿排障 + 实测定案）：
//!
//! 旧路线（把静帧写进**系统桌面壁纸**以盖住过渡间隙）已整体移除——
//! macOS 壁纸代理 FIFO 每张 5–16 秒、拥塞时 `setDesktopImageURL` 单次
//! 实测 2 秒：任何运行时写入都在交互路径（主线程）制造 UI 冻结，且
//! 临时覆盖下静帧永远来不及在间隙内渲染，收益从未兑现（详见
//! CHANGELOG 0.1.3 / 工程笔记 §2）。
//!
//! 新路线：静帧只落本地文件（`bridge/<entry_id>.png`），过渡间隙由
//! **自家过渡窗**（pin 层毫秒级建窗）显示静帧盖住——全程不碰系统桌面。
//! 本模块职责收敛为三件事：
//! 1. 静帧文件供给（启动预生成 + 指派后按需补产，全部后台线程）；
//! 2. 旧版本残留的一次性启动清理（桌面仍是旧版桥接帧 → 恢复原壁纸，
//!    零等待，此后进程不再写系统桌面）；
//! 3. 条目移除时清理静帧文件。
//!
//! 残留清理的回退纪律（继承实测教训）：台账原值不可用（文件消失/航拍）
//! → 回退静态系统桌面图；**记录与恢复两侧都严禁航拍路径**——macOS 26
//! 的 `DefaultDesktop.heic` 是航拍视频壁纸软链，静态 API 设置渲染为黑屏。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use gesso_core::{LibraryEntry, WallpaperKind};

// ── 台账与回退（纯数据/纯函数，跨平台可测）─────────────────────────────

/// entry_id → 接管时该屏的原始桌面图（仅用于旧版本残留的启动清理）。
#[derive(Default, Serialize, Deserialize)]
struct Ledger {
    entries: BTreeMap<String, String>,
}

/// 是否旧版桥接图（按路径前缀判定——桥接图只产出到 `bridge_dir` 之下）。
fn is_bridge_path(bridge_dir: &Path, p: &str) -> bool {
    Path::new(p).starts_with(bridge_dir)
}

fn path_stem(p: &str) -> &str {
    Path::new(p)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
}

/// 静态回退桌面图探测序（取第一个存在的）。
const FALLBACK_DESKTOPS: &[&str] = &[
    "/System/Library/Desktop Pictures/Sonoma.heic",
    "/System/Library/Desktop Pictures/Mac Blue.heic",
];

/// 航拍桌面判定：静态 API 设置航拍路径渲染为黑屏（2026-10-13 实测）。
fn is_aerial_desktop(p: &str) -> bool {
    p == "/System/Library/CoreServices/DefaultDesktop.heic"
        || p.starts_with("/System/Library/Wallpapers/")
        || p.ends_with(".madesktop")
}

/// 静态回退目标解析（探测序取第一个存在的）；None = 无可回退。
fn static_fallback(exists: impl Fn(&str) -> bool) -> Option<String> {
    FALLBACK_DESKTOPS
        .iter()
        .copied()
        .find(|p| exists(p))
        .map(String::from)
}

/// 恢复目标决议：台账原值可用（存在且非航拍）→ 原值；否则 → 回退静态
/// 系统桌面图；全部不可用 → None（无恢复目标，落日志留台账待下次尝试）。
fn restore_target(original: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    if exists(original) && !is_aerial_desktop(original) {
        return Some(original.to_string());
    }
    static_fallback(exists)
}

// ── 主体 ────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct DesktopBridge {
    ledger: Ledger,
    /// 桥接图目录（`<config_dir>/bridge/`）。
    dir: PathBuf,
}

impl DesktopBridge {
    fn with_dir(dir: PathBuf) -> Self {
        let mut b = Self {
            ledger: Ledger::default(),
            dir,
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

    /// 静帧文件路径：`<dir>/<entry_id>.<ext>`。图片类沿用源扩展名，其余 png。
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

    /// 静帧供给（只做毫秒级文件操作；视频**绝不在此同步解码**——解码
    /// 秒级且曾在指派路径主线程执行导致冻结，现全部走后台，见
    /// `preseed_video_frames` / `ensure_frame_async`）。返回 true = 帧文件
    /// 已就位。图片 = 源文件拷贝（绝不把用户文件路径写进系统设置）；
    /// shader/html = 现成缩略图档拷贝。
    fn produce_frame(&self, entry: &LibraryEntry, out: &Path) -> bool {
        // 桥接目录可能尚不存在（首次产出早于任何 save() 的 create_dir_all）
        let _ = std::fs::create_dir_all(&self.dir);
        match entry.kind {
            WallpaperKind::Image => {
                let src = crate::encoding::entry_main_source(entry);
                let src = Path::new(&entry.source_dir).join(&src);
                src.exists() && std::fs::copy(&src, out).is_ok()
            }
            WallpaperKind::Video => {
                // 视频帧由后台线程解码产出，此处只认现成文件
                let _ = out;
                false
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
}

/// 视频首帧全分辨率提取（仅后台线程调用；复用 thumb.rs 的 objc2 所有权
/// 纪律与 ImageIO 写盘）。
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

/// 旧版本残留的一次性启动清理：扫描各屏桌面 URL，仍在旧版桥接目录下的
/// → set 回台账原值（或静态回退），零等待（渲染由壁纸代理自行完成）；
/// 恢复成功的清台账。此后进程不再写系统桌面。非 macOS no-op。
pub fn cleanup_legacy_takeovers() {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSDictionary, NSString, NSURL};

        let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let mut b = global().lock().expect("桌面桥接");
        let mut restored: Vec<String> = Vec::new();
        for (info, screen) in crate::pin::macos::screens() {
            let Some(url) = ws
                .desktopImageURLForScreen(&screen)
                .and_then(|u| u.path())
                .map(|p| p.to_string())
            else {
                continue;
            };
            if !is_bridge_path(&b.dir, &url) {
                continue;
            }
            let stem = path_stem(&url).to_string();
            let Some(original) = b.ledger.entries.get(&stem).cloned() else {
                println!("[bridge] 残留桥接帧无台账记录（{url}），跳过（安全优先）");
                continue;
            };
            let Some(target) = restore_target(&original, |p| Path::new(p).exists()) else {
                println!("[bridge] {stem} 原壁纸不可用（{original}）且无静态回退，留台账待下次");
                continue;
            };
            let nsurl = NSURL::fileURLWithPath(&NSString::from_str(&target));
            let empty = NSDictionary::new();
            // SAFETY: 主线程（启动路径）；存活 screen；target 已确认存在
            match unsafe { ws.setDesktopImageURL_forScreen_options_error(&nsurl, &screen, &empty) }
            {
                Ok(()) => {
                    println!("[bridge] {stem} 残留清理 → {target}（{}）", info.id);
                    restored.push(stem);
                }
                Err(e) => println!("[bridge] {stem} 残留清理失败：{e:?}"),
            }
        }
        if !restored.is_empty() {
            b.ledger.entries.retain(|k, _| !restored.contains(k));
            b.save();
        }
    }
}

/// 视频桥接帧启动预生成（后台线程，串行）：把库里缺帧的视频条目逐个
/// 解出全分辨率首帧（`.tmp` → rename 原子落盘）。主线程永不解码视频。
pub fn preseed_video_frames(library: &[LibraryEntry]) {
    #[cfg(target_os = "macos")]
    {
        let b = global().lock().expect("桌面桥接");
        let jobs: Vec<(PathBuf, PathBuf)> = library
            .iter()
            .filter(|e| e.kind == WallpaperKind::Video)
            .filter_map(|e| {
                let out = b.static_path(e);
                (!out.exists()).then(|| {
                    let src = Path::new(&e.source_dir).join(crate::encoding::entry_main_source(e));
                    (src, out)
                })
            })
            .filter(|(src, _)| src.exists())
            .collect();
        drop(b);
        if jobs.is_empty() {
            return;
        }
        println!("[bridge] 预生成 {} 个视频桥接帧（后台）", jobs.len());
        std::thread::spawn(move || {
            for (src, out) in jobs {
                let tmp = out.with_extension("png.tmp");
                if extract_video_first_frame(&src, &tmp) {
                    let _ = std::fs::rename(&tmp, &out);
                } else {
                    let _ = std::fs::remove_file(&tmp);
                }
            }
            println!("[bridge] 视频桥接帧预生成完成");
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = library;
    }
}

/// 指派后按需补产并解码静帧（后台线程）：
/// - 图片/shader/html：补拷贝帧文件（毫秒级）→ PNG 解码；
/// - 视频：等预生成产出的帧文件（最多 30s）→ PNG 解码；
/// - 图片类源为 jpeg 等非 PNG 格式，无解码器，跳过（v1 已知限制：图片条目
///   的重配间隙退回露出用户壁纸）。
///   解码结果进 FRAMES 缓存（RGBA，Send），过渡窗建窗时主线程包装成 NSImage。
pub fn refresh_frame(entry: &LibraryEntry) {
    #[cfg(target_os = "macos")]
    {
        if entry.kind == WallpaperKind::Image {
            return; // v1 限制：无通用图片解码器，不缓存（日志一次不打，静默降级）
        }
        let entry = entry.clone();
        std::thread::spawn(move || {
            let out = global().lock().expect("桌面桥接").static_path(&entry);
            if !out.exists() {
                let tmp = out.with_extension("tmp");
                if global()
                    .lock()
                    .expect("桌面桥接")
                    .produce_frame(&entry, &tmp)
                {
                    let _ = std::fs::rename(&tmp, &out);
                }
            }
            // 视频帧可能还在预生成中：有限等待（250ms × 120 = 30s 上限）
            for _ in 0..120 {
                if out.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            let Some((w, h, rgba)) = decode_png(&out) else {
                println!("[bridge] 静帧解码失败（{}）", out.display());
                return;
            };
            frames()
                .lock()
                .expect("静帧缓存")
                .insert(entry.id.clone(), DecodedFrame { w, h, rgba });
            println!("[bridge] 静帧就绪（{} {}×{}）", entry.title, w, h);
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = entry;
    }
}

/// 已解码静帧（后台线程生产，主线程消费）。RGBA 字节，Send 无虞。
#[derive(Clone)]
struct DecodedFrame {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
}

static FRAMES: OnceLock<Mutex<BTreeMap<String, DecodedFrame>>> = OnceLock::new();

fn frames() -> &'static Mutex<BTreeMap<String, DecodedFrame>> {
    FRAMES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn decode_png(path: &Path) -> Option<(usize, usize, Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => {
            let (chunks, _) = buf.as_chunks::<3>();
            chunks
                .iter()
                .flat_map(|px| [px[0], px[1], px[2], 0xFF])
                .collect()
        }
        _ => return None,
    };
    Some((w, h, rgba))
}

// —— 过渡窗生命周期（全部主线程；非 macOS no-op）——

#[cfg(target_os = "macos")]
thread_local! {
    /// monitor_id → 过渡窗。仅主线程触碰（AppKit），thread_local 免 Send 约束。
    static COVERS: std::cell::RefCell<BTreeMap<String, crate::pin::macos::MacCoverWindow>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
}

/// 遮盖一屏：显示该条目静帧的过渡窗（先盖后熄旧窗，无闪现）。无缓存帧则
/// 不遮盖（退回露出用户壁纸）。已存在同屏过渡窗则替换。
pub fn cover(monitor_id: &str, frame: (f64, f64, f64, f64), entry_id: &str) {
    #[cfg(target_os = "macos")]
    {
        let Some(f) = frames().lock().expect("静帧缓存").get(entry_id).cloned() else {
            return;
        };
        let mtm = match objc2::MainThreadMarker::new() {
            Some(m) => m,
            None => return,
        };
        let image = crate::pin::macos::nsimage_from_rgba(&f.rgba, f.w, f.h, mtm);
        match crate::pin::macos::create_cover_window(frame, image) {
            Ok(win) => {
                COVERS.with(|c| {
                    c.borrow_mut().insert(monitor_id.to_string(), win);
                });
                println!("[bridge] {monitor_id} 过渡窗遮盖（{}）", entry_id);
            }
            Err(e) => println!("[bridge] 过渡窗创建失败：{e}"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (monitor_id, frame, entry_id);
    }
}

/// 撤除一屏的过渡窗（真壁纸窗已点亮后调用）。
pub fn uncover(monitor_id: &str) {
    #[cfg(target_os = "macos")]
    {
        COVERS.with(|c| {
            c.borrow_mut().remove(monitor_id);
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = monitor_id;
    }
}

/// 撤除全部过渡窗（拆会话/退出路径）。
pub fn uncover_all() {
    #[cfg(target_os = "macos")]
    {
        COVERS.with(|c| {
            c.borrow_mut().clear();
        });
    }
}

/// 移除条目时顺手清静帧文件。
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

    #[test]
    fn bridge_path_detection_by_prefix() {
        let dir = Path::new("/cfg/bridge");
        assert!(is_bridge_path(dir, "/cfg/bridge/e1.png"));
        assert!(!is_bridge_path(dir, "/Users/u/wall.jpg"));
        // 前缀同名的兄弟目录不算（组件级比较，非字符串前缀）
        assert!(!is_bridge_path(dir, "/cfg/bridge-extra/e1.png"));
    }

    #[test]
    fn aerial_desktop_detection() {
        // macOS 26：DefaultDesktop.heic 是航拍软链，静态 API 渲染必黑屏
        assert!(is_aerial_desktop(
            "/System/Library/CoreServices/DefaultDesktop.heic"
        ));
        assert!(is_aerial_desktop(
            "/System/Library/Wallpapers/Aerial/xxx.heic"
        ));
        assert!(is_aerial_desktop("/Users/u/movie.madesktop"));
        assert!(!is_aerial_desktop("/Users/u/wall.jpg"));
        assert!(!is_aerial_desktop(
            "/System/Library/Desktop Pictures/Sonoma.heic"
        ));
    }

    #[test]
    fn restore_target_prefers_live_original_then_static_fallback() {
        let exists = |p: &str| p != "/gone.jpg";
        // 原值活着且非航拍 → 原值
        assert_eq!(
            restore_target("/Users/u/wall.jpg", exists).as_deref(),
            Some("/Users/u/wall.jpg")
        );
        // 原值消失 → 静态回退（探测序首个存在者）
        assert_eq!(
            restore_target("/gone.jpg", exists).as_deref(),
            Some("/System/Library/Desktop Pictures/Sonoma.heic")
        );
        // 航拍原值视同死值 → 强制静态回退（静态 API 渲染航拍必黑屏）
        assert_eq!(
            restore_target("/System/Library/CoreServices/DefaultDesktop.heic", exists).as_deref(),
            Some("/System/Library/Desktop Pictures/Sonoma.heic")
        );
    }
}
