//! gesso:// 只读资源协议。
//!
//! 路由：
//!   gesso://host/<file>            → assets/host/（宿主页自身，单一副本）
//!   gesso://library/<entry>/<file> → <config>/library/<entry>/（白名单 = 随机 entry id）
//!   gesso://steam/<entry>/<file>   → 该条目 source_dir（WE 用户导入后只读直引）
//!
//! 宿主页从 `gesso://host/index.html` 加载。steam 路由以库清单里的 entry id 为
//! 凭据，查到该条目（用户已导入）的 source_dir 后拼接相对路径——**不扫描磁盘**。
//! 安全：仅 GET、路径穿越防护、HTML 响应注入 CSP，靠路径规则 + 不可猜 entry id。

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use raw_window_handle::HasWindowHandle;

/// 协议请求日志默认静默（视频壁纸每帧 seek/缓冲都会发 Range 请求，启动即刷屏）；
/// `GESSO_PROTOCOL_LOG=1` 打开（协议联调/播放问题排查用）。
fn log_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("GESSO_PROTOCOL_LOG").is_ok_and(|v| v != "0"))
}

/// URI 路径百分号解码。webview 发来的 path 是编码态（空格=%20、非 ASCII=%XX），
/// 磁盘文件是未编码的。**必须在路径穿越校验之前解码**，否则 %2e%2e 可绕过校验。
fn decode_path(p: &str) -> String {
    percent_encoding::percent_decode_str(p)
        .decode_utf8_lossy()
        .into_owned()
}

/// 所有 gesso 资源同处一 scheme；CSP 按 scheme 收敛。
/// frame-src 的 `https:` 仅服务远端网页条目（origin=url，宿主页 iframe 直装
/// https 页面）；本地条目 iframe 仍只能指向 gesso:（sandbox 拦导航，见 host 页注）。
/// Windows（WebView2 workaround，见 pin/windows.rs）：页面实际 origin 是
/// `http://gesso.<host段>`，CSP 源必须按 workaround 宿主枚举（`gesso:` 匹配不到它们）。
/// 协议回调收到的是还原后的 gesso:// URI，路由不分平台。
///
/// CSP 是函数而非常量：视频走回环媒体服务（见下）时 media-src 要放行该源，
/// 端口每次启动随机，运行时拼入。
fn csp() -> String {
    let media = match MEDIA_PORT.get() {
        Some(&p) if p != 0 => format!(" http://127.0.0.1:{p}"),
        _ => String::new(),
    };
    if cfg!(target_os = "macos") {
        format!(
            "default-src 'none'; script-src 'unsafe-inline' gesso:; \
             style-src 'unsafe-inline' gesso:; frame-src gesso: https:; \
             media-src gesso: blob:{media}; img-src gesso: data:; connect-src gesso:"
        )
    } else {
        let h = "http://gesso.host http://gesso.library http://gesso.steam";
        format!(
            "default-src 'none'; \
             script-src 'unsafe-inline' {h}; \
             style-src 'unsafe-inline' {h}; \
             frame-src {h} https:; \
             media-src {h} blob:{media}; img-src {h} data:; connect-src {h}"
        )
    }
}

// ================= 视频媒体回环 HTTP 服务 =================
// WKWebView/WebView2 对自定义 scheme 的媒体装载不流式：AVPlayer 经 WebContent
// 中转，以 2-4KB 碎片 Range 逐段拉取（实测双屏 ~92 req/s，每条三跳 IPC 往返），
// 解码器喂不饱即卡顿——b98b4e1 引入 gesso:// 时只验证了功能未验证吞吐节奏。
// AVPlayer 的原生 HTTP 管线（持久 socket + 大块读 + 内核缓冲）才是正路，故视频
// src 单独走本回环服务；页面/图片/子资源是一次性加载，仍走 gesso:// 无此病。
// 信任模型与 gesso:// 相同：仅绑 127.0.0.1，不可猜 entry id 即凭据，路径解析
// 复用 resolve() 的穿越防护。

/// 0 = 未启动（绑定失败时 media_url 回退 gesso:// 直连，行为同旧版）。
static MEDIA_PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();

