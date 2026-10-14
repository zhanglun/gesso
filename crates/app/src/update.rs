//! 应用自更新（§4.7 软件更新）：GitHub Releases 检查 → 通知 → 安装包下载 → 交付。
//!
//! v1 交付形态 = 下载官方安装包交由系统安装（dmg 挂载 / setup.exe），**不做静默
//! 自替换**——ad-hoc 签名 + 无 Sparkle 基建，自替换的失败模式（半写入、签名失效、
//! 无法回滚）远超收益（设计规格 §4.7 决策）。
//!
//! 并发模型：状态真源 = 本模块全局 [`STATE`]（代数 [`GEN`] 每次变更 +1），
//! 检查/下载跑在独立线程（阻塞 IO，见 `spawn_check`/`spawn_download`），引擎主循环
//! 每拍比对代数、变化即回灌快照 + 刷新窗口——与 150ms 动作轮询同一节奏，无额外通道。
//!
//! 检查优先读取 release 资产里的静态 `latest.json`（不访问限流 API）；旧 release
//! 没有清单时才回退 GitHub API。纯函数（`version_newer`/`parse_release`/`parse_manifest`/
//! `pick_asset`）带单测，网络路径不测。

use std::io::{Read as _, Write as _};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use crate::ui::data::{UpdatePhase, UpdateView};

/// 旧 release 或静态清单不可用时的回退查询（GitHub API 自动排除 draft/prerelease）。
pub const RELEASE_API: &str = "https://api.github.com/repos/zhanglun/gesso/releases/latest";
/// 检查更新首选端点：release 自带的静态清单资产（`releases/latest/download/`
/// 是 github.com 网页端点，不受 API 未认证限流约束；由 release.yml 随每次发布生成）。
pub const RELEASE_MANIFEST: &str =
    "https://github.com/zhanglun/gesso/releases/latest/download/latest.json";
/// 发布页（弹窗「前往发布页」出口；手动下载兜底）。
pub const RELEASE_PAGE: &str = "https://github.com/zhanglun/gesso/releases/latest";
/// 自动检查抑制窗口（§4.7：启动首查后每 24h 一次）。
const AUTO_CHECK_INTERVAL_SECS: u64 = 24 * 3600;

/// 一次发布的可交付信息（已选定本平台安装包）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// 不带 v 前缀的语义版本，如 "0.1.2"。
    pub version: String,
    /// release body（按纯文本展示）。
    pub notes: String,
    pub asset_name: String,
    pub asset_url: String,
    /// 字节（API 提供的资产大小；Content-Length 缺失时进度条兜底用它）。
    pub asset_size: u64,
}

/// 更新子系统状态机（§4.7 弹窗六态的真源投影）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(ReleaseInfo),
    Downloading {
        release: ReleaseInfo,
        downloaded: u64,
        total: u64,
    },
    Ready {
        release: ReleaseInfo,
        path: String,
    },
    Failed {
        message: String,
        /// true = 下载阶段失败（发布信息仍在手，[重试] 直达下载）；false = 检查失败。
        can_retry_download: bool,
    },
}

static STATE: Mutex<UpdateState> = Mutex::new(UpdateState::Idle);
/// 状态代数：每次变更 +1。引擎主循环每拍比对，变化才回灌快照/刷新窗口。
static GEN: AtomicU64 = AtomicU64::new(0);
/// 下载取消旗标（引擎动作置位，下载线程每块数据后检查）。
static CANCEL: AtomicBool = AtomicBool::new(false);
/// 检查/下载互斥（§4.7：忙时忽略重复触发；检查与下载不同时进行）。
static BUSY: AtomicBool = AtomicBool::new(false);
/// 上次自动/手动检查完成的 UNIX 秒（0 = 从未查过；手动检查同样刷新，统一抑制窗口）。
static LAST_CHECK: AtomicU64 = AtomicU64::new(0);
/// 已发过「发现新版本」应用内通知的状态代数（同一次发现只通知一次）。
static NOTIFIED_GEN: AtomicU64 = AtomicU64::new(0);

fn set(state: UpdateState) {
    *STATE.lock().expect("update 状态锁") = state;
    GEN.fetch_add(1, Ordering::SeqCst);
}

pub fn generation() -> u64 {
    GEN.load(Ordering::SeqCst)
}

fn state_snapshot() -> UpdateState {
    STATE.lock().expect("update 状态锁").clone()
}

