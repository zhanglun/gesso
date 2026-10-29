//! Local browser control surface for the desktop pet.
//!
//! Phase 1 is deliberately read-only: loopback HTTP, one-time bootstrap code,
//! session cookie, and a public pet snapshot. It does not touch PetStore or UI.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gesso_core::{AdoptionState, PetActivity, PetBehaviorState, PetSave, PetSize};
use serde_json::json;

use crate::pet_template::FrameKind;
use crate::pet_template_candle;

const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024;
const BOOTSTRAP_TTL: Duration = Duration::from_secs(60);

static CONTROL: OnceLock<Arc<WebControl>> = OnceLock::new();

pub fn open() {
    let control = CONTROL.get_or_init(|| Arc::new(WebControl::start()));
    control.open_browser();
}

pub fn active() -> bool {
    CONTROL.get().is_some()
}

pub fn publish(pet: &PetSave, behavior: PetBehaviorState, adoption: &AdoptionState) {
    if let Some(control) = CONTROL.get() {
        let (body, preview) = snapshot_json(pet, behavior, adoption);
        control.publish(body, preview);
    }
}

pub fn shutdown() {
    if let Some(control) = CONTROL.get() {
        control.shutdown();
    }
}

/// Web 写操作（领养流程）。全部转换为既有 EngineAction，由主循环在引擎线程
/// 执行——Web 线程绝不直接触碰 AdoptionState / PetStore。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebOp {
    AdoptionStart,
    AdoptionReroll,
    AdoptionSelect(usize),
    AdoptionConfirm,
    AdoptionBack,
    /// 幂等设置宠物显示状态（pet.set_visible）。
    SetVisible(bool),
    /// 尺寸档位（pet.set_size：small/medium/large）。
    SetSize(PetSize),
    /// 活跃度档位（pet.set_activity：quiet/normal/lively）。
    SetActivity(PetActivity),
    /// 重置第一阶段：进入待确认态（安全，不破坏数据）。
    ResetRequest,
    /// 重置第二阶段：执行清除（服务端额外校验待确认态）。
    ResetConfirm,
    /// 取消重置，回到 Adopted。
    ResetCancel,
}

/// 写操作簿记：request_id → operation_id 去重，operation_id → 已执行标记。
/// 只在进程内存中存在，随 Gesso 退出一起消失。
struct OpLog {
    queue: Vec<(String, WebOp)>,
    seen: HashMap<String, String>,
    done: HashSet<String>,
}

static OP_LOG: OnceLock<Mutex<OpLog>> = OnceLock::new();

fn op_log() -> &'static Mutex<OpLog> {
    OP_LOG.get_or_init(|| {
        Mutex::new(OpLog {
            queue: Vec::new(),
            seen: HashMap::new(),
            done: HashSet::new(),
        })
    })
}

/// 主循环取走待执行操作（取走即标记已执行——入队引擎动作是本拍的提交点）。
pub fn drain_ops() -> Vec<(String, WebOp)> {
    let mut log = op_log().lock().expect("宠物控制台操作簿记");
    let drained = std::mem::take(&mut log.queue);
    for (operation_id, _) in &drained {
        log.done.insert(operation_id.clone());
    }
    drained
}

fn parse_op(value: &serde_json::Value) -> Option<WebOp> {
    let name = value.get("op")?.as_str()?;
    let index = |max: usize| -> Option<usize> {
        let raw = value.get("index")?.as_u64()?;
        (raw <= max as u64).then_some(raw as usize)
    };
    match name {
        "adoption.start" => Some(WebOp::AdoptionStart),
        "adoption.reroll" => Some(WebOp::AdoptionReroll),
        "adoption.back" => Some(WebOp::AdoptionBack),
        "adoption.confirm" => Some(WebOp::AdoptionConfirm),
        "adoption.select" => Some(WebOp::AdoptionSelect(index(2)?)),
        "pet.set_visible" => Some(WebOp::SetVisible(value.get("visible")?.as_bool()?)),
        "pet.set_size" => {
            let size = match value.get("size")?.as_str()? {
                "small" => PetSize::Small,
                "medium" => PetSize::Medium,
                "large" => PetSize::Large,
                _ => return None,
            };
            Some(WebOp::SetSize(size))
        }
        "pet.set_activity" => {
            let activity = match value.get("activity")?.as_str()? {
                "quiet" => PetActivity::Quiet,
                "normal" => PetActivity::Normal,
                "lively" => PetActivity::Lively,
                _ => return None,
            };
            Some(WebOp::SetActivity(activity))
        }
        "pet.reset.request" => Some(WebOp::ResetRequest),
        "pet.reset.confirm" => Some(WebOp::ResetConfirm),
        "pet.reset.cancel" => Some(WebOp::ResetCancel),
        _ => None,
    }
}

