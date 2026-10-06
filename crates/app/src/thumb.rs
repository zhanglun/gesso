//! 壁纸缩略图：统一的预览帧策略 + 视频抽帧（首帧 + 悬停预览帧序列）。
//!
//! # 统一模型
//! 缩略图 = "从条目得到可显示的帧序列"。按类型只有来源策略不同，输出契约
//! （卡片首帧 + hover 帧）、调度（ThumbScheduler 去重/重试/回灌）、消费
//! （widgets::preview）全统一。策略查 `gesso_core::content_type`，帧发现见 [`preview_frames`]：
//! - `Direct`（image：jpg/png/jpeg/avif/gif/webp）：源文件本身即位图，
//!   **零生成、零拷贝直引**；gif/webp 由 GPUI `img` 自动循环，无需抽帧。
//! - `Extract`（video）：后台 AVFoundation 抽帧。
//! - `Capture`（shader/html）：主线程 webview 快照（见 `capture.rs`）。
//!
//! v2 重写（2026-10-04）。v1 在后台线程裸调 `msg_send!` 人肉维护 ObjC 引用计数，
//! 有两处所有权违约（两次线上崩溃同指纹：GCD 池弹出时对已释放对象 release）：
//! ① alloc+init 出的对象被两次 `Retained::from_raw` 接管 → 每帧双重释放 NSBitmapImageRep；
//! ② `representationUsingType:properties:` 的 **autoreleased** 返回值被当作 +1 接管
//!    → drop 提前释放，autorelease 池里的悬垂记录在 GCD drain 结束时二次释放。
//! 教训沉淀为三条结构约束，本版按此重建：
//! 1. **所有权不裸写**：AVFoundation 走 objc2 生成绑定（init/copy 家族返回
//!    `Retained<T>` 由类型承载）；PNG 编码走 ImageIO `CGImageDestination`
//!    （纯 C API：+1 创建 → 使用 → CFRelease，无 ObjC 对象、无 autorelease）。
//! 2. **autoreleasepool 包整任务**：框架内部临时对象的释放时机确定化，
//!    不依赖宿主线程的池基础设施（GPUI 后台任务 = GCD 全局队列，per-drain 池）。
//! 3. **输出原子化**：先写 `.tmp.png` 再 rename——中途崩溃不留半截帧，
//!    `existing_frames` 的连续性判定不会数进坏文件。
//!
//! 调度侧约束（去重/重试上限）见 `engine::ThumbScheduler`；本模块只负责"把一个
//! 目录的缺帧补齐"这一件事。

use std::path::{Path, PathBuf};

use gesso_core::WallpaperKind;

/// 悬停轮播：以【原速】回放视频开头一个短片段。
///
/// - `SAMPLE_FPS`：采样帧率 = 回放帧率（8fps 轮播即 8fps 采样 → 每帧驻留
///   125ms = 抽帧间隔，速度与原片一致）
/// - 片段时长 = HOVER_FRAMES / SAMPLE_FPS = 2 秒（循环点轻微跳变可接受）
pub const SAMPLE_FPS: u64 = 8;
pub const HOVER_FRAMES: usize = 15; // 2 秒片段（含首帧共 16 帧）

/// 一次抽帧的结果（macOS AVFoundation 路径；Windows webview 管线直接计数）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct ThumbOutcome {
    /// 本次新写入的帧数（已缓存的帧不计）。
    pub written: usize,
}

pub(crate) fn thumb_path(dir: &Path, idx: usize) -> PathBuf {
    if idx == 0 {
        dir.join("thumb.png")
    } else {
        dir.join(format!("thumb-{idx}.png"))
    }
}

/// 已存在的帧路径列表（index 0 = 首帧）。
///
/// 连续性语义：遇到第一个缺口即止——轮播按索引取帧，中段缺帧宁可回退静态首帧，
/// 也不让 UI 拿到有洞的序列。
pub fn existing_frames(source_dir: &str) -> Vec<String> {
    let dir = Path::new(source_dir);
    let mut out = Vec::new();
    for i in 0..=HOVER_FRAMES {
        let p = thumb_path(dir, i);
        if p.exists() {
            out.push(p.display().to_string());
        } else {
            break;
        }
    }
    out
}