impl UpdateState {
    fn available(&self) -> Option<&ReleaseInfo> {
        match self {
            UpdateState::Available(r) => Some(r),
            _ => None,
        }
    }
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 启动清理：清空下载目录（`.part` 半成品与已交付的旧安装包不跨会话保留）。
pub fn cleanup_download_dir() {
    let dir = download_dir();
    if dir.exists() {
        println!("[update] 清理下载目录：{}", dir.display());
        std::fs::remove_dir_all(&dir).ok();
    }
}

fn download_dir() -> std::path::PathBuf {
    crate::protocol::config_dir().join("update")
}

/* ---------- 触发入口（引擎动作 / 主循环自动节拍共用） ---------- */

/// 发起检查（后台线程；忙时忽略——§4.7 与进行中的检查/下载互斥）。
pub fn spawn_check() {
    if BUSY.swap(true, Ordering::SeqCst) {
        println!("[update] 已有检查/下载进行中，忽略本次检查请求");
        return;
    }
    std::thread::spawn(run_check);
}

/// 自动检查节拍：仅当开关开启且距上次检查超过 24h 才真正发起（§4.7 检查策略）。
pub fn maybe_auto_check(enabled: bool) {
    if !enabled || BUSY.load(Ordering::SeqCst) {
        return;
    }
    let last = LAST_CHECK.load(Ordering::SeqCst);
    if last != 0 && unix_secs().saturating_sub(last) < AUTO_CHECK_INTERVAL_SECS {
        return;
    }
    spawn_check();
}

/// 取消进行中的下载（回到「发现新版本」态并清理临时文件；非下载态为无害空操作）。
pub fn cancel_download() {
    if matches!(state_snapshot(), UpdateState::Downloading { .. }) {
        CANCEL.store(true, Ordering::SeqCst);
    }
}

/// 关闭更新小窗时调用：失败态重置为 Idle（重开从「检查更新」干净起步，
/// 不停留在旧错误）；其余态保留——Available 还能回来下载、Ready 还能打开安装包。
pub fn reset_failure() {
    let mut s = STATE.lock().expect("update 状态锁");
    if matches!(*s, UpdateState::Failed { .. }) {
        *s = UpdateState::Idle;
        drop(s);
        GEN.fetch_add(1, Ordering::SeqCst);
    }
}

/// 发起下载（仅「发现新版本」态受理；后台流式写盘，进度经状态回灌）。
pub fn spawn_download() {
    let Some(release) = state_snapshot().available().cloned() else {
        return;
    };
    if BUSY.swap(true, Ordering::SeqCst) {
        println!("[update] 已有检查/下载进行中，忽略本次下载请求");
        return;
    }
    CANCEL.store(false, Ordering::SeqCst);
    let total = release.asset_size;
    set(UpdateState::Downloading {
        release: release.clone(),
        downloaded: 0,
        total,
    });
    std::thread::spawn(move || {
        let outcome = download_with_progress(&release);
        // 三条结束路径统一释放互斥标志：漏掉即「重试无反应」（BUSY 死锁到进程重启）
        BUSY.store(false, Ordering::SeqCst);
        match outcome {
            DownloadOutcome::Done(path) => {
                set(UpdateState::Ready {
                    release,
                    path: path.display().to_string(),
                });
            }
            DownloadOutcome::Cancelled => {
                // 回到「发现新版本」：弹窗按既有发布信息重新给出下载入口
                println!("[update] 下载已取消");
                set(UpdateState::Available(release));
            }
            DownloadOutcome::Failed(message) => {
                println!("[update] 下载失败：{message}");
                set(UpdateState::Failed {
                    message,
                    can_retry_download: true,
                });
            }
        }
    });
}

/* ---------- 后台线程体 ---------- */

fn run_check() {
    // BUSY 已由 spawn_check 持有；LAST_CHECK 在发起时刷新（失败也不至高频重试）
    LAST_CHECK.store(unix_secs(), Ordering::SeqCst);
    set(UpdateState::Checking);
    let current = env!("CARGO_PKG_VERSION");
    match fetch_latest() {
        Ok(release) => {
            if version_newer(current, &release.version) {
                println!(
                    "[update] 发现新版本 v{}（当前 v{current}）",
                    release.version
                );
                notify_update_available(&release);
                set(UpdateState::Available(release));
            } else {
                println!("[update] 已是最新（远端 v{}）", release.version);
                set(UpdateState::UpToDate);
            }
        }
        Err(e) => {
            println!("[update] 检查失败：{e}");
            set(UpdateState::Failed {
                message: e,
                can_retry_download: false,
            });
        }
    }
    BUSY.store(false, Ordering::SeqCst);
}

enum DownloadOutcome {
    Done(std::path::PathBuf),
    Cancelled,
    Failed(String),
}

/// 流式下载到 `<配置目录>/update/<name>.part`，成功后原子改名为最终文件。
/// 每块数据后检查取消旗标；每块回写进度（主循环 150ms 节拍取样，无需节流）。
fn download_with_progress(release: &ReleaseInfo) -> DownloadOutcome {
    let asset_name = &release.asset_name;
    let dir = download_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return DownloadOutcome::Failed(format!("无法创建下载目录：{e}"));
    }
    let part = dir.join(format!("{asset_name}.part"));
    let final_path = dir.join(asset_name.as_str());