/// 重置第二阶段只能在待确认态执行；其他任何状态都拒绝。
fn reset_pending(body: &serde_json::Value) -> bool {
    body.get("adoption")
        .and_then(|a| a.get("status"))
        .and_then(|s| s.as_str())
        == Some("reset_confirm")
}

fn snapshot_json(
    pet: &PetSave,
    behavior: PetBehaviorState,
    adoption: &AdoptionState,
) -> (serde_json::Value, Option<PreviewAppearance>) {
    let preview = pet.adoption.as_ref().map(|adoption| PreviewAppearance {
        coat: adoption.appearance.coat_color,
        accent: adoption.appearance.accent,
        marking: adoption.appearance.marking,
        body: adoption.appearance.body,
    });
    let public_pet = pet.adoption.as_ref().map(|adoption| {
        json!({
            "name": {"zh": &adoption.name.zh, "en": &adoption.name.en},
            "species": format!("{:?}", adoption.species).to_ascii_lowercase(),
            "behavior": behavior_name(behavior),
            "visible": pet.visible,
            "activity": format!("{:?}", pet.activity).to_ascii_lowercase(),
            "size": format!("{:?}", pet.size).to_ascii_lowercase(),
            "stats": {
                "companion_seconds": pet.stats.companion_seconds,
                "touch_count": pet.stats.touch_count,
                "poke_count": pet.stats.poke_count,
            }
        })
    });
    (
        json!({
            "adoption": adoption_json(adoption),
            "pet": public_pet,
        }),
        preview,
    )
}

fn adoption_json(state: &AdoptionState) -> serde_json::Value {
    let candidates = match state {
        AdoptionState::CandidatesVisible(set) | AdoptionState::CandidateSelected(set) => Some(set),
        AdoptionState::Persisting { candidates: set } => Some(set),
        _ => None,
    };
    let Some(set) = candidates else {
        return json!({"status": adoption_status(state)});
    };
    let selected = set.selected;
    json!({
        "status": adoption_status(state),
        "species": format!("{:?}", set.species).to_ascii_lowercase(),
        "reroll_generation": set.reroll_generation,
        "selected": selected,
        "candidates": set.candidates.iter().enumerate().map(|(index, candidate)| json!({
            "index": index,
            "candidate_id": set.candidate_id(index).map(|id| id.to_hex()),
            "name": {"zh": &candidate.name.zh, "en": &candidate.name.en},
            "appearance": {
                "body": candidate.appearance.body,
                "coat_color": candidate.appearance.coat_color,
                "marking": candidate.appearance.marking,
                "accent": candidate.appearance.accent,
            }
        })).collect::<Vec<_>>(),
    })
}

fn adoption_status(state: &AdoptionState) -> &'static str {
    match state {
        AdoptionState::NotAdopted => "not_adopted",
        AdoptionState::CandidatesVisible(_) => "candidates_visible",
        AdoptionState::CandidateSelected(_) => "candidate_selected",
        AdoptionState::Persisting { .. } => "persisting",
        AdoptionState::Adopted => "adopted",
        AdoptionState::ResetConfirm => "reset_confirm",
    }
}

fn published_pet(state: &Arc<Mutex<PublishedState>>) -> Option<PreviewAppearance> {
    state.lock().expect("宠物控制台快照").preview
}

/// 取预览外观：无 candidate 参数 = 正式宠物；有 = 在已发布快照的公开候选 DTO
/// 里按 candidate_id 查找（candidate_id 本就是公开字段，无新增泄露面）。
fn published_appearance(
    state: &Arc<Mutex<PublishedState>>,
    candidate_id: Option<&str>,
) -> Option<PreviewAppearance> {
    let state = state.lock().expect("宠物控制台快照");
    let candidate_id = candidate_id?;
    let candidates = state.body.get("adoption")?.get("candidates")?.as_array()?;
    candidates
        .iter()
        .find(|c| c.get("candidate_id").and_then(|v| v.as_str()) == Some(candidate_id))
        .and_then(|c| c.get("appearance"))
        .map(|a| PreviewAppearance {
            coat: a.get("coat_color").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
            accent: a.get("accent").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
            marking: a.get("marking").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
            body: a.get("body").and_then(|v| v.as_u64()).unwrap_or(0) as u8,
        })
}

