//! gesso:// 只读资源协议。
//!
//! 路由：
//!   gesso://host/<file>            → assets/host/（宿主页自身，单一副本）
//!   gesso://library/<entry>/<file> → <config>/library/<entry>/（白名单 = 随机 entry id）
//!   gesso://steam/<工坊相对路径>    → Steam 工坊目录（WE 零拷贝只读直引）
//!
//! 宿主页从 `gesso://host/index.html` 加载；ContentSpec.source 为指向
//! `gesso://library/<entry>/<主资源>` 的绝对 URL。html 条目整页在该基路径下，
//! 其相对资源引用天然落在条目目录内。
//! 安全：仅 GET、路径穿越防护、HTML 响应注入 CSP。白名单不依赖 origin（跨平台
//! origin 表示不同），靠路径规则 + 不可猜 entry id。

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use raw_window_handle::HasWindowHandle;

/// URI 路径百分号解码。webview 发来的 path 是编码态（空格=%20、非 ASCII=%XX），
/// 磁盘文件是未编码的。**必须在路径穿越校验之前解码**，否则 %2e%2e 可绕过校验。
fn decode_path(p: &str) -> String {
    percent_encoding::percent_decode_str(p)
        .decode_utf8_lossy()
        .into_owned()
}

/// 所有 gesso 资源同处一 scheme；CSP 按 scheme 收敛。
#[cfg(target_os = "macos")]
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' gesso:; \
     style-src 'unsafe-inline' gesso:; frame-src gesso:; \
     media-src gesso: blob:; img-src gesso: data:; connect-src 'none'";
/// Windows（WebView2 workaround，见 pin/windows.rs）：页面实际 origin 是
/// `http://gesso.<host段>`，CSP 源必须按 workaround 宿主枚举（`gesso:` 匹配不到它们）。
/// 协议回调收到的是还原后的 gesso:// URI，路由不分平台。
#[cfg(not(target_os = "macos"))]
const CSP: &str = "default-src 'none'; \
     script-src 'unsafe-inline' http://gesso.host http://gesso.library http://gesso.steam; \
     style-src 'unsafe-inline' http://gesso.host http://gesso.library http://gesso.steam; \
     frame-src http://gesso.host http://gesso.library http://gesso.steam; \
     media-src http://gesso.host http://gesso.library http://gesso.steam blob:; \
     img-src http://gesso.host http://gesso.library http://gesso.steam data:; connect-src 'none'";

/// 宿主页/样例资源根（开发态 = crate assets；发布态 = exe 旁 assets）
pub fn assets_dir() -> PathBuf {
    if let Some(dir) = option_env!("CARGO_MANIFEST_DIR") {
        return PathBuf::from(dir).join("assets");
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("assets")))
        .unwrap_or_else(|| PathBuf::from("assets"))
}

/// 配置目录：~/Library/Application Support/Gesso（win: ~/.gesso）
pub fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    #[cfg(target_os = "macos")]
    let base = format!("{home}/Library/Application Support/Gesso");
    #[cfg(not(target_os = "macos"))]
    let base = format!("{home}/.gesso");
    PathBuf::from(base)
}

/// 库根目录
pub fn library_dir() -> PathBuf {
    config_dir().join("library")
}

/// 条目资源的页面可见 URL。
/// - WE 零拷贝条目（origin == "wallpaper-engine"，source_dir 指向某 Steam 库）→ steam 路由
/// - 其余（库内拷贝）→ library 路由
/// `rel` 为该条目内的相对资源路径。
///
/// Windows 返回 WebView2 workaround 形态 `http://gesso.<host段>/…`（子资源不走
/// wry 的导航翻译，见 pin/windows.rs）；协议回调按还原后的 gesso:// URI 路由，
/// 因此本函数是页面子资源 URL 唯一的平台分派点。
pub fn entry_url(entry: &gesso_core::LibraryEntry, rel: &str) -> String {
    let gesso = if entry.origin == "wallpaper-engine" {
        let src = Path::new(&entry.source_dir);
        // 多库：找包含该 source_dir 的库，取相对该库的路径（URL 不带库标识，
        // route 侧再遍历全部库归属解析）。
        crate::we::find_libraries()
            .iter()
            .find(|l| src.starts_with(l))
            .and_then(|root| src.strip_prefix(root).ok())
            // Steam 不可用（已卸载/移动）→ 退回库内路径（旧拷贝条目兼容）
            .map(|inner| format!("gesso://steam/{}/{}", inner.to_string_lossy(), rel))
    } else {
        None
    };
    workaround_page_url(gesso.unwrap_or_else(|| format!("gesso://library/{}/{}", entry.id, rel)))
}

/// 页面可见 gesso URL → 平台导航形态。Windows 走 WebView2 workaround
/// （`gesso://X/` → `http://gesso.X/`），macOS 原样。
fn workaround_page_url(url: String) -> String {
    #[cfg(target_os = "windows")]
    return crate::pin::windows::workaround_url(&url);
    #[cfg(not(target_os = "windows"))]
    url
}