    println!("[update] 开始下载 {asset_name}");
    let ua = user_agent();
    let resp = match http_agent()
        .get(&release.asset_url)
        .set("User-Agent", &ua)
        .call()
    {
        Ok(r) => r,
        Err(e) => {
            return DownloadOutcome::Failed(format!(
                "下载连接失败：{e}；请检查网络/代理后重试，或前往发布页手动下载"
            ))
        }
    };
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(release.asset_size);
    let mut reader = resp.into_reader();
    let mut file = match std::fs::File::create(&part) {
        Ok(f) => f,
        Err(e) => return DownloadOutcome::Failed(format!("无法写入临时文件：{e}")),
    };
    let mut downloaded: u64 = 0;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if CANCEL.load(Ordering::SeqCst) {
                    drop(file);
                    std::fs::remove_file(&part).ok();
                    println!("[update] 下载已取消，清理 .part");
                    return DownloadOutcome::Cancelled;
                }
                if let Err(e) = file.write_all(&buf[..n]) {
                    std::fs::remove_file(&part).ok();
                    return DownloadOutcome::Failed(format!("写入失败（磁盘空间或权限）：{e}"));
                }
                downloaded += n as u64;
                set(UpdateState::Downloading {
                    release: release.clone(),
                    downloaded,
                    total,
                });
            }
            Err(e) => {
                std::fs::remove_file(&part).ok();
                return DownloadOutcome::Failed(format!("下载中断：{e}"));
            }
        }
    }
    if let Err(e) = std::fs::rename(&part, &final_path) {
        std::fs::remove_file(&part).ok();
        return DownloadOutcome::Failed(format!("下载文件就位失败：{e}"));
    }
    println!("[update] 下载完成：{}", final_path.display());
    DownloadOutcome::Done(final_path)
}

fn user_agent() -> String {
    format!("gesso/{}", env!("CARGO_PKG_VERSION"))
}

/// 共享 agent：连接超时 15s（下载体用整体超时无意义——大文件 + 慢网会被误杀，
/// 停滞依赖用户「取消下载」兜底）。检查请求在其上再叠 15s 整体超时。
fn http_agent() -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(15))
        // 连接停滞不再无限挂起（读不到数据 30s 即报错；慢速下载不受影响）
        .timeout_read(std::time::Duration::from_secs(30));
    // 代理优先级：环境变量（ureq proxy-from-env 默认行为）> 系统代理 > 直连。
    // GUI 进程不继承 shell 环境变量、ureq 不读系统代理——国内用户浏览器能下、
    // 应用内超时（os error 60）的根因即此，故系统代理要自己补读。
    if !env_proxy_set() {
        if let Some(uri) = SYSTEM_PROXY.get_or_init(system_proxy_uri) {
            if let Ok(p) = ureq::Proxy::new(uri) {
                builder = builder.proxy(p);
            }
        }
    }
    builder.build()
}

static SYSTEM_PROXY: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

fn env_proxy_set() -> bool {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
}

/* ---------- 系统代理读取（macOS: scutil --proxy；Windows: Internet Settings 注册表） ---------- */

/// macOS 系统代理（含 Clash/Surge「系统代理」模式写入的静态代理）；PAC 不解析回落直连。
#[cfg(target_os = "macos")]
fn system_proxy_uri() -> Option<String> {
    let out = std::process::Command::new("scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    parse_scutil_proxy(&String::from_utf8_lossy(&out.stdout))
}
/// `scutil --proxy` 输出 → 代理 URI（纯函数，单测覆盖）。优先 HTTPS 代理，
/// 其次 HTTP，最后 SOCKS；各自需 Enable=1 且地址/端口齐全；PAC 不解析。
#[cfg(any(target_os = "macos", test))]
fn parse_scutil_proxy(s: &str) -> Option<String> {
    let get = |key: &str| -> Option<String> {
        s.lines()
            .find(|l| l.trim_start().starts_with(key))
            .and_then(|l| l.split(':').nth(1))
            .map(|v| v.trim().to_string())
    };
    let httpish = |enable: &str, host: &str, port: &str| {
        (get(enable).as_deref() == Some("1"))
            .then(|| {
                get(host)
                    .zip(get(port))
                    .map(|(h, p)| format!("http://{h}:{p}"))
            })
            .flatten()
    };
    httpish("HTTPSEnable", "HTTPSProxy", "HTTPSPort")
        .or_else(|| httpish("HTTPEnable", "HTTPProxy", "HTTPPort"))
        .or_else(|| {
            (get("SOCKSEnable").as_deref() == Some("1"))
                .then(|| {
                    get("SOCKSProxy")
                        .zip(get("SOCKSPort"))
                        .map(|(h, p)| format!("socks5://{h}:{p}"))
                })
                .flatten()
        })
}

/// Windows 系统代理（WinInet Internet Settings；`ProxyServer` 形如
/// `host:port` 或 `http=h:p;https=h:p;socks=h:p`）。
#[cfg(target_os = "windows")]
fn system_proxy_uri() -> Option<String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };
    const SUBKEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings");
    unsafe {
        let mut enabled: u32 = 0;
        let mut cb = std::mem::size_of::<u32>() as u32;
        if RegGetValueW(
            HKEY_CURRENT_USER,
            SUBKEY,
            w!("ProxyEnable"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut enabled as *mut u32 as *mut _),
            Some(&mut cb),
        )
        .0 != 0
            || enabled != 1
        {
            return None;
        }
        let mut buf = [0u16; 256];
        let mut cb = (buf.len() * 2) as u32;
        if RegGetValueW(
            HKEY_CURRENT_USER,
            SUBKEY,
            w!("ProxyServer"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut cb),
        )
        .0 != 0
        {
            return None;
        }
        let s = String::from_utf16_lossy(&buf[..(cb as usize) / 2]);
        parse_windows_proxy_server(&s)
    }
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_proxy_server(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if !s.contains('=') {
        return Some(format!("http://{s}")); // 单一形态：对所有协议生效
    }
    let val = |want: &str| {
        s.split(';').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k.trim() == want).then(|| v.trim().to_string())
        })
    };
    val("https")
        .or_else(|| val("http"))
        .map(|v| format!("http://{v}"))
        .or_else(|| val("socks").map(|v| format!("socks5://{v}")))
}

