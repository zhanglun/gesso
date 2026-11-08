//! Windows 缩略图采集：webview 抽帧，输出契约与 macOS 完全一致
//! （`thumb.png + thumb-N.png` 连续序列 + `.tmp` → rename 原子写）。
//!
//! 与 macOS 的平台面差异（收敛目标见 ROADMAP）：
//! - 快照 = PrintWindow(PW_RENDERFULLCONTENT)，兜底 BitBlt 屏幕区（采集窗
//!   TOPMOST 恒不被遮挡，屏幕抓取可靠）；WKWebView takeSnapshot 无 Windows 对应物
//! - 视频 = 影子 `<video crossorigin=anonymous>` + canvas 绘制（协议响应已带
//!   `Access-Control-Allow-Origin: *`，规避 canvas 跨源污染）；macOS 走 AVFoundation
//! - shader = `preserveDrawingBuffer` 下的 `#gl.toDataURL`（宿主页已开启；
//!   WebGL 默认帧缓冲合成后即清空，跨任务读取是黑帧——工程笔记 #41）
//!
//! 线程与生命周期纪律（同 capture.rs 模块头）：主线程（GPUI 前台执行器）、
//! 进程级串行（CAPTURE_QUEUE）。采集窗口进程级复用（重建 webview 昂贵）：
//! **采集期显身、空闲期隐藏**——隐藏窗口不参与命中测试、不被 DWM 合成，
//! 屏幕上不再有常驻的近隐形顶层层；显身后 Chromium 恢复渲染，首帧延迟由
//! 现有就绪轮询（READY_TIMEOUT / FRAME_SETTLE）吸收。永不 close（快照/求值
//! 悬挂期绝不能销毁窗口）。窗口为顶层 TOPMOST，不受 explorer 重启影响，无需重钉。

use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetWindowRect, ShowWindow, PW_RENDERFULLCONTENT, SW_HIDE, SW_SHOWNA,
};

use gesso_core::WallpaperKind;

use crate::capture::{CAP_H, CAP_W, FRAME_SETTLE, READY_TIMEOUT, SHOT_TIMEOUT};
use crate::thumb::{self, HOVER_FRAMES, SAMPLE_FPS};

// 采集分辨率/节奏参数与 macOS capture.rs 同源（单一契约）：
// CAP_W/CAP_H（逻辑 640×360）、READY_TIMEOUT、FRAME_SETTLE、SHOT_TIMEOUT

/// 采集一个条目的帧序列（串行队列消费；调用方已过 ThumbScheduler 去重）。
/// 返回成功写盘的帧数；完成经调用方 `ThumbsDone` 回灌 UI。
pub(crate) async fn capture_entry(
    bg: gpui_kit::gpui::BackgroundExecutor,
    url: String,
    dir: String,
    kind: WallpaperKind,
) -> usize {
    let cap = take_window();
    let written = match kind {
        WallpaperKind::Video => video_frames(&cap, &bg, &url, &dir).await,
        WallpaperKind::Shader => shader_frames(&cap, &bg, &url, &dir).await,
        WallpaperKind::Html => html_frames(&cap, &bg, &url, &dir).await,
        _ => 0,
    };
    put_window(cap);
    written
}

// ---------------------------------------------------------------------------
// 采集窗口
// ---------------------------------------------------------------------------

struct CaptureWindow {
    hwnd: HWND,
    webview: lb_wry::WebView,
}

thread_local! {
    /// 采集窗口：主线程独占；进程级常驻（纪律：快照/求值悬挂期绝不 close）
    static CAP_WINDOW: RefCell<Option<CaptureWindow>> = const { RefCell::new(None) };
}

/// wry 直挂所需的句柄包装（仅主线程使用）
struct HwndWrap(HWND);
// SAFETY: 指针仅在该句柄传给 wry（主线程）期间解引用
impl raw_window_handle::HasWindowHandle for HwndWrap {
    fn window_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError>
    {
        let hwnd = std::num::NonZero::<isize>::new(self.0 .0 as isize).expect("采集窗口 HWND 非空");
        let handle = raw_window_handle::Win32WindowHandle::new(hwnd);
        Ok(unsafe {
            raw_window_handle::WindowHandle::borrow_raw(raw_window_handle::RawWindowHandle::Win32(
                handle,
            ))
        })
    }
}

