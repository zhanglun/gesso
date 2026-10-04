//! macOS 桥实现：CGWindowList 全屏检测 + IOKit 电源状态。
//!
//! 两条纪律（工程笔记 §2）：
//! - 纯 C API 手写 extern（CF 的 +1/CFRelease 对称即可），不裸碰 ObjC 引用计数；
//!   IOPSCopyPowerSourcesInfo 返回 +1（要 Release），GetPowerSourceList 与
//!   Description 都是借用（随 info 释放，不得重复 Release）。
//! - 全屏判定用启发式：layer 0 窗口覆盖整块显示器 frame ≈ 全屏应用
//!   （菜单栏 layer 24 / Dock layer 20 天然被过滤；壁纸窗口是
//!   layer -2147483604 也被过滤）。

use std::collections::BTreeSet;
use std::os::raw::{c_char, c_double, c_void};

use crate::pin;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct CGRect {
    x: c_double,
    y: c_double,
    w: c_double,
    h: c_double,
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(arr: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(arr: *const c_void, idx: isize) -> *const c_void;
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        c_str: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
    fn CFNumberGetValue(num: *const c_void, the_type: isize, value: *mut c_void) -> u8;
    fn CFStringGetCString(
        s: *const c_void,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> u8;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative: u32) -> *const c_void;
    fn CGRectMakeWithDictionaryRepresentation(dict: *const c_void, rect: *mut CGRect) -> u8;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> *const c_void; // +1
    // 新 SDK 只导出 Copy 变体（+1 要 Release）；老的手写 extern 用的是
    // 借用式 IOPSGetPowerSourceList——本机 SDK（15.4）tbd 里已无此符号
    fn IOPSCopyPowerSourcesList(info: *const c_void) -> *const c_void; // +1
    fn IOPSGetPowerSourceDescription(list: *const c_void, ps: *const c_void) -> *const c_void; // 借用
}

const K_CF_STRING_ENCODING: u32 = 0x08000100; // kCFStringEncodingUTF8
const K_CF_NUMBER_DOUBLE_TYPE: isize = 6;

fn cfstr(s: &str) -> *const c_void {
    let c = std::ffi::CString::new(s).expect("无内嵌 NUL 的键名");
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING) }
}

fn dict_str(dict: *const c_void, key: &str) -> Option<String> {
    unsafe {
        let v = CFDictionaryGetValue(dict, cfstr(key));
        if v.is_null() {
            return None;
        }
        let mut buf = [0 as c_char; 128];
        if CFStringGetCString(v, buf.as_mut_ptr(), 128, K_CF_STRING_ENCODING) != 0 {
            Some(
                std::ffi::CStr::from_ptr(buf.as_ptr())
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        }
    }
}

fn dict_f64(dict: *const c_void, key: &str) -> Option<f64> {
    unsafe {
        let v = CFDictionaryGetValue(dict, cfstr(key));
        if v.is_null() {
            return None;
        }
        let mut out = 0f64;
        if CFNumberGetValue(v, K_CF_NUMBER_DOUBLE_TYPE, &mut out as *mut f64 as *mut c_void) != 0 {
            Some(out)
        } else {
            None
        }
    }
}

/// 正被全屏应用覆盖的显示器 ID（`cg-<id>`，与 MonitorInfo.id 同源）。
///
/// 判定：屏幕上 layer 0 的窗口 bounds 与某显示器 frame 完全重合（±3pt 容差）。
/// CG 坐标是全局"顶左原点"，MonitorInfo.frame 是 AppKit"底左原点"——
/// y 轴经主显示器高度翻转后比较。
pub fn fullscreen_displays() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let monitors = pin::macos::enumerate_monitors();
    if monitors.is_empty() {
        return out;
    }
    // 主显示器 = 全局坐标系翻转基准（AppKit 原点在其底左）
    let primary_h = monitors
        .iter()
        .find(|m| m.is_main)
        .or_else(|| monitors.first())
        .map(|m| m.frame.3)
        .unwrap_or(0.0);

    unsafe {
        // kCGWindowListOptionOnScreenOnly = 1, kCGNullWindowID = 0
        let list = CGWindowListCopyWindowInfo(1, 0);
        if list.is_null() {
            return out;
        }
        let n = CFArrayGetCount(list);
        for i in 0..n {
            let d = CFArrayGetValueAtIndex(list, i);
            if d.is_null() {
                continue;
            }
            // 只看普通层窗口（壁纸 -2147483604 / Dock 20 / 菜单栏 24 天然排除）
            let layer = dict_f64(d, "kCGWindowLayer").unwrap_or(f64::NAN);
            if layer != 0.0 {
                continue;
            }
            let mut rect = CGRect::default();
            let b = CFDictionaryGetValue(d, cfstr("kCGWindowBounds"));
            if b.is_null() || CGRectMakeWithDictionaryRepresentation(b, &mut rect) == 0 {
                continue;
            }
            if rect.w < 100.0 || rect.h < 100.0 {
                continue; // 排除悬浮小窗的巧合重合
            }
            for m in &monitors {
                let (mx, my, mw, mh) = m.frame;
                // CG 顶左 y → AppKit 底左 y
                let y_appkit = primary_h - rect.y - rect.h;
                let hit = (rect.w - mw).abs() < 3.0
                    && (rect.h - mh).abs() < 3.0
                    && (rect.x - mx).abs() < 3.0
                    && (y_appkit - my).abs() < 3.0;
                if hit {
                    out.insert(m.id.clone());
                }
            }
        }
        CFRelease(list);
    }
    out
}

/// 电源态：Some(true)=电池供电。台式机（无电池源）返回 None。
pub fn on_battery() -> Option<bool> {
    unsafe {
        let info = IOPSCopyPowerSourcesInfo();
        if info.is_null() {
            return None;
        }
        let list = IOPSCopyPowerSourcesList(info);
        let mut result = None;
        if !list.is_null() {
            let n = CFArrayGetCount(list);
            for i in 0..n {
                let ps = CFArrayGetValueAtIndex(list, i);
                if ps.is_null() {
                    continue;
                }
                let desc = IOPSGetPowerSourceDescription(list, ps);
                if desc.is_null() {
                    continue;
                }
                match dict_str(desc, "Power Source State").as_deref() {
                    // kIOPSBatteryPowerValue / kIOPSACPowerValue
                    Some("Battery Power") => {
                        result = Some(true);
                        break;
                    }
                    Some("AC Power") => {
                        result = Some(false);
                        break;
                    }
                    _ => {}
                }
            }
        }
        if !list.is_null() {
            CFRelease(list); // Copy 变体 +1
        }
        CFRelease(info); // desc 借用自 info，随它一起失效
        result
    }
}
