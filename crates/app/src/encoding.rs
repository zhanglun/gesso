//! URL/主资源发现相关纯函数：base64url、主资源文件名。无会话状态、可独立单测。

use gesso_core::WallpaperKind;
use std::path::Path;

/// base64url（无填充；shader 源码经 URL 查询参数传递，§2 踩坑 #10）。
pub fn base64url(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(T[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(T[n as usize & 63] as char);
        }
    }
    out
}

/// 标准 base64 解码（`+/` 字母表，容忍缺省 padding）。
/// 缩略图采集用：浏览器 `toDataURL("image/png")` 的载荷即此编码。
/// 返回 None = 含非法字符或长度 %4 == 1。
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = s
        .bytes()
        .filter(|&b| !b.is_ascii_whitespace() && b != b'=')
        .collect();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n: u32 = 0;
        for (i, &b) in chunk.iter().enumerate() {
            n |= val(b)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// urlencode（查询参数值；`/` 也编码）
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 递归拷贝目录（跳过指定顶层文件名）；用于 WE web 条目整目录复制。
/// 导入落盘用的扩展名：**保留源文件扩展名**（WKWebView 按扩展名判定媒体类型，
/// 把 `.webm` 存成 `index.mp4`、`.webp` 存成 `index.gif` 会直接播不出来）；
/// 源文件无扩展名时退回类型默认名。
pub fn import_ext(path: &Path, kind: WallpaperKind) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| gesso_core::content_type(kind).default_ext.to_string())
}

/// 类型对应的默认扩展名（core 描述表的薄包装）。
pub fn default_ext(kind: WallpaperKind) -> &'static str {
    gesso_core::content_type(kind).default_ext
}

/// 该类型合法的源资源扩展名（core 描述表的薄包装）。
pub fn valid_exts(kind: WallpaperKind) -> &'static [&'static str] {
    gesso_core::content_type(kind).extensions
}

/// 条目主资源文件名：优先目录内真实存在的 `index.*`（导入保留源扩展名），
/// 找不到时回退到类型默认名。**导入 / 失效判定 / 宿主页 spec 三处必须共用本函数**，
/// 否则 webm/webp 这类条目会出现"能导入但被判失效"或"宿主页请求错文件名"。
///
/// Html 条目例外：主资源固定 `wallpaper.html`——条目里的 `index.html` 永远是
/// 宿主页，绝不能被"任意 index.*"回退命中成壁纸资源。
pub fn main_asset_name(source_dir: &str, kind: WallpaperKind) -> Option<String> {
    if kind == WallpaperKind::Html {
        let p = Path::new(source_dir).join("wallpaper.html");
        return p.is_file().then(|| "wallpaper.html".to_string());
    }
    let dir = Path::new(source_dir);
    let pref = std::fs::read_dir(dir).ok()?;
    let mut fallback: Option<String> = None;
    for ent in pref.flatten() {
        let name = ent.file_name().to_string_lossy().to_string();
        if let Some(rest) = name.strip_prefix("index.") {
            // 只认该类型合法扩展名（排除宿主页 index.html 等异名污染）
            let valid = valid_exts(kind).iter().any(|e| rest.eq_ignore_ascii_case(e));
            if valid {
                // 类型默认扩展名优先，其次任意合法 index.*
                if rest.eq_ignore_ascii_case(default_ext(kind)) {
                    return Some(name);
                }
                fallback.get_or_insert(name);
            }
        }
    }
    fallback
}

/// 条目主资源（相对条目根的路径）。
pub fn entry_main_source(entry: &gesso_core::LibraryEntry) -> String {
    // WE 零拷贝条目：主文件名来自 project.json（可能不是 index.* 命名）。
    if let Some(f) = &entry.main_file {
        return f.clone();
    }
    main_asset_name(&entry.source_dir, entry.kind).unwrap_or_else(|| match entry.kind {
        WallpaperKind::Html => "wallpaper.html".to_string(),
        k => format!("index.{}", default_ext(k)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decode_standard_alphabet() {
        assert_eq!(base64_decode("aGVsbG8="), Some(b"hello".to_vec()));
        // 缺省 padding 容忍
        assert_eq!(base64_decode("aGVsbG8"), Some(b"hello".to_vec()));
        // 标准 base64 的 62/63 是 +/（与 url 变体 -_ 区分）
        assert_eq!(base64_decode("+/8="), Some(vec![0xFB, 0xFF]));
        assert_eq!(base64_decode("A"), None); // 长度 %4 == 1
        assert_eq!(base64_decode("aGVs*bG8="), None); // 非法字符
    }
}