fn take_window() -> CaptureWindow {
    CAP_WINDOW.with(|c| {
        if let Some(w) = c.borrow_mut().take() {
            // 空闲期窗口被隐藏（put_window），采集前重新显身（不抢焦点）
            // SAFETY: 同主线程创建的窗口
            unsafe {
                let _ = ShowWindow(w.hwnd, SW_SHOWNA);
            }
            return w;
        }
        let hwnd = crate::pin::windows::create_overlay_window((CAP_W as i32, CAP_H as i32))
            .expect("采集窗口创建失败");
        let webview = crate::protocol::create_webview(HwndWrap(hwnd), "about:blank")
            .expect("采集 webview 创建失败");
        // 物理像素铺满（窗口尺寸已按系统 DPI 缩放）
        let mut r = RECT::default();
        let _ = unsafe { GetWindowRect(hwnd, &mut r) };
        let _ = webview.set_bounds(lb_wry::Rect {
            size: lb_wry::dpi::Size::Physical(lb_wry::dpi::PhysicalSize::new(
                (r.right - r.left) as u32,
                (r.bottom - r.top) as u32,
            )),
            position: lb_wry::dpi::Position::Physical(lb_wry::dpi::PhysicalPosition::new(0, 0)),
        });
        println!(
            "[thumbs] 采集窗口就绪 {}×{}（物理像素）",
            r.right - r.left,
            r.bottom - r.top
        );
        CaptureWindow { hwnd, webview }
    })
}

fn put_window(w: CaptureWindow) {
    // 熄火（对齐 macOS capture）：导航回 about:blank 再隐藏——隐藏 ≠ 停播，
    // WebView2 隐藏窗里的 <video> 仍在解码；导航才是确定性的断电点。
    // 下次采集 take_window 显身后由各 frames 路径重新加载目标 URL
    let _ = w.webview.load_url("about:blank");
    // 空闲即隐藏：屏幕上不常驻近隐形顶层窗（隐藏窗不参与命中测试与合成）。
    // SAFETY: 同主线程创建的窗口；webview 保留，下次采集显身即复用
    unsafe {
        let _ = ShowWindow(w.hwnd, SW_HIDE);
    }
    CAP_WINDOW.with(|c| *c.borrow_mut() = Some(w));
}

// ---------------------------------------------------------------------------
// JS 通道
// ---------------------------------------------------------------------------

/// 带回调求值（结果经共享 cell 回传，定时器轮询收割——与 macOS snapshot 同款）
async fn eval_cb(
    cap: &CaptureWindow,
    bg: &gpui_kit::gpui::BackgroundExecutor,
    js: &str,
) -> Option<String> {
    let cell: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    {
        let cell = cell.clone();
        let _ = cap
            .webview
            .evaluate_script_with_callback(js, move |v: String| {
                // ExecuteScript 结果是 JSON 编码：字符串带引号（"12.0"）、布尔裸值
                // （true）。统一去引号，调用方按裸字符串比较/解析。base64/dataURL
                // 载荷无转义字符，不必上完整 JSON 反序列化。
                let t = v.trim();
                let unquoted = if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
                    t[1..t.len() - 1]
                        .replace("\\\"", "\"")
                        .replace("\\\\", "\\")
                } else {
                    t.to_string()
                };
                *cell.lock().unwrap() = Some(unquoted);
            });
    }
    let deadline = Instant::now() + SHOT_TIMEOUT;
    loop {
        if let Some(v) = cell.lock().unwrap().take() {
            return Some(v);
        }
        if Instant::now() > deadline {
            return None;
        }
        bg.timer(Duration::from_millis(15)).await;
    }
}

/// 轮询一个 JS 布尔表达式直到为真（页面就绪 / seek 完成）
async fn poll_true(
    cap: &CaptureWindow,
    bg: &gpui_kit::gpui::BackgroundExecutor,
    js: &str,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if eval_cb(cap, bg, js).await.as_deref() == Some("true") {
            return true;
        }
        bg.timer(Duration::from_millis(120)).await;
    }
    false
}

