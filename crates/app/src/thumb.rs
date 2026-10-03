//! 壁纸缩略图：视频抽帧（首帧 + 悬停预览帧序列）。
//!
//! 方案：AVFoundation `AVAssetImageGenerator`（objc2 裸调，无需 av-foundation 绑定 crate）。
//!
//! - `thumb.png`：首帧（卡片静态预览）
//! - `thumb-1..7.png`：悬停轮播帧（均匀取 8 个时间点；hover 时 UI 以 ~8fps 轮播）
//!
//! 全部在后台线程执行（文件 IO + 解码），UI 只读结果。
//! 失败静默（缺帧 → UI 回退静态首帧/渐变占位，预览失败不打扰用户）。

use std::path::{Path, PathBuf};

/// 悬停轮播：以【原速】回放视频开头一个短片段。
///
/// - `SAMPLE_FPS`：采样帧率 = 回放帧率（8fps 轮播即 8fps 采样 → 每帧驻留
///   125ms = 抽帧间隔，速度与原片一致）
/// - 片段时长 = HOVER_FRAMES / SAMPLE_FPS = 2 秒（循环点轻微跳变可接受）
pub const SAMPLE_FPS: u64 = 8;
pub const HOVER_FRAMES: usize = 15; // 2 秒片段（含首帧共 16 帧）

fn thumb_path(dir: &Path, idx: usize) -> PathBuf {
    if idx == 0 {
        dir.join("thumb.png")
    } else {
        dir.join(format!("thumb-{idx}.png"))
    }
}

/// 已存在的帧路径列表（index 0 = 首帧）。缓存齐全时直接返回。
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

/// 生成帧序列（后台线程调用；duration 由 AVAsset 元数据取）。
/// 返回生成的帧路径（含首帧）；失败返回空。
pub fn extract_frames(source_dir: &str) -> Vec<String> {
    let dir = Path::new(source_dir);
    let src = dir.join("index.mp4");
    if !src.exists() {
        return existing_frames(source_dir);
    }

    unsafe {
        use objc2::msg_send;
        use objc2::rc::Retained;
        use objc2::runtime::{Bool, NSObject};
        use objc2_foundation::{NSString, NSURL};

        let class = |name: &str| -> Option<&'static objc2::runtime::AnyClass> {
            let c = std::ffi::CString::new(name).ok()?;
            objc2::runtime::AnyClass::get(&c)
        };
        let url = NSURL::fileURLWithPath(&NSString::from_str(&src.display().to_string()));
        let Some(av_asset) = class("AVURLAsset") else {
            return existing_frames(source_dir);
        };
        let raw: *mut NSObject = msg_send![av_asset, alloc];
        let inited: *mut NSObject =
            msg_send![raw, initWithURL: &*url, options: std::ptr::null::<NSObject>()];
        let Some(asset) = Retained::from_raw(inited) else {
            return existing_frames(source_dir);
        };

        // duration = CMTimeGetSeconds([asset duration])
        let duration_cm: CMTime = msg_send![&asset, duration];
        let seconds: f64 = {
            extern "C" {
                fn CMTimeGetSeconds(time: CMTime) -> f64;
            }
            CMTimeGetSeconds(duration_cm)
        };
        if !(seconds.is_finite() && seconds > 0.1) {
            return existing_frames(source_dir);
        }

        let Some(gen_class) = class("AVAssetImageGenerator") else {
            return existing_frames(source_dir);
        };
        let gen_raw: *mut NSObject = msg_send![gen_class, alloc];
        let gen_inited: *mut NSObject = msg_send![gen_raw, initWithAsset: &*asset];
        let Some(gen) = Retained::from_raw(gen_inited) else {
            return existing_frames(source_dir);
        };
        // 帧精度 + 免裁剪原分辨率
        let _: () = msg_send![&gen, setRequestedTimeToleranceBefore: cm_time_zero()];
        let _: () = msg_send![&gen, setRequestedTimeToleranceAfter: cm_time_zero()];
        let _: () = msg_send![&gen, setAppliesPreferredTrackTransform: Bool::YES];

        let mut generated = Vec::new();
        let total = HOVER_FRAMES + 1; // 首帧 + 轮播帧
                                      // 采样上限：不足 2 秒的短视频按实际时长均分（保证帧间 ≥ 原速间隔）
        let clip_seconds = seconds.min(total as f64 / SAMPLE_FPS as f64);
        for i in 0..total {
            let out_path = thumb_path(dir, i);
            if out_path.exists() {
                generated.push(out_path.display().to_string());
                continue;
            }
            // 原速采样：帧间隔 = 1/SAMPLE_FPS（t_i = i / SAMPLE_FPS，截在片段内）
            let t = (i as f64 / SAMPLE_FPS as f64).min(clip_seconds - 0.05);
            let cm = make_cm_time(t);
            let mut actual = cm;
            let cg_raw: *mut NSObject = msg_send![&gen, copyCGImageAtTime: cm, actualTime: &mut actual, error: std::ptr::null_mut::<NSObject>()];
            if cg_raw.is_null() {
                continue;
            }
            let cg: Retained<NSObject> = Retained::from_raw(cg_raw).expect("copyCGImage 非空");
            // CGImage → NSBitmapImageRep → PNG
            let Some(rep_class) = class("NSBitmapImageRep") else {
                continue;
            };
            let rep_raw: *mut NSObject = msg_send![rep_class, alloc];
            let Some(rep) = Retained::from_raw(rep_raw) else {
                continue;
            };
            let rep_inited: *mut NSObject = msg_send![&rep, initWithCGImage: &*cg];
            let rep: Retained<NSObject> =
                (Retained::from_raw(rep_inited)).expect("initWithCGImage 非空");
            let png_raw: *mut NSObject = msg_send![&rep, representationUsingType: 4usize, properties: std::ptr::null::<NSObject>()]; // NSBitmapImageFileTypePNG = 4
            let Some(png_data) = Retained::from_raw(png_raw) else {
                continue;
            };
            let ok: Bool = msg_send![&png_data,
                writeToFile: &*NSString::from_str(&out_path.display().to_string()),
                atomically: true
            ];
            if ok.as_bool() {
                generated.push(out_path.display().to_string());
            }
        }
        generated
    }
}

// ---- CMTime（CoreMedia C 结构体；objc2 需手动 Encode 才能按值传给 objc 方法） ----

#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

unsafe impl objc2::encode::Encode for CMTime {
    const ENCODING: objc2::encode::Encoding = objc2::encode::Encoding::Struct(
        "{CMTime=qiIq}",
        &[
            objc2::encode::Encoding::LongLong,
            objc2::encode::Encoding::Int,
            objc2::encode::Encoding::UInt,
            objc2::encode::Encoding::LongLong,
        ],
    );
}

unsafe impl objc2::encode::RefEncode for CMTime {
    const ENCODING_REF: objc2::encode::Encoding =
        objc2::encode::Encoding::Pointer(&<Self as objc2::encode::Encode>::ENCODING);
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMTimeMake(value: i64, scale: i32) -> CMTime;
    fn CMTimeMakeWithSeconds(seconds: f64, preferred_timescale: i32) -> CMTime;
}

fn cm_time_zero() -> CMTime {
    unsafe { CMTimeMake(0, 600) }
}

fn make_cm_time(seconds: f64) -> CMTime {
    unsafe { CMTimeMakeWithSeconds(seconds, 600) }
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    // 显式链接，确保 AVURLAsset/AVAssetImageGenerator 类符号在进程内可用
}
