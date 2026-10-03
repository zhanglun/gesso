//! 壁纸缩略图：视频抽帧（首帧 + 悬停预览帧序列）。
//!
//! 方案：AVFoundation `AVAssetImageGenerator`（objc2 裸调，无需 av-foundation 绑定 crate）。
//! - `thumb.png`        首帧（卡片静态预览）
//! - `thumb-1..7.png`   悬停轮播帧（均匀取 8 个时间点；hover 时 UI 以 ~8fps 轮播）
//! 全部在后台线程执行（文件 IO + 解码），UI 只读结果。
//! 失败静默（缺帧 → UI 回退静态首帧/渐变占位，预览失败不打扰用户）。

use std::path::{Path, PathBuf};

/// 悬停轮播帧数（不含首帧 thumb.png）。
pub const HOVER_FRAMES: usize = 7;

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
        use objc2::runtime::{AnyClass, AnyObject, Bool, NSObjectProtocol};
        use objc2_foundation::{NSURL, NSString};

        let url = NSURL::fileURLWithPath(&NSString::from_str(&src.display().to_string()));
        let asset_class = AnyClass::get("AVURLAsset").expect("AVFoundation 不可用");
        let asset: Retained<AnyObject> = msg_send![asset_class, alloc];
        let asset: Retained<AnyObject> = msg_send![asset, initWithURL: &*url options: std::ptr::null::<AnyObject>()];

        // duration = CMTimeGetSeconds([asset duration])
        let duration_cm: CMTime = msg_send![&asset, duration];
        let seconds: f64 = unsafe {
            extern "C" {
                fn CMTimeGetSeconds(time: CMTime) -> f64;
            }
            CMTimeGetSeconds(duration_cm)
        };
        if !(seconds.is_finite() && seconds > 0.1) {
            return existing_frames(source_dir);
        }

        let gen_class = AnyClass::get("AVAssetImageGenerator").expect("AVFoundation 不可用");
        let gen: Retained<AnyObject> = msg_send![gen_class, alloc];
        let gen: Retained<AnyObject> = msg_send![gen, initWithAsset: &*asset];
        // 帧精度 + 免裁剪原分辨率
        let _: () = msg_send![&gen, setRequestedTimeToleranceBefore: cm_time_zero()];
        let _: () = msg_send![&gen, setRequestedTimeToleranceAfter: cm_time_zero()];
        let _: () = msg_send![&gen, setAppliesPreferredTrackTransform: Bool::YES];

        let mut generated = Vec::new();
        let total = HOVER_FRAMES + 1; // 首帧 + 轮播帧
        for i in 0..total {
            let out_path = thumb_path(dir, i);
            if out_path.exists() {
                generated.push(out_path.display().to_string());
                continue;
            }
            // 取第 i 个采样点：t = duration * (i + 0.5) / total（避开首尾黑帧）
            let t = seconds * (i as f64 + 0.5) / total as f64;
            let cm = make_cm_time(t);
            let mut actual = cm;
            let cg: Retained<AnyObject> =
                msg_send![&gen, copyCGImageAtTime: cm actualTime: &mut actual error: std::ptr::null_mut::<AnyObject>()];
            if cg.is_null() {
                continue;
            }
            // CGImage → NSBitmapImageRep → PNG
            let rep_class = AnyClass::get("NSBitmapImageRep").expect("AppKit 不可用");
            let rep: Retained<AnyObject> = msg_send![rep_class, alloc];
            let rep: Retained<AnyObject> =
                msg_send![rep, initWithCGImage: &*cg];
            let png_data: Retained<AnyObject> =
                msg_send![&rep, representationUsingType: 4usize properties: std::ptr::null::<AnyObject>()]; // NSBitmapImageFileTypePNG = 4
            if png_data.writeToFileAtomically(
                &NSString::from_str(&out_path.display().to_string()),
                true,
            ) {
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
    const ENCODING: objc2::encode::Encoding =
        objc2::encode::Encoding::Struct("{CMTime=qiIq}", &[

            objc2::encode::Encoding::LongLong,
            objc2::encode::Encoding::Int,
            objc2::encode::Encoding::UInt,
            objc2::encode::Encoding::LongLong,
        ]);
}

unsafe impl objc2::encode::RefEncode for CMTime {
    const ENCODING_REF: objc2::encode::Encoding = objc2::encode::Encoding::Pointer(&<Self as objc2::encode::Encode>::ENCODING);
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