/// dataURL → PNG 原子落盘（`.tmp` → rename；契约与 macOS thumb.rs v2 一致）。
/// ExecuteScript 的字符串结果经 JSON 编码（带引号），base64/dataURL 前缀
/// 无转义字符，trim 引号即安全。任意 image/* dataURL（png/jpeg 通用）。
fn write_dataurl(dataurl: &str, out: &Path) -> bool {
    let payload = dataurl.trim().trim_matches('"');
    let b64 = payload
        .split_once(";base64,")
        .map(|(_, b)| b)
        .unwrap_or(payload);
    let Some(bytes) = crate::encoding::base64_decode(b64) else {
        println!("[thumbs] dataURL 解码失败");
        return false;
    };
    let tmp = out.with_extension("tmp");
    let ok = std::fs::write(&tmp, &bytes).is_ok() && std::fs::rename(&tmp, out).is_ok();
    if !ok {
        let _ = std::fs::remove_file(&tmp);
    }
    ok
}

// ---------------------------------------------------------------------------
// 视频抽帧（影子 video + canvas）
// ---------------------------------------------------------------------------

/// 采集助手：影子 `<video crossorigin=anonymous>`（协议响应带 ACAO:*，canvas
/// 不被污染）+ cover-fit 绘制画布；不触碰宿主页自己的播放元素。
/// 画布尺寸与输出格式用占位符注入：缩略图 640×360 PNG，封面帧按目标分辨率 JPEG。
const CAP_SETUP_JS: &str = r##"window.__cap = (function () {
  if (window.__cap) return window.__cap;
  let spec = null;
  try { spec = JSON.parse(new URLSearchParams(location.search).get("spec") || "null"); } catch (e) {}
  const v = document.createElement("video");
  v.crossOrigin = "anonymous"; v.muted = true; v.playsInline = true; v.preload = "auto";
  if (spec && spec.source) v.src = spec.source;
  const cvs = document.createElement("canvas"); cvs.width = __W__; cvs.height = __H__;
  // willReadFrequently 强制软件画布：GPU 画布上 drawImage(视频) 后 toDataURL
  // 会回读到黑缓冲（New Zealand 实测：getImageData 有内容、toDataURL 全黑），
  // 软件画布让绘制/读取/编码走同一条内存路径
  return {
    v: v, cvs: cvs, ctx: cvs.getContext("2d", { willReadFrequently: true }),
    meta: function () {
      return (v.readyState >= 2 && isFinite(v.duration) && v.duration > 0.1) ? String(v.duration) : "";
    },
    seek: function (t) { v.currentTime = t; },
    settled: function () { return (!v.seeking && v.readyState >= 2) ? "true" : "false"; },
    grab: function () {
      const s = Math.max(__W__ / (v.videoWidth || __W__), __H__ / (v.videoHeight || __H__));
      const dw = (v.videoWidth || __W__) * s, dh = (v.videoHeight || __H__) * s;
      this.ctx.fillStyle = "#000"; this.ctx.fillRect(0, 0, __W__, __H__);
      this.ctx.drawImage(v, (__W__ - dw) / 2, (__H__ - dh) / 2, dw, dh);
      // 全黑检测：drawImage 没渲染出来时画布只剩黑色底填充——采样判定，
      // 全黑返回标记（Rust 侧判失败重试），黑帧不再被固化（New Zealand 实测）
      // 只采 RGB（alpha 恒 255，混通道采样会把全黑误判成有内容——New Zealand 实测）
      const d = this.ctx.getImageData(0, 0, __W__, __H__).data;
      let mn = 255, mx = 0;
      for (let i = 0; i < d.length; i += 16) {
        const r = d[i], g = d[i + 1], b = d[i + 2];
        if (r < mn) mn = r; if (r > mx) mx = r;
        if (g < mn) mn = g; if (g > mx) mx = g;
        if (b < mn) mn = b; if (b > mx) mx = b;
      }
      // 全黑 = 视频帧未呈现（高分辨率视频 seek 后解码滞后）：Rust 侧重定位重抓
      if (mx - mn <= 8) return "BLACK:" + mn + "-" + mx;
      return "OK|" + cvs.toDataURL("__MIME____Q__");
    }
  };
})(); 'ok'"##;

fn cap_setup_js(w: u32, h: u32, mime: &str, quality: &str) -> String {
    CAP_SETUP_JS
        .replace("__W__", &w.to_string())
        .replace("__H__", &h.to_string())
        .replace("__MIME__", mime)
        .replace("__Q__", quality)
}