/// 进程启动早期调用（会话构建前，端口需先进 OnceLock）。
pub fn start_media_server() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0");
    let port = match &listener {
        Ok(l) => l.local_addr().map(|a| a.port()).unwrap_or(0),
        Err(_) => 0,
    };
    let _ = MEDIA_PORT.set(port);
    if port == 0 {
        eprintln!("[media-http] 回环端口绑定失败，视频回退 gesso:// 协议路径");
        return;
    }
    println!("[media-http] 视频媒体服务 127.0.0.1:{port}");
    if let Ok(l) = listener {
        std::thread::spawn(move || {
            for stream in l.incoming().flatten() {
                std::thread::spawn(move || handle_media_conn(stream));
            }
        });
    }
}

/// 视频 src 的页面可见 URL（AVPlayer 原生 HTTP 管线）。服务未启动时回退
/// entry_url（gesso:// 形态）。
pub fn media_url(entry: &gesso_core::LibraryEntry, rel: &str) -> String {
    match MEDIA_PORT.get() {
        Some(&p) if p != 0 => {
            let route = if entry.origin == "wallpaper-engine" {
                "steam"
            } else {
                "library"
            };
            format!("http://127.0.0.1:{p}/{route}/{}/{rel}", entry.id)
        }
        _ => entry_url(entry, rel),
    }
}