/// 统一的预览帧发现（snapshot / 兜底扫描共用）：
/// - Direct：直引源文件（找 `index.<图片扩展名>`），无生成帧也能立即显示；
/// - 其余：`existing_frames` 的 `thumb.png` 连续序列。
pub fn preview_frames(source_dir: &str, kind: WallpaperKind) -> Vec<String> {
    let ct = gesso_core::content_type(kind);
    if ct.thumb == gesso_core::ThumbStrategy::Direct {
        let dir = Path::new(source_dir);
        if let Ok(rd) = std::fs::read_dir(dir) {
            for ent in rd.flatten() {
                let name = ent.file_name();
                let n = name.to_string_lossy();
                let is_img = n
                    .strip_prefix("index.")
                    .map(|e| ct.extensions.contains(&e.to_ascii_lowercase().as_str()))
                    .unwrap_or(false);
                if is_img {
                    return vec![ent.path().display().to_string()];
                }
            }
        }
        return Vec::new();
    }
    existing_frames(source_dir)
}

/// 采样时刻表（纯逻辑）：第 i 帧取 t = i / SAMPLE_FPS；不足 2 秒的短视频按
/// 实际时长钳制（保证相邻帧 ≥ 原速间隔，最末帧不越界，全部落在 (0, duration) 内）。
pub(crate) fn frame_times(duration_secs: f64) -> Vec<f64> {
    let total = HOVER_FRAMES + 1;
    let clip = duration_secs.min(total as f64 / SAMPLE_FPS as f64);
    (0..total)
        .map(|i| ((i as f64) / SAMPLE_FPS as f64).min(clip - 0.05))
        .collect()
}

/// 补齐一个条目目录的帧序列（后台任务调用；已存在的帧跳过）。
/// 失败静默：单帧失败继续后面的帧；调用方经 `ThumbScheduler` 的重试上限负责
/// 不对持续失败的条目无限空转。
#[cfg(target_os = "macos")]
pub fn extract_frames(source_dir: &str) -> ThumbOutcome {
    objc2::rc::autoreleasepool(|_| extract_frames_inner(source_dir))
}

/// 把已渲染的 CGImage 编码为 PNG 写盘（shader 缩略图采集复用 ImageIO 管线）。
/// 原子写：先 .tmp 再 rename，`existing_frames` 的连续性判定不会数进半截文件。
#[cfg(target_os = "macos")]
pub fn write_png(image: &objc2::rc::Retained<objc2_core_graphics::CGImage>, path: &Path) -> bool {
    let Some(uti) = ffi::cf_string("public.png") else {
        return false;
    };
    let tmp = path.with_extension("png.tmp");
    let ok = ffi::write_png_to_file(image, &tmp, uti);
    ffi::cf_release(uti);
    if ok && std::fs::rename(&tmp, path).is_ok() {
        true
    } else {
        let _ = std::fs::remove_file(&tmp);
        false
    }
}