fn fetch_latest() -> Result<ReleaseInfo, String> {
    // 首选静态清单（github.com 网页端点，不受 api.github.com 未认证 60 次/小时
    // IP 限流约束；Tauri updater 同款模式）；清单缺失/解析失败回退 REST API，
    // 旧 release（无 latest.json 资产）与新版本无缝共存
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("GESSO_RELEASE_API") {
        // 开发验证钩子：覆盖端点作为唯一源（先按清单、再按 API 解析），不叠加回退
        let body = fetch_text(&url, None)?;
        return parse_manifest(&body).or_else(|_| parse_release(&body));
    }
    if let Ok(body) = fetch_text(RELEASE_MANIFEST, None) {
        if let Ok(info) = parse_manifest(&body) {
            return Ok(info);
        }
    }
    let body = fetch_text(RELEASE_API, Some("application/vnd.github+json"))?;
    parse_release(&body)
}

/// GET 文本（共用 UA/代理/超时/错误文案；清单路径的失败由调用方降级，不直接见用户）
fn fetch_text(url: &str, accept: Option<&str>) -> Result<String, String> {
    let mut req = http_agent()
        .get(url)
        .set("User-Agent", &user_agent())
        .timeout(std::time::Duration::from_secs(15));
    if let Some(a) = accept {
        req = req.set("Accept", a);
    }
    let resp = req.call().map_err(check_err_message)?;
    resp.into_string().map_err(|e| format!("响应读取失败：{e}"))
}

/* ---------- 纯函数（单测覆盖） ---------- */

/// 检查失败 → 用户文案。403 且配额耗尽（`x-ratelimit-remaining: 0`）时给限流
/// 专属说法（含重置倒计时）——共享出口 IP（VPN/代理/运营商 NAT）下同出口所有
///设备共吃 60 次/小时，与“连不上”的处置完全不同，不能混为一谈。
fn check_err_message(e: ureq::Error) -> String {
    if let ureq::Error::Status(403, resp) = &e {
        if resp.header("x-ratelimit-remaining") == Some("0") {
            let reset = resp
                .header("x-ratelimit-reset")
                .and_then(|v| v.parse::<u64>().ok());
            return rate_limit_message(reset, now_unix());
        }
    }
    format!("无法连接 GitHub（{e}）；请检查网络后重试，或前往发布页手动下载")
}