/// 宿主页入口 URL
pub fn host_url() -> &'static str {
    "gesso://host/index.html"
}

/// 创建挂到给定原生视图的壁纸 webview（注册 gesso:// 协议）。
pub fn create_webview<H: HasWindowHandle + 'static>(
    handle: H,
    url: &str,
) -> Result<lb_wry::WebView, String> {
    lb_wry::WebViewBuilder::new()
        .with_custom_protocol("gesso".into(), move |_id, req| route(req))
        .with_url(url)
        .build_as_child(&handle)
        .map_err(|e| format!("{e}"))
}

/// 处理一条 gesso:// 请求（协议回调）。
fn route(
    request: lb_wry::http::Request<Vec<u8>>,
) -> lb_wry::http::Response<Cow<'static, [u8]>> {
    use lb_wry::http::StatusCode;

    // URI 结构：gesso://<host 段>/<path>。host 段区分路由：
    //   gesso://host/…        → assets/host/（宿主页）
    //   gesso://library/<id>/<rel> → 库条目目录
    //   gesso://steam/<rel from steam root> → Steam 工坊（WE 只读直引）
    let uri = request.uri();
    let host = uri.host().map(|h| h.as_ref() as &str).unwrap_or_default();
    // 先解码再路由：entry id 是 ascii hex 不受影响，资源相对路径含空格/非 ASCII。
    let path = decode_path(uri.path().trim_start_matches('/'));
    let path = path.as_str();

    let file: PathBuf = if host == "host" {
        assets_dir().join("host").join(path)
    } else if host == "library" {
        // path = "<entry>/<相对资源>"
        let mut it = path.splitn(2, '/');
        let entry = it.next().unwrap_or_default();
        let tail = it.next().unwrap_or_default();
        // 路径穿越防护：条目内相对路径只允许 Normal 分量
        if Path::new(tail)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return err(StatusCode::FORBIDDEN, "路径非法");
        }
        library_dir().join(entry).join(tail)
    } else if host == "steam" {
        // 只允许 Normal 分量 + 限定工坊 431960 路径空间（防借路由读其他 Steam 文件）
        if Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return err(StatusCode::FORBIDDEN, "路径非法");
        }
        let allowed = Path::new("steamapps/workshop/content").join(crate::we::APPID);
        if !Path::new(path).starts_with(&allowed) {
            return err(StatusCode::FORBIDDEN, "超出工坊路径空间");
        }
        // 多库：path 是相对「该条目所属库」的路径，URL 不带库标识——遍历全部
        // 内容库，找包含该路径且文件存在的那个。安全检查（Normal + 工坊空间）
        // 已在上面完成，遍历只做归属解析。
        let libs = crate::we::find_libraries();
        if libs.is_empty() {
            return err(StatusCode::SERVICE_UNAVAILABLE, "Steam 不可用");
        }
        match libs.iter().map(|l| l.join(path)).find(|f| f.exists()) {
            Some(f) => f,
            None => return err(StatusCode::NOT_FOUND, "文件不存在"),
        }
    } else {
        return err(StatusCode::NOT_FOUND, "未知路由");
    };

    eprintln!(
        "[protocol] {} {} range={:?}",
        request.method(),
        uri,
        request.headers().get("range").map(|v| v.to_str().unwrap_or("?"))
    );
    match std::fs::read(&file) {
        Ok(mut bytes) => {
            let mime = mime_of(&file);
            // WE web 直引：Steam 原文件只读不能改，主 HTML 文档在此内存注入 shim。
            // 仅注入整页（无 Range；子资源/分片请求 HTML 不注入）。
            let inject_shim = host == "steam"
                && mime.starts_with("text/html")
                && request.headers().get("range").is_none();
            if inject_shim {
                if let Ok(html) = std::str::from_utf8(&bytes) {
                    bytes = crate::we_shim::inject(html).into_bytes();
                }
            }
            // 视频播放器走 Range：按 bytes=start-end 回 206；无 Range 或解析失败回全量 200
            if let Some((start, end)) = request
                .headers()
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(parse_byte_range)
            {
                let end = end.min(bytes.len() - 1);
                if start > end || start >= bytes.len() {
                    return err(StatusCode::RANGE_NOT_SATISFIABLE, "范围越界");
                }
                let chunk = bytes[start..=end].to_vec();
                lb_wry::http::Response::builder()
                    .status(206)
                    .header("Content-Type", mime)
                    .header("Accept-Ranges", "bytes")
                    .header(
                        "Content-Range",
                        format!("bytes {start}-{end}/{}", bytes.len()),
                    )
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Content-Security-Policy", CSP)
                    .body(Cow::Owned(chunk))
                    .unwrap()
            } else {
                lb_wry::http::Response::builder()
                    .status(200)
                    .header("Content-Type", mime)
                    .header("Accept-Ranges", "bytes")
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Content-Security-Policy", CSP)
                    .body(Cow::Owned(bytes))
                    .unwrap()
            }
        }
        Err(_) => err(StatusCode::NOT_FOUND, "文件不存在"),
    }
}