/// 一条回环连接：keep-alive 串行请求。shortcut: 未处理 HTTP 管道化（客户端
/// 携带未读完的下一请求字节），CFNetwork 的媒体 Range 请求是串行等待响应的，
/// 现实中不会触发；若未来换客户端再补。
fn handle_media_conn(mut stream: std::net::TcpStream) {
    use std::io::{Read, Seek as _, SeekFrom, Write};
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut tmp = [0u8; 8192];
    loop {
        // —— 读一个请求头 ——
        buf.clear();
        let head_end = loop {
            if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break p;
            }
            match stream.read(&mut tmp) {
                Ok(0) => return,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(_) => return, // 超时/连接复位：断连，播放器自行重连
            }
            if buf.len() > 16 * 1024 {
                return; // 头部异常膨胀，直接断
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let mut parts = head.lines().next().unwrap_or("").split(' ');
        let method = parts.next().unwrap_or("").to_uppercase();
        let raw_path = parts.next().unwrap_or("").to_string();
        let range = head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("range")
                .then(|| parse_byte_range(v.trim()))
                .flatten()
        });
        if method != "GET" && method != "HEAD" {
            let _ = stream.write_all(
                b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
        // 路径 → 三路由解析（与 gesso:// 同规则同防护）
        let path = raw_path.split('?').next().unwrap_or("");
        let decoded = decode_path(path.trim_start_matches('/'));
        let (host_seg, rest) = decoded.split_once('/').unwrap_or(("", ""));
        let not_found = |stream: &mut std::net::TcpStream| {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        };
        let file = match resolve(host_seg, rest) {
            Ok(f) => f,
            Err(_) => return not_found(&mut stream),
        };
        match std::fs::metadata(&file) {
            Ok(m) if m.is_file() && m.len() > 0 => {}
            _ => return not_found(&mut stream),
        }
        let len = std::fs::metadata(&file).unwrap().len() as usize;
        let (start, end, status) = match range {
            Some((s, _)) if s >= len => {
                let _ = stream.write_all(
                    b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                return;
            }
            Some((s, e)) => (s, e.min(len - 1), "206 Partial Content"),
            None => (0, len - 1, "200 OK"),
        };
        let n = end - start + 1;
        let mut resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {}\r\nAccept-Ranges: bytes\r\nContent-Length: {n}\r\nConnection: keep-alive\r\n",
            mime_of(&file)
        );
        if status.starts_with("206") {
            resp.push_str(&format!("Content-Range: bytes {start}-{end}/{len}\r\n"));
        }
        resp.push_str("\r\n");
        if stream.write_all(resp.as_bytes()).is_err() {
            return;
        }
        if log_enabled() {
            eprintln!("[media-http] {method} {path} → {status} {start}-{end}/{len}");
        }
        if method == "HEAD" {
            continue;
        }
        // —— 正文流送：客户端（AVPlayer）常缓冲够了就断，写失败属正常 ——
        let Ok(mut f) = std::fs::File::open(&file) else {
            return;
        };
        if f.seek(SeekFrom::Start(start as u64)).is_err() {
            return;
        }
        let mut remain = n;
        let mut chunk = vec![0u8; 256 * 1024];
        while remain > 0 {
            let want = remain.min(chunk.len());
            match f.read(&mut chunk[..want]) {
                Ok(0) => break,
                Ok(got) => {
                    if stream.write_all(&chunk[..got]).is_err() {
                        return;
                    }
                    remain -= got;
                }
                Err(_) => return,
            }
        }
    }
}

/// 宿主页/样例资源根（开发态 = crate assets；发布态 = exe 旁 assets）
pub fn assets_dir() -> PathBuf {
    // 开发构建：直接用源码 assets（改宿主页/样例立即生效，无需打包）。
    // 发布构建：绝不能硬编码开发机绝对路径（option_env! 会在编译期固化），
    // 改用 exe 相对路径定位 bundle 内资源。
    if cfg!(debug_assertions) {
        if let Some(dir) = option_env!("CARGO_MANIFEST_DIR") {
            return PathBuf::from(dir).join("assets");
        }
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return PathBuf::from("assets"),
    };
    #[cfg(target_os = "macos")]
    {
        // 标准 .app：Contents/MacOS/gesso → 上两级到 Contents，再 Resources/assets。
        // 组装脚本把 host/ + samples/ 放在 Resources/assets。
        let app_bundle = exe
            .parent() // MacOS
            .and_then(|p| p.parent()) // Contents
            .map(|c| c.join("Resources/assets"));
        if let Some(dir) = app_bundle {
            if dir.exists() {
                return dir;
            }
        }
    }
    // 非 .app / 未命中 bundle：exe 同级 assets（便携 tar、直接跑 release 二进制）。
    exe.parent()
        .map(|d| d.join("assets"))
        .unwrap_or_else(|| PathBuf::from("assets"))
}

/// 配置目录：~/Library/Application Support/Gesso（win: %USERPROFILE%\.gesso）
pub fn config_dir() -> PathBuf {
    // Windows 没有跨进程一致的 HOME（Git Bash 等才有）；此前读 HOME 兜底 "."，
    // 配置目录会跟着 cwd 跑（不同位置启动配置分裂，见 issue #2 排查）。
    // 顺序：Windows 用 USERPROFILE，其余平台 HOME，末位才 "."。
    let home = if cfg!(windows) {
        std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into())
    } else {
        std::env::var("HOME").unwrap_or_else(|_| ".".into())
    };
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

/// 读库清单（steam 路由用它把 entry id 映射到用户已导入条目的 source_dir）。
fn load_manifest() -> Option<gesso_core::LibraryManifest> {
    gesso_core::LibraryManifest::load(&library_dir().join("library.json")).ok()
}

/// 条目资源的页面可见 URL。
/// - WE 条目（origin == "wallpaper-engine"）→ steam 路由，以 entry id 为凭据
/// - 其余（库内拷贝）→ library 路由
/// `rel` 为该条目内的相对资源路径。
///
/// Windows 返回 WebView2 workaround 形态 `http://gesso.<host段>/…`（子资源不走
/// wry 的导航翻译，见 pin/windows.rs）；协议回调按还原后的 gesso:// URI 路由，
/// 因此本函数是页面子资源 URL 唯一的平台分派点。
pub fn entry_url(entry: &gesso_core::LibraryEntry, rel: &str) -> String {
    let gesso = if entry.origin == "wallpaper-engine" {
        format!("gesso://steam/{}/{}", entry.id, rel)
    } else {
        format!("gesso://library/{}/{}", entry.id, rel)
    };
    workaround_page_url(gesso)
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

/// gesso:// 三路由 → 本地文件路径。协议回调与回环媒体服务共用同一套解析
/// 与穿越防护（改路由只改这里）。
type RouteError = (lb_wry::http::StatusCode, &'static str);

fn resolve(host: &str, path: &str) -> Result<PathBuf, RouteError> {
    use lb_wry::http::StatusCode;
    if host == "host" {
        Ok(assets_dir().join("host").join(path))
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
            return Err((StatusCode::FORBIDDEN, "路径非法"));
        }
        Ok(library_dir().join(entry).join(tail))
    } else if host == "steam" {
        // path = "<entry>/<相对资源>"。凭据 = 用户已导入条目的不可猜 id：查清单
        // 找到该条目的 source_dir（用户主动导入时记录的位置），拼接相对路径。
        // 不扫描磁盘、不按路径空间枚举。
        let mut it = path.splitn(2, '/');
        let entry_id = it.next().unwrap_or_default();
        let tail = it.next().unwrap_or_default();
        if Path::new(tail)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err((StatusCode::FORBIDDEN, "路径非法"));
        }
        let Some(man) = load_manifest() else {
            return Err((StatusCode::SERVICE_UNAVAILABLE, "库不可用"));
        };
        let Some(entry) = man.entries.iter().find(|e| e.id == entry_id) else {
            return Err((StatusCode::NOT_FOUND, "条目不存在或未导入"));
        };
        Ok(Path::new(&entry.source_dir).join(tail))
    } else {
        Err((StatusCode::NOT_FOUND, "未知路由"))
    }
}

/// 处理一条 gesso:// 请求（协议回调）。
fn route(request: lb_wry::http::Request<Vec<u8>>) -> lb_wry::http::Response<Cow<'static, [u8]>> {
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

    let file = match resolve(host, path) {
        Ok(f) => f,
        Err((status, msg)) => return err(status, msg),
    };

    if log_enabled() {
        eprintln!(
            "[protocol] {} {} range={:?}",
            request.method(),
            uri,
            request
                .headers()
                .get("range")
                .map(|v| v.to_str().unwrap_or("?"))
        );
    }

    // —— Range 请求：惰性切片（seek + 只读所需字节）——
    // 整读再切片会让大视频（几百 MB）的每次 Range 探查/seek 都全量过盘，
    // 缩略图就绪窗口和壁纸播放双双受害。moov 在文件尾的 mp4 依赖尾部随机读。
    if let Some((start, end_req)) = request
        .headers()
        .get("range")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_byte_range)
    {
        let Some(meta) = std::fs::metadata(&file).ok().filter(|m| m.is_file()) else {
            return err(StatusCode::NOT_FOUND, "文件不存在");
        };
        let len = meta.len() as usize;
        if len == 0 {
            return err(StatusCode::RANGE_NOT_SATISFIABLE, "空文件");
        }
        if start >= len {
            return err(StatusCode::RANGE_NOT_SATISFIABLE, "范围越界");
        }
        // 开放范围（bytes=N-/0-）不读到 EOF：回有限前缀（206 部分响应），
        // 客户端按 Content-Range 自行追索后续区间——否则首字节延迟 = 整文件读取，
        // 大视频的探查请求和 seek 全部超窗。4MB：4K60 ≈14Mbps 下 512KB 只够
        // 0.29s，每秒 3+ 轮三跳 IPC 往返；4MB 降 8 倍且内存代价可接受。
        const OPEN_RANGE_CAP: usize = 4 * 1024 * 1024;
        let end = if end_req >= len {
            (start + OPEN_RANGE_CAP - 1).min(len - 1)
        } else {
            end_req
        };
        use std::io::{Read as _, Seek as _, SeekFrom};
        let Ok(mut f) = std::fs::File::open(&file) else {
            return err(StatusCode::NOT_FOUND, "文件不存在");
        };
        if f.seek(SeekFrom::Start(start as u64)).is_err() {
            return err(StatusCode::RANGE_NOT_SATISFIABLE, "定位失败");
        }
        let mut chunk = vec![0u8; end - start + 1];
        if f.read_exact(&mut chunk).is_err() {
            return err(StatusCode::RANGE_NOT_SATISFIABLE, "读取失败");
        }
        if log_enabled() {
            eprintln!(
                "[protocol] 206 bytes {start}-{end}/{len}（惰性切片 {}KB）",
                chunk.len() / 1024
            );
        }
        return lb_wry::http::Response::builder()
            .status(206)
            .header("Content-Type", mime_of(&file))
            .header("Accept-Ranges", "bytes")
            .header("Content-Range", format!("bytes {start}-{end}/{len}"))
            .header("Access-Control-Allow-Origin", "*")
            .header("Content-Security-Policy", csp())
            .body(Cow::Owned(chunk))
            .unwrap();
    }

    match std::fs::read(&file) {
        Ok(mut bytes) => {
            let mime = mime_of(&file);
            // WE web 直引：Steam 原文件只读不能改，主 HTML 文档在此内存注入 shim。
            // 仅注入整页（无 Range；子资源/分片请求 HTML 不注入）。
            if host == "steam" && mime.starts_with("text/html") {
                if let Ok(html) = std::str::from_utf8(&bytes) {
                    bytes = crate::we_shim::inject(html).into_bytes();
                }
            }
            lb_wry::http::Response::builder()
                .status(200)
                .header("Content-Type", mime)
                .header("Accept-Ranges", "bytes")
                .header("Access-Control-Allow-Origin", "*")
                .header("Content-Security-Policy", csp())
                .body(Cow::Owned(bytes))
                .unwrap()
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
    use super::{library_dir, parse_byte_range, route};
    #[test]
    fn byte_ranges() {
        assert_eq!(parse_byte_range("bytes=0-100"), Some((0, 100)));
        assert_eq!(parse_byte_range("bytes=500-"), Some((500, usize::MAX)));
        assert_eq!(parse_byte_range("bytes=abc-1"), None);
        assert_eq!(parse_byte_range("0-100"), None);
    }

    /// WE steam 路由（entry-id 凭据）：内存注入 shim、仅访问已导入条目、原文件不改。
    #[test]
    fn steam_route_injects_and_sandboxes() {
        let _env = crate::ENV_LOCK.lock().unwrap();
        // 直接接管 HOME（已持 ENV_LOCK，无并行测试竞争）；结束时还原并清理
        let old_home = std::env::var("HOME").unwrap_or_default();
        let thome = std::env::temp_dir().join(format!("gesso-home-{}", gesso_core::generate_id()));
        unsafe {
            std::env::set_var("HOME", &thome);
        }
        let dir = thome.join("workshop/content/431960/777");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("project.json"),
            r#"{"type":"web","file":"main.html","title":"T"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("main.html"),
            b"<html><head><title>T</title></head><body>x</body></html>",
        )
        .unwrap();
        // 用户已导入：清单里有该条目，source_dir 指向用户选中的位置
        let ldir = library_dir();
        std::fs::create_dir_all(&ldir).unwrap();
        let man = gesso_core::LibraryManifest {
            entries: vec![gesso_core::LibraryEntry {
                id: "abc0000000000001".into(),
                kind: gesso_core::WallpaperKind::Html,
                title: "T".into(),
                origin: "wallpaper-engine".into(),
                source_dir: dir.to_string_lossy().into_owned(),
                main_file: Some("main.html".into()),
                source_url: None,
            }],
        };
        man.save(&ldir.join("library.json")).unwrap();

        // 已导入条目：主文档 200，shim 内存注入（原文件仍不含）
        let req = lb_wry::http::Request::builder()
            .uri("gesso://steam/abc0000000000001/main.html")
            .body(vec![])
            .unwrap();
        let resp = route(req);
        assert_eq!(resp.status(), 200);
        let body = std::str::from_utf8(resp.body()).unwrap();
        assert!(body.contains("wallpaperRegisterAudioListener"));
        assert!(body.contains("<title>T</title>"));
        let orig = std::fs::read_to_string(dir.join("main.html")).unwrap();
        assert!(!orig.contains("wallpaperRegisterAudioListener"));

        // 未导入/猜测的 entry id：404（不靠路径空间猜测）
        let unknown = lb_wry::http::Request::builder()
            .uri("gesso://steam/ffffffffffffffff/main.html")
            .body(vec![])
            .unwrap();
        assert_eq!(route(unknown).status(), 404);
        // 已导入条目内路径穿越：403
        let trav = lb_wry::http::Request::builder()
            .uri("gesso://steam/abc0000000000001/../../secret")
            .body(vec![])
            .unwrap();
        assert_eq!(route(trav).status(), 403);
        // 编码态穿越 %2e%2e：先解码再校验，必须仍 403
        let enc_trav = lb_wry::http::Request::builder()
            .uri("gesso://steam/abc0000000000001/%2e%2e/%2e%2e/secret")
            .body(vec![])
            .unwrap();
        assert_eq!(route(enc_trav).status(), 403);

        // 百分号解码：含空格文件名以 %20 请求，读到磁盘上的真实文件
        std::fs::write(dir.join("my page.html"), b"<html>space</html>").unwrap();
        let spaced = lb_wry::http::Request::builder()
            .uri("gesso://steam/abc0000000000001/my%20page.html")
            .body(vec![])
            .unwrap();
        let sr = route(spaced);
        assert_eq!(sr.status(), 200, "含空格文件名 %20 应解码后命中");
        assert!(std::str::from_utf8(sr.body())
            .unwrap()
            .contains("<html>space</html>"));

        unsafe {
            std::env::set_var("HOME", old_home);
        }
        let _ = std::fs::remove_dir_all(&thome);
    }
}

fn err(status: lb_wry::http::StatusCode, msg: &str) -> lb_wry::http::Response<Cow<'static, [u8]>> {
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