/// 限流文案：按重置时刻算剩余分钟（向上取整、至少 1）；拿不到重置头就给通用说法。
fn rate_limit_message(reset_epoch: Option<u64>, now: u64) -> String {
    match reset_epoch {
        Some(reset) => {
            let mins = reset.saturating_sub(now).div_ceil(60);
            format!(
                "GitHub 接口限流：未认证请求按 IP 每小时 60 次，本 IP 配额已用尽，约 {mins} 分钟后重置；稍后重试，或前往发布页手动下载"
            )
        }
        None => "GitHub 接口限流：未认证请求按 IP 每小时 60 次，本 IP 配额已用尽；稍后重试，或前往发布页手动下载".into(),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 语义版本三元组比较；任一侧无法解析（含预发布后缀剥离后仍非法）保守返回 false。
pub fn version_newer(current: &str, candidate: &str) -> bool {
    match (triplet(current), triplet(candidate)) {
        (Some(c), Some(n)) => n > c,
        _ => false,
    }
}

fn triplet(v: &str) -> Option<(u64, u64, u64)> {
    let core = v
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()?;
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// 从 GitHub Release JSON 提取本平台可交付信息。
pub fn parse_release(json: &str) -> Result<ReleaseInfo, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("发布信息解析失败：{e}"))?;
    let version = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or("发布信息缺少 tag_name")?
        .trim()
        .trim_start_matches('v')
        .to_string();
    let notes = v
        .get("body")
        .and_then(|b| b.as_str())
        .unwrap_or_default()
        .to_string();
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or("发布信息缺少 assets")?;
    let list: Vec<(String, u64, String)> = assets
        .iter()
        .filter_map(|a| {
            Some((
                a.get("name")?.as_str()?.to_string(),
                a.get("size")?.as_u64()?,
                a.get("browser_download_url")?.as_str()?.to_string(),
            ))
        })
        .collect();
    let (asset_name, asset_size, asset_url) =
        pick_asset(&list).ok_or("该发布没有本平台的安装包（dmg / portable zip / setup.exe）")?;
    Ok(ReleaseInfo {
        version,
        notes,
        asset_name,
        asset_url,
        asset_size,
    })
}

/// 静态清单（latest.json，release.yml 随每次发布生成并作为 release 资产上传）
/// → ReleaseInfo。结构自定但字段与 API 路径同构：`{version, notes, assets:[{name,size,url}]}`。
/// 解析失败（清单缺失返回的 404 页/HTML）由调用方降级到 API，不直接见用户。
pub fn parse_manifest(json: &str) -> Result<ReleaseInfo, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("更新清单解析失败：{e}"))?;
    let version = v
        .get("version")
        .and_then(|t| t.as_str())
        .ok_or("更新清单缺少 version")?
        .trim()
        .trim_start_matches('v')
        .to_string();
    let notes = v
        .get("notes")
        .and_then(|b| b.as_str())
        .unwrap_or_default()
        .to_string();
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or("更新清单缺少 assets")?;
    let list: Vec<(String, u64, String)> = assets
        .iter()
        .filter_map(|a| {
            Some((
                a.get("name")?.as_str()?.to_string(),
                a.get("size")?.as_u64()?,
                a.get("url")?.as_str()?.to_string(),
            ))
        })
        .collect();
    let (asset_name, asset_size, asset_url) =
        pick_asset(&list).ok_or("该发布没有本平台的安装包（dmg / portable zip / setup.exe）")?;
    Ok(ReleaseInfo {
        version,
        notes,
        asset_name,
        asset_url,
        asset_size,
    })
}

/// 本机构建对应的发布资产 arch 标记（与 packaging 产物命名对齐：
/// macOS `uname -m` → arm64；Windows `$arch` → x64/arm64）。
fn arch_tag() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// 资产挑选评分：>0 可用，分高者优先；平分取先出现（§4.7 安装包选择规则）。
fn asset_score(name: &str, arch: &str) -> i32 {
    let arch_hit = i32::from(name.contains(arch));
    #[cfg(target_os = "macos")]
    {
        if name.ends_with(".dmg") {
            4 + arch_hit
        } else {
            0
        }
    }
    #[cfg(target_os = "windows")]
    {
        if name.ends_with(".exe") && name.contains("setup") {
            6 + arch_hit // 安装器优先：升级路径明确，且注册 AUMID 供系统通知
        } else if name.ends_with(".zip") && name.contains("portable") {
            3 + arch_hit
        } else {
            0
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (arch, arch_hit);
        0
    }
}

pub fn pick_asset(assets: &[(String, u64, String)]) -> Option<(String, u64, String)> {
    let arch = arch_tag();
    let mut best: Option<(i32, usize)> = None;
    for (i, (name, _, _)) in assets.iter().enumerate() {
        let score = asset_score(name, arch);
        if score <= 0 {
            continue;
        }
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, i));
        }
    }
    best.map(|(_, i)| assets[i].clone())
}

/* ---------- 通知与交付 ---------- */

/// 系统通知（best-effort，§4.7 通知第 1 层）：失败只留日志，不阻塞其余通知层。
fn notify_update_available(release: &ReleaseInfo) {
    let body = format!("新版本 v{} 可用，请到设置中更新", release.version);
    #[cfg(target_os = "macos")]
    {
        // osascript：免 bundle/签名限制（UNUserNotificationCenter 需要 .app bundle，
        // 开发裸进程弹不出）。文案经参数数组传入，不经 shell 解析；仅净化引号防
        // AppleScript 注入（版本号来自远端 tag）。
        let safe = |s: &str| s.replace('\\', "").replace('"', "'");
        let script = format!(
            "display notification \"{}\" with title \"Gesso\"",
            safe(&body)
        );
        if let Err(e) = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .spawn()
        {
            println!("[update] 系统通知发送失败（忽略）：{e}");
        }
    }
    #[cfg(target_os = "windows")]
    {
        use tauri_winrt_notification::Toast;
        if let Err(e) = Toast::new("Gesso").title("Gesso").text1(&body).show() {
            println!("[update] 系统 toast 发送失败（便携版未注册 AUMID 时预期内）：{e}");
        }
    }
}