async fn video_frames(
    cap: &CaptureWindow,
    bg: &gpui_kit::gpui::BackgroundExecutor,
    url: &str,
    dir: &str,
) -> usize {
    let _ = cap
        .webview
        .load_url(&crate::pin::windows::workaround_url(url));
    if eval_cb(cap, bg, &cap_setup_js(640, 360, "image/png", ""))
        .await
        .is_none()
    {
        println!("[thumbs] 视频采集助手注入失败：{dir}");
        return 0;
    }
    // 等 loadeddata + duration（坏文件超时放弃，ThumbScheduler 重试上限兜底）
    if !poll_true(
        cap,
        bg,
        "window.__cap && __cap.meta() !== ''",
        READY_TIMEOUT,
    )
    .await
    {
        println!("[thumbs] 视频未就绪（解码失败或超时）：{dir}");
        return 0;
    }
    let duration: f64 = eval_cb(cap, bg, "__cap.meta()")
        .await
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0.0);
    if !(duration.is_finite() && duration > 0.1) {
        println!("[thumbs] 视频时长异常（{duration}）：{dir}");
        return 0;
    }
    let times = thumb::frame_times(duration);
    let mut written = 0usize;
    for (i, t) in times.into_iter().enumerate() {
        let out = thumb::thumb_path(Path::new(dir), i);
        if out.exists() {
            continue;
        }
        // 高分辨率视频 seek 后帧呈现滞后（8K 实测）：全黑则重定位重抓，
        // 最多 4 次（首次 140ms，其后 500ms 逐次等待解码跟上）
        for attempt in 0..4u32 {
            let _ = eval_cb(cap, bg, &format!("__cap.seek({t}); 'ok'")).await;
            let settle = if attempt == 0 {
                FRAME_SETTLE
            } else {
                Duration::from_millis(500)
            };
            bg.timer(settle).await;
            if !poll_true(cap, bg, "__cap.settled()", Duration::from_secs(2)).await {
                println!("[thumbs] 第 {i} 帧 seek 超时，跳过");
                break;
            }
            let Some(data) = eval_cb(cap, bg, "__cap.grab()").await else {
                continue;
            };
            let (tag, payload) = match data.split_once('|') {
                Some((t2, u)) => (t2.to_string(), u.to_string()),
                None => (data.clone(), data),
            };
            if tag.starts_with("BLACK") {
                println!(
                    "[thumbs] 第 {i} 帧全黑（第 {} 次尝试），帧未呈现，重试",
                    attempt + 1
                );
                continue;
            }
            if write_dataurl(&payload, &out) {
                written += 1;
            }
            break;
        }
    }
    written
}

// ---------------------------------------------------------------------------
// shader 定格（preserveDrawingBuffer 下 toDataURL）
// ---------------------------------------------------------------------------

async fn shader_frames(
    cap: &CaptureWindow,
    bg: &gpui_kit::gpui::BackgroundExecutor,
    url: &str,
    dir: &str,
) -> usize {
    let _ = cap
        .webview
        .load_url(&crate::pin::windows::workaround_url(url));
    if !poll_true(cap, bg, "window.__gessoReady === true", READY_TIMEOUT).await {
        println!("[thumbs] shader 页面未就绪（编译失败或加载超时）：{dir}");
        return 0;
    }
    bg.timer(Duration::from_millis(150)).await;
    let mut written = 0usize;
    for i in 0..=HOVER_FRAMES {
        let t = i as f64 / SAMPLE_FPS as f64;
        let _ = eval_cb(
            cap,
            bg,
            &format!("window.__gessoSeek && window.__gessoSeek({t}); 'ok'"),
        )
        .await;
        bg.timer(FRAME_SETTLE).await;
        let out = thumb::thumb_path(Path::new(dir), i);
        if out.exists() {
            continue;
        }
        let Some(data) = eval_cb(
            cap,
            bg,
            "document.getElementById('gl').toDataURL('image/png')",
        )
        .await
        else {
            continue;
        };
        if write_dataurl(&data, &out) {
            written += 1;
        }
    }
    written
}

// ---------------------------------------------------------------------------
// html 实时帧（PrintWindow——唯一平台接缝）
// ---------------------------------------------------------------------------