/// 解析 preview 查询：`state` 必填（白名单），`candidate` 可选（hex ≤64）；
/// 其他任何参数拒绝。
fn preview_query(path: &str) -> Option<(FrameKind, Option<String>)> {
    let (base, query) = path.split_once('?').unwrap_or((path, ""));
    if base != "/api/pet/preview" {
        return None;
    }
    let mut kind = None;
    let mut candidate = None;
    for part in query.split('&') {
        if let Some(value) = part.strip_prefix("state=") {
            if kind.is_some() {
                return None;
            }
            kind = Some(match value {
                "idle" => FrameKind::Idle,
                "sleep" => FrameKind::Sleep,
                "blink" => FrameKind::Blink,
                _ => return None,
            });
        } else if let Some(value) = part.strip_prefix("candidate=") {
            if candidate.is_some()
                || value.is_empty()
                || value.len() > 64
                || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return None;
            }
            candidate = Some(value.to_string());
        } else {
            return None;
        }
    }
    Some((kind?, candidate))
}

fn render_preview(pet: PreviewAppearance, kind: FrameKind) -> Vec<u8> {
    let template = pet_template_candle::candle();
    let cells = template.compose(kind, pet.marking, pet.body);
    let colors = template.palette(pet.coat, pet.accent, 0);
    let mut rgba = vec![0; cells.len() * 4];
    template.render_rgba(&cells, &colors, &mut rgba);
    encode_png(template.grid_w as u32, template.grid_h as u32, &rgba)
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut png = Vec::from(&b"\x89PNG\r\n\x1a\n"[..]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png_chunk(&mut png, b"IHDR", &ihdr);
    let mut scanlines = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in rgba.chunks_exact(width as usize * 4) {
        scanlines.push(0);
        scanlines.extend_from_slice(row);
    }
    png_chunk(&mut png, b"IDAT", &zlib_store(&scanlines));
    png_chunk(&mut png, b"IEND", &[]);
    png
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&[kind.as_slice(), data].concat()).to_be_bytes());
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = data.chunks(65_535).collect();
    for (index, chunk) in chunks.iter().enumerate() {
        out.push(u8::from(index + 1 == chunks.len()));
        out.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn response_json(state: &PublishedState) -> String {
    json!({
        "state_revision": state.revision,
        "adoption_revision": state.adoption_revision,
        "adoption": state.body.get("adoption").cloned().unwrap_or(serde_json::Value::Null),
        "pet": state.body.get("pet").cloned().unwrap_or(serde_json::Value::Null),
    })
    .to_string()
}

fn behavior_name(state: PetBehaviorState) -> &'static str {
    match state {
        PetBehaviorState::Idle => "idle",
        PetBehaviorState::Watching => "watching",
        PetBehaviorState::Walking => "walking",
        PetBehaviorState::Sleeping => "sleeping",
        PetBehaviorState::Poked => "poked",
        PetBehaviorState::Touched => "touched",
        PetBehaviorState::Dragging => "dragging",
    }
}

struct WebControl {
    addr: std::net::SocketAddr,
    stop: Arc<std::sync::atomic::AtomicBool>,
    state: Arc<Mutex<PublishedState>>,
    auth: Arc<Mutex<AuthState>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PreviewAppearance {
    coat: u8,
    accent: u8,
    marking: u8,
    body: u8,
}

struct PublishedState {
    revision: u64,
    /// 只在领养状态/候选变化时递增——写操作用它做乐观锁，
    /// 避免行为状态 (idle/blink) 频繁跳动让 Web 写请求永远过期。
    adoption_revision: u64,
    body: serde_json::Value,
    preview: Option<PreviewAppearance>,
}

/// publish 的纯逻辑（抽出供单测直接验证 revision 规则）。
fn apply_publish(
    state: &mut PublishedState,
    body: serde_json::Value,
    preview: Option<PreviewAppearance>,
) {
    if state.body.get("adoption") != body.get("adoption") {
        state.adoption_revision = state.adoption_revision.saturating_add(1);
    }
    if state.body != body || state.preview != preview {
        state.revision = state.revision.saturating_add(1);
        state.body = body;
        state.preview = preview;
    }
}

struct AuthState {
    bootstrap: Option<String>,
    bootstrap_expires: std::time::Instant,
    sessions: HashSet<String>,
}

impl WebControl {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("本地宠物控制台端口");
        listener
            .set_nonblocking(true)
            .expect("本地宠物控制台非阻塞监听");
        let addr = listener.local_addr().expect("本地宠物控制台地址");
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let state = Arc::new(Mutex::new(PublishedState {
            revision: 0,
            adoption_revision: 0,
            body: json!({"adoption": {"status": "not_adopted"}, "pet": null}),
            preview: None,
        }));
        let auth = Arc::new(Mutex::new(AuthState {
            bootstrap: Some(random_hex()),
            bootstrap_expires: std::time::Instant::now() + BOOTSTRAP_TTL,
            sessions: HashSet::new(),
        }));
        let thread_stop = Arc::clone(&stop);
        let thread_state = Arc::clone(&state);
        let thread_auth = Arc::clone(&auth);
        let host = addr.to_string();
        let handle = thread::Builder::new()
            .name("gesso-pet-web".to_string())
            .spawn(move || serve(listener, host, thread_stop, thread_state, thread_auth))
            .expect("启动宠物控制台服务");
        Self {
            addr,
            stop,
            state,
            auth,
            thread: Mutex::new(Some(handle)),
        }
    }