/// 「打开安装包」：macOS 挂载 dmg；Windows 安装器直接运行、zip 交给资源管理器。
pub fn open_installer(path: &str) {
    if path.is_empty() {
        return;
    }
    println!("[update] 打开安装包：{path}");
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    if path.ends_with(".exe") {
        let _ = std::process::Command::new(path).spawn();
    } else {
        reveal(path);
    }
}

/// 「打开所在文件夹」：在系统文件管理器中定位安装包。
pub fn reveal(path: &str) {
    if path.is_empty() {
        return;
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .args(["-R", path])
        .spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{path}"))
        .spawn();
}

/// 「前往发布页」：系统默认浏览器。
pub fn open_release_page() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(RELEASE_PAGE).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", RELEASE_PAGE])
        .spawn();
}

/* ---------- UI 投影（快照注入唯一出处；main.rs snapshot_ui 调用） ---------- */

/// 引擎快照取视图：`GessoState.update` 的数据源（UI 不直接读本模块全局）。
pub fn public_view() -> UpdateView {
    let current = env!("CARGO_PKG_VERSION");
    match state_snapshot() {
        UpdateState::Idle => UpdateView {
            current_version: current.into(),
            ..UpdateView::default()
        },
        UpdateState::Checking => UpdateView {
            phase: UpdatePhase::Checking,
            current_version: current.into(),
            ..UpdateView::default()
        },
        UpdateState::UpToDate => UpdateView {
            phase: UpdatePhase::UpToDate,
            current_version: current.into(),
            ..UpdateView::default()
        },
        UpdateState::Available(r) => UpdateView {
            phase: UpdatePhase::Available,
            current_version: current.into(),
            version: r.version.into(),
            notes: r.notes.into(),
            asset_name: r.asset_name.into(),
            asset_size: r.asset_size,
            ..UpdateView::default()
        },
        UpdateState::Downloading {
            release,
            downloaded,
            total,
        } => UpdateView {
            phase: UpdatePhase::Downloading,
            current_version: current.into(),
            version: release.version.into(),
            asset_name: release.asset_name.into(),
            asset_size: total,
            downloaded,
            ..UpdateView::default()
        },
        UpdateState::Ready { release, path } => UpdateView {
            phase: UpdatePhase::Ready,
            current_version: current.into(),
            version: release.version.into(),
            asset_name: release.asset_name.into(),
            file_path: path.into(),
            ..UpdateView::default()
        },
        UpdateState::Failed {
            message,
            can_retry_download,
        } => UpdateView {
            phase: UpdatePhase::Failed,
            current_version: current.into(),
            error: message.into(),
            can_retry_download,
            ..UpdateView::default()
        },
    }
}

