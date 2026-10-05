//! gesso:// 只读资源协议 —— M2 最小版。
//!
//! 路由（技术方案 §7）：
//!   gesso://host/<file>            → assets/host/（宿主页自身）
//!   gesso://library/<entry>/<file> → <config>/library/<entry>/（白名单 = 随机 entry id）
//! 安全：仅 GET 语义、路径穿越防护、HTML 响应注入 CSP（§7.2）。
//! 白名单不依赖 origin（M0.5 事实：跨平台 origin 表示不同），靠路径规则 + 不可猜 entry id。

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use raw_window_handle::HasWindowHandle;

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

/// 库资源 URL
#[allow(dead_code)] // gesso:// 协议修复待办（协议面保留，见 ROADMAP/AGENTS）
pub fn library_url(entry: &str, rel: &str) -> String {
    format!("gesso://library/{entry}/{rel}")
}

/// 库根目录
pub fn library_dir() -> PathBuf {
    config_dir().join("library")
}

/// 宿主页入口 URL
#[allow(dead_code)] // gesso:// 协议修复待办
pub fn host_url() -> &'static str {
    "gesso://host/index.html"
}

/// 带内容规格（urlencoded JSON）的宿主页 URL
#[allow(dead_code)] // gesso:// 协议修复待办
pub fn host_url_with_spec(spec_json: &str) -> String {
    let mut enc = String::with_capacity(spec_json.len() * 3);
    for b in spec_json.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                enc.push(b as char)
            }
            _ => enc.push_str(&format!("%{b:02X}")),
        }
    }
    format!("{}?spec={enc}", host_url())
}

/// 创建挂到给定原生视图的壁纸 webview（注册 gesso:// 协议）。
pub fn create_webview<H: HasWindowHandle + 'static>(
    handle: H,
    url: &str,
) -> Result<lb_wry::WebView, String> {
    // ⚠️ 本版 lb-wry/WKWebView 下注册自定义协议会让 webview 渲染不出任何内容
    //（协议回调零触发 + 页面空白，SPIKE-REPORT 已记）；M2 走条目自包含 file:// 模式，
    // 协议注册在 M3 修复后启用（候选：注册前共享 config / 上游 wry 异步协议 API）
    lb_wry::WebViewBuilder::new()
        .with_url(url)
        .build_as_child(&handle)
        .map_err(|e| format!("{e}"))
}

/// 处理 gesso:// 请求。
#[allow(dead_code)] // gesso:// 协议修复待办（零回调问题，当前 file:// 自包含）
fn handle_request(
    request: &lb_wry::http::Request<Vec<u8>>,
) -> lb_wry::http::Response<Cow<'static, [u8]>> {
    use lb_wry::http::StatusCode;

    // host 自动带上 scheme://host 前缀；uri.path() 即我们路由
    let path = request.uri().path().to_string(); // "/host/index.html"
    let rel = path.trim_start_matches('/');

    let file: PathBuf = if let Some(rest) = rel.strip_prefix("host/") {
        assets_dir().join("host").join(rest)
    } else if let Some(rest) = rel.strip_prefix("library/") {
        let mut it = rest.splitn(2, '/');
        let entry = it.next().unwrap_or_default();
        let tail = it.next().unwrap_or_default();
        library_dir().join(entry).join(tail)
    } else {
        return err(StatusCode::NOT_FOUND, "未知路由");
    };

    // 路径穿越防护：库内相对路径只允许 Normal 分量
    if let Some(stripped) = rel.strip_prefix("library/") {
        if Path::new(stripped)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return err(StatusCode::FORBIDDEN, "路径非法");
        }
    }

    eprintln!(
        "[protocol] {} {} → {}（{}）",
        request.method(),
        request.uri(),
        file.display(),
        if file.exists() { "存在" } else { "缺失" }
    );
    match std::fs::read(&file) {
        Ok(bytes) => lb_wry::http::Response::builder()
            .status(200)
            .header("Content-Type", mime_of(&file))
            .header(
                // v1（M2）：自定义 scheme 跨 host（host/ → library/）下 'self' 会误杀，
                // 放宽到 scheme 级 gesso:；M3 收紧为按条目路径的白名单
                "Content-Security-Policy",
                "default-src 'none'; script-src 'unsafe-inline' gesso:; \
                 style-src 'unsafe-inline' gesso:; media-src gesso: blob:; img-src gesso: data:; \
                 connect-src 'none'",
            )
            .body(Cow::Owned(bytes))
            .unwrap(),
        Err(_) => err(StatusCode::NOT_FOUND, "文件不存在"),
    }
}

#[allow(dead_code)] // gesso:// 协议修复待办
fn err(status: lb_wry::http::StatusCode, msg: &str) -> lb_wry::http::Response<Cow<'static, [u8]>> {
    lb_wry::http::Response::builder()
        .status(status)
        .body(Cow::Owned(msg.as_bytes().to_vec()))
        .unwrap()
}

#[allow(dead_code)] // gesso:// 协议修复待办
fn mime_of(p: &Path) -> &'static str {
    p.extension()
        .and_then(|e| e.to_str())
        .and_then(gesso_core::mime_for_ext)
        .unwrap_or("application/octet-stream")
}