    fn publish(&self, body: serde_json::Value, preview: Option<PreviewAppearance>) {
        let mut state = self.state.lock().expect("宠物控制台快照");
        apply_publish(&mut state, body, preview);
    }

    fn open_browser(&self) {
        let bootstrap = {
            let mut auth = self.auth.lock().expect("宠物控制台认证");
            if auth.bootstrap.is_none() {
                auth.bootstrap = Some(random_hex());
                auth.bootstrap_expires = std::time::Instant::now() + BOOTSTRAP_TTL;
            }
            auth.bootstrap.clone().unwrap_or_default()
        };
        let url = format!("http://{}/#code={bootstrap}", self.addr);
        // 开发验收钩子（仅 debug + 显式 env）：回显完整引导 URL，供无头 HTTP 驱动
        // 走完 exchange → 领养写操作全链路；GESSO_PET_WEB_NO_OPEN 额外跳过系统
        // 浏览器（避免真实浏览器抢先消费一次性 code）。正式构建永不输出 code。
        #[cfg(debug_assertions)]
        {
            let echo = std::env::var("GESSO_PET_WEB_ECHO").is_ok();
            let no_open = std::env::var("GESSO_PET_WEB_NO_OPEN").is_ok();
            if echo {
                eprintln!("[pet-web] bootstrap url: {url}");
            }
            if !no_open {
                open_url(&url);
            }
        }
        #[cfg(not(debug_assertions))]
        open_url(&url);
        eprintln!("[pet-web] 已打开本地控制台：http://{}", self.addr);
    }

    fn shutdown(&self) {
        if self.stop.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.thread.lock().expect("宠物控制台线程").take() {
            let _ = handle.join();
        }
        self.auth.lock().expect("宠物控制台认证").sessions.clear();
    }
}

