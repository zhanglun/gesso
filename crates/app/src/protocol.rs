//! gesso:// 只读资源协议。
//!
//! 路由：
//!   gesso://host/<file>            → assets/host/（宿主页自身，单一副本）
//!   gesso://library/<entry>/<file> → <config>/library/<entry>/（白名单 = 随机 entry id）
//!
//! 宿主页从 `gesso://host/index.html` 加载；ContentSpec.source 为指向
//! `gesso://library/<entry>/<主资源>` 的绝对 URL。html 条目整页在该基路径下，
//! 其相对资源引用天然落在条目目录内。
//! 安全：仅 GET、路径穿越防护、HTML 响应注入 CSP。白名单不依赖 origin（跨平台
//! origin 表示不同），靠路径规则 + 不可猜 entry id。

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use raw_window_handle::HasWindowHandle;

/// 所有 gesso 资源同处一 scheme；CSP 按 scheme 收敛。
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' gesso:; \
     style-src 'unsafe-inline' gesso:; frame-src gesso:; \
     media-src gesso: blob:; img-src gesso: data:; connect-src 'none'";

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

/// 某条目内相对资源的 gesso URL（媒体 / iframe / 子资源）。
pub fn library_url(entry_id: &str, rel: &str) -> String {
    format!("gesso://library/{entry_id}/{rel}")
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
    let uri = request.uri();
    let host = uri.host().map(|h| h.as_ref() as &str).unwrap_or_default();
    let path = uri.path().trim_start_matches('/');

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
        Ok(bytes) => {
            let mime = mime_of(&file);
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
    use super::parse_byte_range;
    #[test]
    fn byte_ranges() {
        assert_eq!(parse_byte_range("bytes=0-100"), Some((0, 100)));
        assert_eq!(parse_byte_range("bytes=500-"), Some((500, usize::MAX)));
        assert_eq!(parse_byte_range("bytes=abc-1"), None);
        assert_eq!(parse_byte_range("0-100"), None);
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