async fn html_frames(
    cap: &CaptureWindow,
    bg: &gpui_kit::gpui::BackgroundExecutor,
    url: &str,
    dir: &str,
) -> usize {
    let _ = cap
        .webview
        .load_url(&crate::pin::windows::workaround_url(url));
    if !poll_true(cap, bg, "window.__gessoReady === true", READY_TIMEOUT).await {
        println!("[thumbs] html 页面未就绪：{dir}");
        return 0;
    }
    bg.timer(Duration::from_millis(300)).await;
    let mut written = 0usize;
    for i in 0..=HOVER_FRAMES {
        let out = thumb::thumb_path(Path::new(dir), i);
        if out.exists() {
            continue;
        }
        bg.timer(FRAME_SETTLE).await;
        let Some((rgba, w, h)) = screenshot_window(cap.hwnd) else {
            continue;
        };
        if encode_png_rgba(&rgba, w as u32, h as u32, &out) {
            written += 1;
        }
    }
    written
}

/// 截采集窗口：PrintWindow(PW_RENDERFULLCONTENT) 直出窗口自身合成内容
/// （被遮挡也有效）；返回失败再走 BitBlt 屏幕区兜底（TOPMOST 恒不被遮挡）。
/// 锁屏快照（§4.8）复用同一函数抓活的壁纸窗（原生分辨率）。
pub(crate) fn screenshot_window(hwnd: HWND) -> Option<(Vec<u8>, i32, i32)> {
    unsafe {
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r).ok()?;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        if w <= 0 || h <= 0 {
            return None;
        }
        let hdc_screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(hdc_screen));
        let bmp = CreateCompatibleBitmap(hdc_screen, w, h);
        let old = SelectObject(mem, bmp.into());

        let mut pixels = None;
        if PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool() {
            pixels = dib_pixels(mem, bmp, w, h);
        }
        if pixels.is_none()
            && BitBlt(mem, 0, 0, w, h, Some(hdc_screen), r.left, r.top, SRCCOPY).is_ok()
        {
            pixels = dib_pixels(mem, bmp, w, h);
        }

        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, hdc_screen);
        pixels.map(|p| (p, w, h))
    }
}

/// 内存 DC 的 32bpp DIB 读出（负高 = top-down）→ BGRA 转 RGBA
unsafe fn dib_pixels(
    mem: windows::Win32::Graphics::Gdi::HDC,
    bmp: windows::Win32::Graphics::Gdi::HBITMAP,
    w: i32,
    h: i32,
) -> Option<Vec<u8>> {
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: windows::Win32::Graphics::Gdi::BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let n = GetDIBits(
        mem,
        bmp,
        0,
        h as u32,
        Some(buf.as_mut_ptr().cast()),
        &mut bmi,
        DIB_RGB_COLORS,
    );
    if n == 0 {
        return None;
    }
    for px in buf.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    Some(buf)
}

/// RGBA → PNG 原子落盘（png crate 编码；托盘解码同依赖，零新增）。
/// 锁屏快照（§4.8）复用同一编码器。
pub(crate) fn encode_png_rgba(rgba: &[u8], w: u32, h: u32, out: &Path) -> bool {
    let tmp = out.with_extension("png.tmp");
    let encoded: std::io::Result<()> = (|| {
        let f = std::fs::File::create(&tmp)?;
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header()?;
        wr.write_image_data(rgba)?;
        wr.finish()?;
        Ok(())
    })();
    let ok = encoded.is_ok() && std::fs::rename(&tmp, out).is_ok();
    if !ok {
        let _ = std::fs::remove_file(&tmp);
    }
    ok
}

// ---------------------------------------------------------------------------
// 封面帧（§4.8）：入库时按主屏分辨率产一张固定帧，锁屏即指即得
// ---------------------------------------------------------------------------

/// 按目标物理尺寸产封面帧：video/shader 走画布 JPEG（canvas 原生编码，快），
/// html/image/gif 走 PrintWindow PNG。窗口一次性（建→采→毁，导入低频）；
/// TOPMOST + alpha 3/255，采集期近隐形不抢焦点。
pub(crate) async fn capture_poster(
    bg: &gpui_kit::gpui::BackgroundExecutor,
    url: String,
    kind: WallpaperKind,
    out: &Path,
    physical: (i32, i32),
) -> bool {
    let hwnd =
        crate::pin::windows::create_overlay_window_physical(physical).expect("封面窗口创建失败");
    let webview = crate::protocol::create_webview(HwndWrap(hwnd), "about:blank")
        .expect("封面 webview 创建失败");
    let _ = webview.set_bounds(lb_wry::Rect {
        size: lb_wry::dpi::Size::Physical(lb_wry::dpi::PhysicalSize::new(
            physical.0 as u32,
            physical.1 as u32,
        )),
        position: lb_wry::dpi::Position::Physical(lb_wry::dpi::PhysicalPosition::new(0, 0)),
    });
    // SAFETY: 同主线程刚创建的窗口
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNA);
    }
    let cap = CaptureWindow { hwnd, webview };
    let ok = match kind {
        WallpaperKind::Video => poster_video(&cap, bg, &url, out, physical).await,
        WallpaperKind::Shader => poster_shader(&cap, bg, &url, out).await,
        _ => poster_print(&cap, bg, &url, out).await,
    };
    drop(cap);
    // SAFETY: 本线程自建自毁，采集已全部完成
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    ok
}