impl Drop for WebControl {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn serve(
    listener: TcpListener,
    host: String,
    stop: Arc<std::sync::atomic::AtomicBool>,
    state: Arc<Mutex<PublishedState>>,
    auth: Arc<Mutex<AuthState>>,
) {
    while !stop.load(std::sync::atomic::Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                // 每连接一线程：慢客户端最多拖住自己（2s 读超时），不阻塞
                // accept 循环和其他请求（shutdown 的唤醒连接也保证及时处理）
                let host = host.clone();
                let state = Arc::clone(&state);
                let auth = Arc::clone(&auth);
                let _ = thread::Builder::new()
                    .name("gesso-pet-web-conn".to_string())
                    .spawn(move || handle(stream, &host, &state, &auth));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                eprintln!("[pet-web] listener error: {error}");
                break;
            }
        }
    }
}

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn handle(
    mut stream: TcpStream,
    host: &str,
    state: &Arc<Mutex<PublishedState>>,
    auth: &Arc<Mutex<AuthState>>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(status) => {
            return_response(&mut stream, status, "bad_request");
            return;
        }
    };
    let Some(request) = request else { return };
    if header(&request, "host").as_deref() != Some(host) {
        return_response(&mut stream, 403, "invalid_host");
        return;
    }

    if request.method == "GET" && request.path == "/" {
        respond(&mut stream, 200, "text/html; charset=utf-8", WEB_INDEX);
        return;
    }
    if request.method == "GET" && request.path == "/assets/app.css" {
        respond(&mut stream, 200, "text/css; charset=utf-8", WEB_CSS);
        return;
    }
    if request.method == "GET" && request.path == "/assets/app.js" {
        respond(&mut stream, 200, "text/javascript; charset=utf-8", WEB_JS);
        return;
    }
    if request.method == "POST" && request.path == "/api/session/exchange" {
        exchange(&mut stream, &request, host, auth);
        return;
    }
    if !valid_session(&request, auth) {
        return_response(&mut stream, 401, "unauthorized");
        return;
    }
    if request.method == "POST" && request.path == "/api/adoption/op" {
        adoption_op(&mut stream, &request, state, host);
        return;
    }
    if request.method == "GET" && request.path.starts_with("/api/operations/") {
        operation_status(&mut stream, &request);
        return;
    }
    if request.method == "GET" && request.path == "/api/state" {
        let state = state.lock().expect("宠物控制台快照");
        let body = response_json(&state);
        respond(&mut stream, 200, "application/json; charset=utf-8", &body);
    } else if request.method == "GET" && request.path.split('?').next() == Some("/api/pet/preview")
    {
        let Some((kind, candidate)) = preview_query(&request.path) else {
            return_response(&mut stream, 400, "invalid_preview_state");
            return;
        };
        let appearance = if candidate.is_some() {
            published_appearance(state, candidate.as_deref())
        } else {
            published_pet(state)
        };
        let Some(appearance) = appearance else {
            let code = if candidate.is_some() {
                "candidate_not_found"
            } else {
                "pet_not_adopted"
            };
            return_response(&mut stream, 404, code);
            return;
        };
        respond_bytes(
            &mut stream,
            200,
            "image/png",
            &render_preview(appearance, kind),
        );
    } else {
        return_response(&mut stream, 404, "not_found");
    }
}

/// Web 领养写操作：request_id 去重 + adoption_revision 乐观锁 + 异步 operation_id。
/// 实际执行在主循环引擎线程（drain_ops → EngineAction），与 GPUI 窗口同一状态机。
fn adoption_op(
    stream: &mut TcpStream,
    request: &Request,
    state: &Arc<Mutex<PublishedState>>,
    host: &str,
) {
    if header(request, "origin").as_deref() != Some(&format!("http://{host}")) {
        return_response(stream, 403, "invalid_origin");
        return;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&request.body) else {
        return_response(stream, 400, "invalid_json");
        return;
    };
    let request_id = value
        .get("request_id")
        .and_then(|v| v.as_str())
        .filter(|id| !id.is_empty() && id.len() <= 64);
    let Some(request_id) = request_id else {
        return_response(stream, 400, "missing_request_id");
        return;
    };
    let Some(revision) = value.get("revision").and_then(|v| v.as_u64()) else {
        return_response(stream, 400, "missing_revision");
        return;
    };
    let Some(op) = parse_op(&value) else {
        return_response(stream, 400, "unknown_op");
        return;
    };
    let current = state.lock().expect("宠物控制台快照");
    let current_revision = current.adoption_revision;
    // 二阶段守卫：reset.confirm 只能在 reset_confirm 态执行（防重复/乱序触发破坏性操作）
    if matches!(op, WebOp::ResetConfirm) && !reset_pending(&current.body) {
        drop(current);
        return_response(stream, 409, "reset_not_pending");
        return;
    }
    drop(current);
    if revision != current_revision {
        return_response(stream, 409, "stale_revision");
        return;
    }
    let mut log = op_log().lock().expect("宠物控制台操作簿记");
    if let Some(existing) = log.seen.get(request_id) {
        respond(
            stream,
            200,
            "application/json; charset=utf-8",
            &format!("{{\"operation_id\":\"{existing}\",\"duplicate\":true}}"),
        );
        return;
    }
    let operation_id = random_hex();
    log.seen
        .insert(request_id.to_string(), operation_id.clone());
    log.queue.push((operation_id.clone(), op));
    respond(
        stream,
        202,
        "application/json; charset=utf-8",
        &format!("{{\"operation_id\":\"{operation_id}\"}}"),
    );
}

fn operation_status(stream: &mut TcpStream, request: &Request) {
    let Some(id) = request.path.strip_prefix("/api/operations/") else {
        return_response(stream, 404, "not_found");
        return;
    };
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return_response(stream, 400, "invalid_operation_id");
        return;
    }
    let log = op_log().lock().expect("宠物控制台操作簿记");
    let status = if log.done.contains(id) {
        "applied"
    } else if log.queue.iter().any(|(queued, _)| queued == id) {
        "pending"
    } else {
        "unknown"
    };
    respond(
        stream,
        200,
        "application/json; charset=utf-8",
        &format!("{{\"operation_id\":\"{id}\",\"status\":\"{status}\"}}"),
    );
}