#[cfg(target_os = "macos")]
fn extract_frames_inner(source_dir: &str) -> ThumbOutcome {
    use objc2_av_foundation::{AVAssetImageGenerator, AVURLAsset};
    use objc2_core_media::CMTime;
    use objc2_foundation::{NSString, NSURL};

    let dir = Path::new(source_dir);
    let src = dir.join("index.mp4");
    if !src.exists() {
        return ThumbOutcome::default();
    }

    // 所有权不裸写：生成绑定的便利构造器/属性方法已按 ARC 语义正确处理
    // autorelease（fileURLWithPath 是 autoreleased 返回，绑定内部走
    // objc_retainAutoreleasedReturnValue）——这正是 v1 裸 msg_send 踩坑的位置。
    let url = NSURL::fileURLWithPath(&NSString::from_str(src.display().to_string().as_str()));
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
    let seconds = unsafe { asset.duration().seconds() };
    if !(seconds.is_finite() && seconds > 0.1) {
        return ThumbOutcome::default();
    }

    let gen = unsafe { AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset) };
    // 帧精度 + 免裁剪原分辨率（v1 定稿参数，SPIKE-REPORT M1.5）
    unsafe {
        gen.setRequestedTimeToleranceBefore(CMTime::new(0, 600));
        gen.setRequestedTimeToleranceAfter(CMTime::new(0, 600));
        gen.setAppliesPreferredTrackTransform(true);
    }

    // PNG UTI（public.png 稳定不变），整个任务共用一份 CFString
    let png_uti = ffi::cf_string("public.png");
    let Some(png_uti) = png_uti else {
        return ThumbOutcome::default();
    };

    let mut written = 0usize;
    for (i, t) in frame_times(seconds).into_iter().enumerate() {
        let out_path = thumb_path(dir, i);
        if out_path.exists() {
            continue;
        }
        // copyCGImageAtTime 是 copy 家族：Retained<CGImage> drop = CFRelease，对称。
        // Apple 把同步版标记 deprecated（推异步回调变体）——本场景是后台任务里的
        // 有限帧循环，同步形态最简且正确，见方法上的 allow(deprecated)。
        let image = {
            #[allow(deprecated)]
            match unsafe {
                gen.copyCGImageAtTime_actualTime_error(
                    CMTime::with_seconds(t, 600),
                    std::ptr::null_mut(),
                )
            } {
                Ok(image) => image,
                Err(_) => continue,
            }
        };

        // 临时文件以 .png 结尾（ImageIO 按 URL 扩展名推断格式），且以 . 开头隐藏，
        // 不会被 existing_frames 的连续性判定数进去
        let file_name = out_path.file_name().unwrap_or_default().to_string_lossy();
        let tmp_path = dir.join(format!(".{file_name}.tmp.png"));

        let ok = ffi::write_png_to_file(&image, &tmp_path, png_uti);
        if ok && std::fs::rename(&tmp_path, &out_path).is_ok() {
            written += 1;
        } else if tmp_path.exists() {
            let _ = std::fs::remove_file(&tmp_path);
        }
    }

    ffi::cf_release(png_uti);
    ThumbOutcome { written }
}

/// 纯 C 的 CoreFoundation / ImageIO 面：每处创建都是 +1，配对 CFRelease。
/// 没有 ObjC 对象、没有 autorelease——所有权一眼可审计。
#[cfg(target_os = "macos")]
mod ffi {
    use std::ffi::{c_char, c_void};

    use objc2::rc::Retained;
    use objc2_core_graphics::CGImage;