/// 解析 `bytes=start-end`（仅单段；end 可省略表示到末尾）。
fn parse_byte_range(h: &str) -> Option<(usize, usize)> {
    let spec = h.strip_prefix("bytes=")?;
    let (s, e) = spec.split_once('-')?;
    let start: usize = s.parse().ok()?;
    // 省略 end（bytes=100-）→ usize::MAX，调用方按文件长度截断
    let end: usize = e.parse().unwrap_or(usize::MAX);
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::{parse_byte_range, route};
    #[test]
    fn byte_ranges() {
        assert_eq!(parse_byte_range("bytes=0-100"), Some((0, 100)));
        assert_eq!(parse_byte_range("bytes=500-"), Some((500, usize::MAX)));
        assert_eq!(parse_byte_range("bytes=abc-1"), None);
        assert_eq!(parse_byte_range("0-100"), None);
    }

    /// WE 零拷贝 steam 路由：内存注入 shim、路径空间隔离、原文件不被修改。
    #[test]
    fn steam_route_injects_and_sandboxes() {
        let _env = crate::ENV_LOCK.lock().unwrap();
        let sf = std::env::temp_dir().join(format!("gesso-sf-{}", gesso_core::generate_id()));
        let dir = sf.join("steamapps/workshop/content/431960/777");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("project.json"),
            r#"{"type":"web","file":"main.html","title":"T"}"#,
        ).unwrap();
        std::fs::write(
            dir.join("main.html"),
            b"<html><head><title>T</title></head><body>x</body></html>",
        ).unwrap();
        // ponytail: STEAM_DIR 只在本测试期间指向夹具；各测试路径唯一，无夹具竞争
        unsafe { std::env::set_var("STEAM_DIR", &sf); }

        let req = lb_wry::http::Request::builder()
            .uri("gesso://steam/steamapps/workshop/content/431960/777/main.html")
            .body(vec![]).unwrap();
        let resp = route(req);
        assert_eq!(resp.status(), 200);
        let body = std::str::from_utf8(resp.body()).unwrap();
        // shim 已在内存注入（原文件仍不含）
        assert!(body.contains("wallpaperRegisterAudioListener"));
        assert!(body.contains("<title>T</title>"));
        let orig = std::fs::read_to_string(dir.join("main.html")).unwrap();
        assert!(!orig.contains("wallpaperRegisterAudioListener"));

        // 路径空间隔离：工坊之外的 Steam 文件 403
        let evil = lb_wry::http::Request::builder()
            .uri("gesso://steam/config/config.vdf")
            .body(vec![]).unwrap();
        assert_eq!(route(evil).status(), 403);
        // 穿越分量 403
        let trav = lb_wry::http::Request::builder()
            .uri("gesso://steam/steamapps/workshop/content/431960/777/../../secret")
            .body(vec![]).unwrap();
        assert_eq!(route(trav).status(), 403);
        // 编码态穿越 %2e%2e：先解码再校验，必须仍 403（否则可绕过路径空间隔离）
        let enc_trav = lb_wry::http::Request::builder()
            .uri("gesso://steam/steamapps/workshop/content/431960/777/%2e%2e/%2e%2e/secret")
            .body(vec![]).unwrap();
        assert_eq!(route(enc_trav).status(), 403);

        // 百分号解码：含空格文件名以 %20 请求，能读到磁盘上的真实文件
        std::fs::write(dir.join("my page.html"), b"<html>space</html>").unwrap();
        let spaced = lb_wry::http::Request::builder()
            .uri("gesso://steam/steamapps/workshop/content/431960/777/my%20page.html")
            .body(vec![]).unwrap();
        let sr = route(spaced);
        assert_eq!(sr.status(), 200, "含空格文件名 %20 应解码后命中");
        assert!(std::str::from_utf8(sr.body()).unwrap().contains("<html>space</html>"));

        unsafe { std::env::remove_var("STEAM_DIR"); }
        let _ = std::fs::remove_dir_all(&sf);
    }
}

fn err(
    status: lb_wry::http::StatusCode,
    msg: &str,
) -> lb_wry::http::Response<Cow<'static, [u8]>> {
    lb_wry::http::Response::builder()
        .status(status)
        .body(Cow::Owned(msg.as_bytes().to_vec()))
        .unwrap()
}

fn mime_of(p: &Path) -> &'static str {
    p.extension()
        .and_then(|e| e.to_str())
        .and_then(gesso_core::mime_for_ext)
        .unwrap_or("application/octet-stream")
}