fn exchange(stream: &mut TcpStream, request: &Request, host: &str, auth: &Arc<Mutex<AuthState>>) {
    if header(request, "origin").as_deref() != Some(&format!("http://{host}")) {
        return_response(stream, 403, "invalid_origin");
        return;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&request.body) else {
        return_response(stream, 400, "invalid_json");
        return;
    };
    let Some(code) = value.get("code").and_then(|v| v.as_str()) else {
        return_response(stream, 400, "missing_code");
        return;
    };
    let mut auth = auth.lock().expect("宠物控制台认证");
    let Some(session) = consume_bootstrap(&mut auth, code) else {
        return_response(stream, 401, "invalid_code");
        return;
    };
    auth.sessions.insert(session.clone());
    respond_with_cookie(
        stream,
        200,
        &format!("gesso_web_session={session}; HttpOnly; SameSite=Strict; Path=/"),
        "application/json; charset=utf-8",
        "{\"ok\":true}",
    );
}

fn consume_bootstrap(auth: &mut AuthState, code: &str) -> Option<String> {
    if auth.bootstrap.as_deref() != Some(code)
        || std::time::Instant::now() >= auth.bootstrap_expires
    {
        return None;
    }
    auth.bootstrap = None;
    Some(random_hex())
}

fn valid_session(request: &Request, auth: &Arc<Mutex<AuthState>>) -> bool {
    let Some(cookie) = header(request, "cookie") else {
        return false;
    };
    let Some(session) = cookie
        .split(';')
        .find_map(|part| part.trim().strip_prefix("gesso_web_session="))
    else {
        return false;
    };
    auth.lock()
        .expect("宠物控制台认证")
        .sessions
        .contains(session)
}

fn read_request(stream: &mut TcpStream) -> Result<Option<Request>, u16> {
    let mut bytes = Vec::with_capacity(4096);
    let header_end = loop {
        let mut chunk = [0u8; 1024];
        let n = match stream.read(&mut chunk) {
            Ok(n) => n,
            // 浏览器预连接（preconnect）会在发请求前就打开 TCP：空连接超时
            // 必须静默关闭——回 400 会被同连接上的后续真实请求读到并渲染。
            Err(_) if bytes.is_empty() => return Ok(None),
            Err(_) => return Err(400),
        };
        if n == 0 {
            return Ok(None);
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() > MAX_HEADERS + MAX_BODY {
            return Err(413);
        }
        if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = std::str::from_utf8(&bytes[..header_end]).map_err(|_| 400u16)?;
    let mut lines = head.split("\r\n");
    let mut start = lines.next().ok_or(400u16)?.split_whitespace();
    let method = start.next().ok_or(400u16)?.to_string();
    let path = start.next().ok_or(400u16)?.to_string();
    let version = start.next().ok_or(400u16)?;
    if version != "HTTP/1.0" && version != "HTTP/1.1"
        || start.next().is_some()
        || head.len() > MAX_HEADERS
    {
        return Err(431);
    }
    let mut headers = Vec::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return Err(400);
        };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let content_length = header_pairs(&headers, "content-length")
        .map(|value| value.parse::<usize>().map_err(|_| 400u16))
        .transpose()?
        .unwrap_or(0);
    if content_length > MAX_BODY {
        return Err(413);
    }
    let mut body = bytes[header_end..].to_vec();
    while body.len() < content_length {
        let mut chunk = [0u8; 1024];
        let n = stream.read(&mut chunk).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    Ok(Some(Request {
        method,
        path,
        headers,
        body,
    }))
}

fn header(request: &Request, name: &str) -> Option<String> {
    header_pairs(&request.headers, name).map(str::to_string)
}
fn header_pairs<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) {
    respond_with_cookie(stream, status, "", content_type, body);
}
fn respond_bytes(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let response = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
}

fn respond_with_cookie(
    stream: &mut TcpStream,
    status: u16,
    cookie: &str,
    content_type: &str,
    body: &str,
) {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        _ => "Bad Request",
    };
    let cookie_line = if cookie.is_empty() {
        String::new()
    } else {
        format!("Set-Cookie: {cookie}\r\n")
    };
    let response = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n{cookie_line}\r\n{body}", body.len());
    let _ = stream.write_all(response.as_bytes());
}
fn return_response(stream: &mut TcpStream, status: u16, code: &str) {
    respond(
        stream,
        status,
        "application/json; charset=utf-8",
        &format!("{{\"error\":\"{code}\"}}"),
    );
}