/// 视频：定位到 10% 处（避开常见黑首帧）抓一帧画布 JPEG。
async fn poster_video(
    cap: &CaptureWindow,
    bg: &gpui_kit::gpui::BackgroundExecutor,
    url: &str,
    out: &Path,
    physical: (i32, i32),
) -> bool {
    let _ = cap
        .webview
        .load_url(&crate::pin::windows::workaround_url(url));
    let js = cap_setup_js(
        physical.0.max(1) as u32,
        physical.1.max(1) as u32,
        "image/jpeg",
        ",0.9",
    );
    if eval_cb(cap, bg, &js).await.is_none() {
        println!("[poster] 视频采集助手注入失败");
        return false;
    }
    if !poll_true(
        cap,
        bg,
        "window.__cap && __cap.meta() !== ''",
        READY_TIMEOUT,
    )
    .await
    {
        println!("[poster] 视频未就绪（解码失败或超时）");
        return false;
    }
    let duration: f64 = eval_cb(cap, bg, "__cap.meta()")
        .await
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0.0);
    let t = if duration > 1.0 { duration * 0.1 } else { 0.0 };
    for attempt in 0..4u32 {
        let _ = eval_cb(cap, bg, &format!("__cap.seek({t}); 'ok'")).await;
        let settle = if attempt == 0 {
            FRAME_SETTLE
        } else {
            Duration::from_millis(500)
        };
        bg.timer(settle).await;
        if !poll_true(cap, bg, "__cap.settled()", Duration::from_secs(2)).await {
            println!("[poster] seek 超时");
            return false;
        }
        let Some(data) = eval_cb(cap, bg, "__cap.grab()").await else {
            continue;
        };
        let (tag, payload) = match data.split_once('|') {
            Some((t2, u)) => (t2.to_string(), u.to_string()),
            None => (data.clone(), data),
        };
        if tag.starts_with("BLACK") {
            println!(
                "[poster] 全黑帧（第 {} 次尝试），帧未呈现，重试",
                attempt + 1
            );
            continue;
        }
        return write_dataurl(&payload, out);
    }
    println!("[poster] 4 次尝试均未呈现有效帧");
    false
}

/// shader：宿主页就绪后直接 toDataURL JPEG。
async fn poster_shader(
    cap: &CaptureWindow,
    bg: &gpui_kit::gpui::BackgroundExecutor,
    url: &str,
    out: &Path,
) -> bool {
    let _ = cap
        .webview
        .load_url(&crate::pin::windows::workaround_url(url));
    if !poll_true(cap, bg, "window.__gessoReady === true", READY_TIMEOUT).await {
        println!("[poster] shader 页面未就绪（编译失败或加载超时）");
        return false;
    }
    bg.timer(Duration::from_millis(300)).await;
    match eval_cb(
        cap,
        bg,
        "document.getElementById('gl').toDataURL('image/jpeg',0.9)",
    )
    .await
    {
        Some(data) => write_dataurl(&data, out),
        None => false,
    }
}

/// html / 图片 / gif：宿主页渲染完整内容后 PrintWindow 整窗成 PNG。
async fn poster_print(
    cap: &CaptureWindow,
    bg: &gpui_kit::gpui::BackgroundExecutor,
    url: &str,
    out: &Path,
) -> bool {
    let _ = cap
        .webview
        .load_url(&crate::pin::windows::workaround_url(url));
    if !poll_true(cap, bg, "window.__gessoReady === true", READY_TIMEOUT).await {
        println!("[poster] 页面未就绪");
        return false;
    }
    bg.timer(Duration::from_millis(500)).await;
    match screenshot_window(cap.hwnd) {
        Some((rgba, w, h)) => encode_png_rgba(&rgba, w as u32, h as u32, out),
        None => false,
    }
}