/// 主循环轮询：进入「发现新版本」且尚未通知过该代数时，返回版本号供应用内
/// toast（§4.7 通知第 2 层；系统通知在检查线程内已先行发出）。
pub fn consume_available_toast() -> Option<String> {
    let gen = GEN.load(Ordering::SeqCst);
    if NOTIFIED_GEN.load(Ordering::SeqCst) == gen {
        return None;
    }
    let version = state_snapshot().available().map(|r| r.version.clone())?;
    NOTIFIED_GEN.store(gen, Ordering::SeqCst);
    Some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare_semantics() {
        assert!(version_newer("0.1.1", "0.1.2"));
        assert!(version_newer("0.1.1", "0.2.0"));
        assert!(version_newer("0.1.1", "1.0.0"));
        assert!(version_newer("0.1.1", "v0.1.2"), "v 前缀剥离");
        assert!(!version_newer("0.1.2", "0.1.2"), "相等不算新");
        assert!(!version_newer("0.1.2", "0.1.1"));
        assert!(!version_newer("0.1.2", "0.0.9"));
        assert!(!version_newer("0.1.1", "abc"), "候选非法 → 保守不更新");
        assert!(!version_newer("", "0.2.0"), "当前非法 → 保守不更新");
        assert!(
            version_newer("0.1.1", "0.1.2-beta"),
            "预发布后缀剥离后按主三元组比较"
        );
        assert!(!version_newer("0.1.1", "0.1.1-beta"), "同版本预发布不算新");
    }

    const SAMPLE_JSON: &str = r###"{
        "tag_name": "v0.2.0",
        "body": "## Fixes\n- 竖版适配",
        "assets": [
            {"name": "Gesso-0.2.0-arm64.dmg", "size": 48000000, "browser_download_url": "https://x/arm64.dmg"},
            {"name": "Gesso-0.2.0-x64.dmg", "size": 52000000, "browser_download_url": "https://x/x64.dmg"},
            {"name": "Gesso-0.2.0-x64-portable.zip", "size": 41000000, "browser_download_url": "https://x/x64.zip"},
            {"name": "Gesso-0.2.0-x64-setup.exe", "size": 43000000, "browser_download_url": "https://x/x64-setup.exe"},
            {"name": "checksums.txt", "size": 400, "browser_download_url": "https://x/checksums.txt"}
        ]
    }"###;

    #[test]
    fn parse_release_extracts_version_notes_and_asset() {
        let r = parse_release(SAMPLE_JSON).expect("样例可解析");
        assert_eq!(r.version, "0.2.0");
        assert!(r.notes.contains("竖版适配"));
        // 资产按本机平台挑选（§4.7 规则：macOS=dmg / Windows=setup.exe，arch
        // 命中优先）——期望值按 arch_tag() 动态推导，断言在双平台 CI 都成立
        let (want_name, want_size, want_url): (&str, u64, &str) = if cfg!(target_os = "macos") {
            ("Gesso-0.2.0-arm64.dmg", 48000000, "https://x/arm64.dmg")
        } else {
            (
                "Gesso-0.2.0-x64-setup.exe",
                43000000,
                "https://x/x64-setup.exe",
            )
        };
        assert_eq!(r.asset_name, want_name, "arch 命中本平台安装包");
        assert_eq!(r.asset_size, want_size);
        assert_eq!(r.asset_url, want_url);
    }

    const SAMPLE_MANIFEST: &str = r###"{
        "version": "v0.3.0",
        "notes": "## 更新\n- 不再查询 GitHub API",
        "pub_date": "2026-10-14T12:00:00Z",
        "assets": [
            {"name": "Gesso-0.3.0-arm64.dmg", "size": 49000000, "url": "https://x/manifest-arm64.dmg"},
            {"name": "Gesso-0.3.0-x64.dmg", "size": 53000000, "url": "https://x/manifest-x64.dmg"},
            {"name": "Gesso-0.3.0-x64-portable.zip", "size": 41000000, "url": "https://x/manifest-x64.zip"},
            {"name": "Gesso-0.3.0-x64-setup.exe", "size": 43000000, "url": "https://x/manifest-x64-setup.exe"},
            {"name": "checksums.txt", "size": 400, "url": "https://x/checksums.txt"}
        ]
    }"###;

    #[test]
    fn parse_manifest_extracts_version_notes_and_asset() {
        let r = parse_manifest(SAMPLE_MANIFEST).expect("清单可解析");
        assert_eq!(r.version, "0.3.0");
        assert!(r.notes.contains("不再查询 GitHub API"));
        if cfg!(target_os = "macos") {
            assert!(r.asset_name.ends_with(".dmg"));
            assert_eq!(
                r.asset_size,
                if cfg!(target_arch = "aarch64") {
                    49000000
                } else {
                    53000000
                }
            );
        } else {
            assert_eq!(r.asset_name, "Gesso-0.3.0-x64-setup.exe");
            assert_eq!(r.asset_size, 43000000);
        }
    }

    #[test]
    fn parse_manifest_rejects_malformed() {
        assert!(parse_manifest("not json").is_err());
        assert!(
            parse_manifest(r#"{"version":"1.0.0"}"#).is_err(),
            "缺 assets"
        );
        assert!(
            parse_manifest(r#"{"notes":"x","assets":[]}"#).is_err(),
            "缺 version"
        );
    }

    #[test]
    fn parse_release_rejects_malformed() {
        assert!(parse_release("not json").is_err());
        assert!(
            parse_release(r#"{"tag_name": "v1.0.0"}"#).is_err(),
            "缺 assets"
        );
        let no_asset = r#"{"tag_name": "v1.0.0", "assets": [{"name": "checksums.txt", "size": 1, "browser_download_url": "https://x"}]}"#;
        assert!(parse_release(no_asset).is_err(), "无本平台资产 → 明确报错");
    }

    #[test]
    fn pick_asset_scores_by_platform_rules() {
        let mk = |names: &[&str]| -> Vec<(String, u64, String)> {
            names
                .iter()
                .enumerate()
                .map(|(i, n)| (n.to_string(), i as u64, format!("https://x/{n}")))
                .collect()
        };
        // 平台无关断言：arch 命中优先于 arch 未命中
        #[cfg(target_os = "macos")]
        {
            let picked =
                pick_asset(&mk(&["Gesso-0.2.0-x64.dmg", "Gesso-0.2.0-arm64.dmg"])).unwrap();
            assert_eq!(picked.0, "Gesso-0.2.0-arm64.dmg");
            let fallback = pick_asset(&mk(&["Gesso-0.2.0-x64.dmg", "checksums.txt"])).unwrap();
            assert_eq!(
                fallback.0, "Gesso-0.2.0-x64.dmg",
                "无 arch 匹配退回任意 dmg"
            );
            assert!(pick_asset(&mk(&["only.zip"])).is_none(), "macOS 不取 zip");
        }
        #[cfg(target_os = "windows")]
        {
            let picked = pick_asset(&mk(&[
                "Gesso-0.2.0-x64-portable.zip",
                "Gesso-0.2.0-x64-setup.exe",
            ]))
            .unwrap();
            assert_eq!(
                picked.0, "Gesso-0.2.0-x64-setup.exe",
                "安装器优先于便携 zip"
            );
            let portable_only = pick_asset(&mk(&["Gesso-0.2.0-arm64-portable.zip"])).unwrap();
            assert_eq!(
                portable_only.0, "Gesso-0.2.0-arm64-portable.zip",
                "无安装器时取便携 zip"
            );
            assert!(
                pick_asset(&mk(&["Gesso-0.2.0.dmg"])).is_none(),
                "Windows 不取 dmg"
            );
        }
    }

    #[test]
    fn toast_dedupes_per_generation() {
        // 未曾进入 Available：不通知
        set(UpdateState::Idle);
        assert!(consume_available_toast().is_none());
        // 进入 Available：首次通知，同代数去重
        set(UpdateState::Available(ReleaseInfo {
            version: "0.2.0".into(),
            notes: String::new(),
            asset_name: "a.dmg".into(),
            asset_url: "https://x".into(),
            asset_size: 1,
        }));
        let first = consume_available_toast();
        assert_eq!(first.as_deref(), Some("0.2.0"));
        assert!(consume_available_toast().is_none(), "同代数不重复通知");
        // 新一次发现（代数变化）：再次通知
        set(UpdateState::Checking);
        set(UpdateState::Available(ReleaseInfo {
            version: "0.3.0".into(),
            notes: String::new(),
            asset_name: "a.dmg".into(),
            asset_url: "https://x".into(),
            asset_size: 1,
        }));
        assert_eq!(consume_available_toast().as_deref(), Some("0.3.0"));
        set(UpdateState::Idle);
    }

    #[test]
    fn scutil_proxy_precedence_and_gating() {
        // 三者全开：HTTPS 优先
        let all = "<dictionary> {\n HTTPSEnable : 1\n HTTPSProxy : 10.0.0.2\n HTTPSPort : 443\n HTTPEnable : 1\n HTTPProxy : 10.0.0.1\n HTTPPort : 7890\n SOCKSEnable : 1\n SOCKSProxy : 127.0.0.1\n SOCKSPort : 7891\n }";
        assert_eq!(
            parse_scutil_proxy(all).as_deref(),
            Some("http://10.0.0.2:443")
        );
        // 仅 HTTP 开
        let http_only = "HTTPEnable : 1\n HTTPProxy : 127.0.0.1\n HTTPPort : 7890\n HTTPSEnable : 0\n SOCKSEnable : 0";
        assert_eq!(
            parse_scutil_proxy(http_only).as_deref(),
            Some("http://127.0.0.1:7890")
        );
        // 仅 SOCKS 开
        let socks_only =
            "SOCKSEnable : 1\n SOCKSProxy : 127.0.0.1\n SOCKSPort : 7891\n HTTPEnable : 0";
        assert_eq!(
            parse_scutil_proxy(socks_only).as_deref(),
            Some("socks5://127.0.0.1:7891")
        );
        // 全关 / 仅 PAC：直连（None）
        assert_eq!(parse_scutil_proxy("HTTPEnable : 0\n SOCKSEnable : 0"), None);
        assert_eq!(
            parse_scutil_proxy(
                "ProxyAutoConfigEnable : 1\n ProxyAutoConfigURLString : http://x/pac"
            ),
            None
        );
        // 开了但缺地址：不产半截 URI
        assert_eq!(parse_scutil_proxy("HTTPEnable : 1\n HTTPPort : 7890"), None);
    }

    #[test]
    fn rate_limit_message_minutes() {
        // 剩余 1–60 秒 → 1 分钟；整分钟向上取整；无重置头 → 通用说法
        assert!(rate_limit_message(Some(1000), 999).contains("约 1 分钟后重置"));
        assert!(rate_limit_message(Some(1120), 1000).contains("约 2 分钟后重置"));
        assert!(rate_limit_message(Some(940), 1000).contains("稍后重试")); // 已过期也给 1 分钟
        assert!(rate_limit_message(None, 1000).contains("配额已用尽"));
        assert!(!rate_limit_message(None, 1000).contains("分钟后"));
    }

    #[test]
    fn windows_proxy_server_forms() {
        // 单一形态：对所有协议生效
        assert_eq!(
            parse_windows_proxy_server("127.0.0.1:7890").as_deref(),
            Some("http://127.0.0.1:7890")
        );
        // 分协议形态：https > http > socks
        let per = "http=127.0.0.1:8080;https=127.0.0.1:7891;socks=127.0.0.1:7892";
        assert_eq!(
            parse_windows_proxy_server(per).as_deref(),
            Some("http://127.0.0.1:7891")
        );
        assert_eq!(
            parse_windows_proxy_server("http=127.0.0.1:8080;socks=127.0.0.1:7892").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(
            parse_windows_proxy_server("socks=127.0.0.1:7892").as_deref(),
            Some("socks5://127.0.0.1:7892")
        );
        // 空串 / 空白
        assert_eq!(parse_windows_proxy_server(""), None);
        assert_eq!(parse_windows_proxy_server("   "), None);
    }
}