fn random_hex() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("系统熵源不可用");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", url])
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

const WEB_INDEX: &str = include_str!(concat!(env!("OUT_DIR"), "/web/index.html"));
const WEB_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/web/assets/app.css"));
const WEB_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/web/assets/app.js"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behavior_names_are_stable() {
        assert_eq!(behavior_name(PetBehaviorState::Sleeping), "sleeping");
        assert_eq!(behavior_name(PetBehaviorState::Touched), "touched");
    }

    #[test]
    fn snapshot_does_not_expose_seed_or_paths() {
        let snapshot = snapshot_json(
            &PetSave::default(),
            PetBehaviorState::Idle,
            &AdoptionState::NotAdopted,
        )
        .0
        .to_string();
        assert!(!snapshot.contains("root_seed"));
        assert!(!snapshot.contains("/Users/"));
    }

    #[test]
    fn candidate_dto_does_not_leak_session_or_generator_fields() {
        let started = gesso_core::adoption_transition(
            AdoptionState::NotAdopted,
            gesso_core::AdoptionEvent::Start {
                species: gesso_core::PetSpecies::Candle,
                session_seed: gesso_core::PetSeed::from_bytes([9; 16]),
                generator_version: gesso_core::GENERATOR_VERSION,
                asset_version: gesso_core::ASSET_VERSION,
            },
        );
        let snapshot = snapshot_json(&PetSave::default(), PetBehaviorState::Idle, &started)
            .0
            .to_string();
        for leaked in [
            "root_seed",
            "session_seed",
            "generator_version",
            "asset_version",
            "/Users/",
        ] {
            assert!(!snapshot.contains(leaked), "泄露字段：{leaked}");
        }
        assert!(snapshot.contains("candidates"));
    }

    #[test]
    fn bootstrap_code_is_single_use() {
        let mut auth = AuthState {
            bootstrap: Some("test-code".to_string()),
            bootstrap_expires: std::time::Instant::now() + BOOTSTRAP_TTL,
            sessions: HashSet::new(),
        };
        assert!(consume_bootstrap(&mut auth, "test-code").is_some());
        assert!(consume_bootstrap(&mut auth, "test-code").is_none());
    }

    #[test]
    fn revision_changes_only_when_public_snapshot_changes() {
        let mut state = PublishedState {
            revision: 0,
            adoption_revision: 0,
            body: snapshot_json(
                &PetSave::default(),
                PetBehaviorState::Idle,
                &AdoptionState::NotAdopted,
            )
            .0,
            preview: None,
        };
        // 同内容重复 publish：两个 revision 都不动
        let (same_body, _) = snapshot_json(
            &PetSave::default(),
            PetBehaviorState::Idle,
            &AdoptionState::NotAdopted,
        );
        apply_publish(&mut state, same_body, None);
        assert_eq!(state.revision, 0);
        assert_eq!(state.adoption_revision, 0);

        // 未领养时行为字段不在公开快照内（pet = null）：内容未变，两个 revision 都不动
        let (behavior_body, _) = snapshot_json(
            &PetSave::default(),
            PetBehaviorState::Sleeping,
            &AdoptionState::NotAdopted,
        );
        apply_publish(&mut state, behavior_body, None);
        assert_eq!(state.revision, 0);
        assert_eq!(state.adoption_revision, 0);

        // 领养状态变化：两个都动
        let started = gesso_core::adoption_transition(
            AdoptionState::NotAdopted,
            gesso_core::AdoptionEvent::Start {
                species: gesso_core::PetSpecies::Candle,
                session_seed: gesso_core::PetSeed::from_bytes([9; 16]),
                generator_version: gesso_core::GENERATOR_VERSION,
                asset_version: gesso_core::ASSET_VERSION,
            },
        );
        let (adoption_body, _) =
            snapshot_json(&PetSave::default(), PetBehaviorState::Idle, &started);
        apply_publish(&mut state, adoption_body, None);
        assert_eq!(state.revision, 1);
        assert_eq!(state.adoption_revision, 1);
    }

    #[test]
    fn parse_op_maps_known_ops_and_rejects_the_rest() {
        assert_eq!(
            parse_op(&json!({"op": "adoption.start"})),
            Some(WebOp::AdoptionStart)
        );
        assert_eq!(
            parse_op(&json!({"op": "adoption.select", "index": 2})),
            Some(WebOp::AdoptionSelect(2))
        );
        assert_eq!(parse_op(&json!({"op": "adoption.select"})), None);
        assert_eq!(
            parse_op(&json!({"op": "adoption.select", "index": 99})),
            None
        );
        assert_eq!(parse_op(&json!({"op": "pet.grant_admin"})), None);
        assert_eq!(parse_op(&json!({})), None);
        assert_eq!(
            parse_op(&json!({"op": "pet.set_visible", "visible": true})),
            Some(WebOp::SetVisible(true))
        );
        assert_eq!(parse_op(&json!({"op": "pet.set_visible"})), None);
        assert_eq!(
            parse_op(&json!({"op": "pet.set_visible", "visible": "yes"})),
            None
        );
        assert_eq!(
            parse_op(&json!({"op": "pet.set_size", "size": "large"})),
            Some(WebOp::SetSize(PetSize::Large))
        );
        assert_eq!(
            parse_op(&json!({"op": "pet.set_size", "size": "huge"})),
            None
        );
        assert_eq!(
            parse_op(&json!({"op": "pet.set_activity", "activity": "lively"})),
            Some(WebOp::SetActivity(PetActivity::Lively))
        );
        assert_eq!(
            parse_op(&json!({"op": "pet.reset.request"})),
            Some(WebOp::ResetRequest)
        );
        assert_eq!(
            parse_op(&json!({"op": "pet.reset.confirm"})),
            Some(WebOp::ResetConfirm)
        );
        assert_eq!(
            parse_op(&json!({"op": "pet.reset.cancel"})),
            Some(WebOp::ResetCancel)
        );
    }

    #[test]
    fn reset_confirm_only_when_pending() {
        let mut body = json!({"adoption": {"status": "adopted"}});
        assert!(!reset_pending(&body));
        body = json!({"adoption": {"status": "reset_confirm"}});
        assert!(reset_pending(&body));
        body = json!({"adoption": {}});
        assert!(!reset_pending(&body));
    }

    #[test]
    fn preview_query_is_strict_and_png_is_well_formed() {
        assert_eq!(
            preview_query("/api/pet/preview?state=idle"),
            Some((FrameKind::Idle, None))
        );
        assert_eq!(
            preview_query("/api/pet/preview?state=blink"),
            Some((FrameKind::Blink, None))
        );
        let cid = "abc123";
        assert_eq!(
            preview_query(&format!("/api/pet/preview?state=sleep&candidate={cid}")),
            Some((FrameKind::Sleep, Some(cid.to_string())))
        );
        assert_eq!(preview_query("/api/pet/preview?state=idle&size=999"), None);
        assert_eq!(preview_query("/api/pet/preview?state=unknown"), None);
        assert_eq!(
            preview_query("/api/pet/preview?state=idle&candidate="),
            None
        );
        assert_eq!(
            preview_query("/api/pet/preview?state=idle&candidate=x y"),
            None
        );
        assert_eq!(
            preview_query("/api/pet/preview?state=idle&candidate=ab&candidate=cd"),
            None
        );

        let png = render_preview(
            PreviewAppearance {
                coat: 0,
                accent: 0,
                marking: 0,
                body: 0,
            },
            FrameKind::Idle,
        );
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 25);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 50);
    }

    #[test]
    fn candidate_appearance_is_resolved_from_published_snapshot() {
        let started = gesso_core::adoption_transition(
            AdoptionState::NotAdopted,
            gesso_core::AdoptionEvent::Start {
                species: gesso_core::PetSpecies::Candle,
                session_seed: gesso_core::PetSeed::from_bytes([9; 16]),
                generator_version: gesso_core::GENERATOR_VERSION,
                asset_version: gesso_core::ASSET_VERSION,
            },
        );
        let (body, _) = snapshot_json(&PetSave::default(), PetBehaviorState::Idle, &started);
        let state = Arc::new(Mutex::new(PublishedState {
            revision: 0,
            adoption_revision: 0,
            body,
            preview: None,
        }));
        let candidates = state
            .lock()
            .unwrap()
            .body
            .get("adoption")
            .unwrap()
            .get("candidates")
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        let cid = candidates[0].get("candidate_id").unwrap().as_str().unwrap();
        // 有效 candidate_id → 渲染成功；未知 id → None
        let found = published_appearance(&state, Some(cid));
        assert!(found.is_some());
        let png = render_preview(found.unwrap(), FrameKind::Idle);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(published_appearance(&state, Some("ffffffff")).is_none());
    }
}