    /// CF 引用（CFStringRef / CFURLRef / CGImageDestinationRef 通吃）。
    type CFRef = *const c_void;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_CF_URL_POSIX_PATH_STYLE: isize = 0;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C-unwind" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFRef;
        fn CFURLCreateWithFileSystemPath(
            alloc: *const c_void,
            path: CFRef,
            path_style: isize,
            is_directory: u8,
        ) -> CFRef;
        fn CFRelease(cf: CFRef);
    }

    #[link(name = "ImageIO", kind = "framework")]
    extern "C-unwind" {
        fn CGImageDestinationCreateWithURL(
            url: CFRef,
            uti_type: CFRef,
            image_count: usize,
            options: CFRef,
        ) -> *mut c_void;
        fn CGImageDestinationAddImage(dest: *mut c_void, image: CFRef, properties: CFRef);
        fn CGImageDestinationFinalize(dest: *mut c_void) -> u8;
    }

    /// 从 &str 建 CFString（+1；调用方负责 CFRelease）。
    pub(super) fn cf_string(s: &str) -> Option<CFRef> {
        let c = std::ffi::CString::new(s).ok()?;
        let ptr = unsafe {
            CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_UTF8)
        };
        (!ptr.is_null()).then_some(ptr)
    }

    pub(super) fn cf_release(cf: CFRef) {
        if !cf.is_null() {
            unsafe { CFRelease(cf) };
        }
    }

    /// 把 CGImage 编码为 PNG 写入目标路径（ImageIO 自行处理像素格式/色彩空间）。
    /// 返回 true = 文件已完整落盘（调用方再 rename 转正）。
    pub(super) fn write_png_to_file(
        image: &Retained<CGImage>,
        path: &Path,
        png_uti: CFRef,
    ) -> bool {
        let Some(path_cf) = cf_string(&path.display().to_string()) else {
            return false;
        };
        let url = unsafe {
            CFURLCreateWithFileSystemPath(std::ptr::null(), path_cf, K_CF_URL_POSIX_PATH_STYLE, 0)
        };
        let dest = (!url.is_null())
            .then(|| unsafe { CGImageDestinationCreateWithURL(url, png_uti, 1, std::ptr::null()) });
        let result = match dest {
            Some(d) if !d.is_null() => unsafe {
                CGImageDestinationAddImage(
                    d,
                    std::ptr::from_ref(&**image).cast(),
                    std::ptr::null(),
                );
                CGImageDestinationFinalize(d) != 0
            },
            _ => false,
        };
        if let Some(d) = dest {
            cf_release(d.cast());
        }
        if !url.is_null() {
            cf_release(url);
        }
        cf_release(path_cf);
        result
    }

    use std::path::Path;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_frames_reference_source_file() {
        let dir = std::env::temp_dir().join(format!("gesso-direct-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.png"), b"P").unwrap();
        // Direct：直接引用源文件，即便没有 thumb.png 也有帧
        let frames = preview_frames(&dir.display().to_string(), WallpaperKind::Image);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].ends_with("index.png"));
        // 非 Direct 类型不读 index.png（它要 thumb.png 序列）
        assert!(preview_frames(&dir.display().to_string(), WallpaperKind::Video).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn direct_picks_first_index_image_ext() {
        let dir = std::env::temp_dir().join(format!("gesso-direct-jpg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.jpg"), b"J").unwrap();
        let frames = preview_frames(&dir.display().to_string(), WallpaperKind::Image);
        assert!(frames[0].ends_with("index.jpg"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn frame_times_long_video_is_exact_grid() {
        let times = frame_times(10.0);
        assert_eq!(times.len(), HOVER_FRAMES + 1);
        assert_eq!(times[0], 0.0);
        // 未被钳制：等差 1/8s，最末帧 1.875 < 10 - 0.05
        for (i, w) in times.windows(2).enumerate() {
            assert!(
                (w[1] - w[0] - 0.125).abs() < 1e-9,
                "第 {i} 帧间隔不等于 125ms"
            );
        }
        assert!(*times.last().unwrap() < 10.0);
    }

    #[test]
    fn frame_times_short_video_clamps_inside_clip() {
        let times = frame_times(0.5);
        assert_eq!(times.len(), HOVER_FRAMES + 1);
        assert!(
            times.iter().all(|t| *t >= 0.0 && *t < 0.5),
            "越界时刻 {times:?}"
        );
        // 单调不减（短视频后段钳在同一时刻）
        assert!(times.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn frame_times_just_over_minimum() {
        // 下限保护：duration ≤ 0.1 在调用侧被拦，这里只验证边界之上不产生负值
        let times = frame_times(0.11);
        assert!(times.iter().all(|t| *t >= 0.0));
    }

    /// 真机回归：内置样例走完整 AVFoundation + ImageIO 管线。
    /// v1 的两处所有权错误在脚本化跑这条路径时会让进程直接崩——测试绿 = 崩溃修复。
    #[cfg(target_os = "macos")]
    #[test]
    fn extract_frames_on_real_sample() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/samples/testsrc.mp4");
        if !src.exists() {
            eprintln!("样例缺失，跳过");
            return;
        }
        let dir = std::env::temp_dir().join(format!("gesso-thumb-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(&src, dir.join("index.mp4")).unwrap();

        let outcome = extract_frames(&dir.display().to_string());

        assert!(outcome.written >= 1, "应至少产出首帧");
        let frames = existing_frames(&dir.display().to_string());
        assert_eq!(frames.len(), HOVER_FRAMES + 1, "12s 样例应产出完整 16 帧");
        assert!(dir.join("thumb.png").metadata().unwrap().len() > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
