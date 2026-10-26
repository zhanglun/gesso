//! 宠物助手事件监听（P3-S2）：本地传输 → S1 线协议解析 → 引擎动作入队。
//!
//! 平台缝只有「接受连接 + 读写原始行」（`pet_events_windows` / `pet_events_unix`）；
//! 协议与档位语义全部在 `gesso_core::pet_events`（S1，冻结）。
//! S3 将在本模块的 AUTH 门位插入 opt-in 名单校验（parse 之后、enqueue 之前）。
//!
//! 信任模型（选型文档 §2）：同用户进程可连已接受（S3 收口），跨用户由传输层挡——
//! Windows 管道默认 DACL 仅创建者用户/管理员/SYSTEM，macOS UDS 文件 0600。

use std::collections::BTreeSet;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use gesso_core::pet_events::PetEventReject;

use crate::engine;

// 传输端点是冻结的集成方契约，单一事实源在 core（服务端/CLI 共用）。
#[cfg(target_os = "windows")]
pub use gesso_core::pet_events::PIPE_PATH as PIPE_NAME;
#[cfg(target_os = "macos")]
pub use gesso_core::pet_events::SOCKET_FILE_NAME as SOCKET_FILE;

/// 首连审批等待时长：超时视为拒绝（客户端收 unauthorized）。
/// debug 可用 GESSO_PET_APPROVAL_TIMEOUT_SECS 覆盖（无头冒烟用短超时）。
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(90);

/// 授权名单镜像（引擎真源在 `settings.pet_event_sources`，本镜像仅供监听线程
/// 无锁读取；由 spawn/sync/审批/撤销/重置各点同步）。
static ALLOWED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// 待审批槽：同一时刻至多一个首连确认窗（macOS 并发连接的第二个未知源
/// 直接收 unauthorized，客户端稍后重试即可）。
static PENDING: Mutex<Option<PendingApproval>> = Mutex::new(None);

struct PendingApproval {
    src: String,
    tx: mpsc::Sender<bool>,
}

/// 鉴权结论。
enum AuthResult {
    /// 名单内，直接放行。
    Allowed,
    /// 已发起首连审批（弹窗已请求，等待用户结论）。
    ApprovalStarted(mpsc::Receiver<bool>),
    /// 已有待审批窗，本次直接拒绝（客户端稍后重试）。
    Busy,
}

fn authorize(src: &str) -> AuthResult {
    if allowed(src) {
        return AuthResult::Allowed;
    }
    let (tx, rx) = mpsc::channel();
    let mut slot = PENDING.lock().expect("pending poisoned");
    if slot.is_some() {
        return AuthResult::Busy;
    }
    *slot = Some(PendingApproval {
        src: src.to_string(),
        tx,
    });
    drop(slot);
    engine::enqueue(engine::EngineAction::PetApprovalNeeded {
        src: src.to_string(),
    });
    AuthResult::ApprovalStarted(rx)
}

fn allowed(src: &str) -> bool {
    ALLOWED.lock().expect("allowed poisoned").contains(src)
}

/// 单连接行处理：解析 → 鉴权 → 入队 → 返回**完整响应行（含行尾 `\n`）**。
/// 响应是面向客户端的线契约——必须带换行（S2 冒烟曾因 trim 掉 `\n` 卡死客户端）。
/// 未授权源走首连审批：弹窗结论（批准）放行本次并加入名单；拒绝/超时/审批窗
/// 占用（Busy）一律 `{"err":"unauthorized"}`。
pub fn process_line(line: &str) -> String {
    process_line_for(line, approval_timeout())
}

fn process_line_for(line: &str, timeout: Duration) -> String {
    match gesso_core::pet_events::parse_line(line) {
        Ok(req) => match authorize(&req.src) {
            AuthResult::Allowed => deliver(&req.kind, &req.src),
            AuthResult::ApprovalStarted(rx) => match rx.recv_timeout(timeout) {
                Ok(true) => deliver(&req.kind, &req.src),
                // 拒绝 / 超时；超时顺带清槽（还挂着的话），后续连接可重新发起审批
                Ok(false) | Err(_) => {
                    let mut slot = PENDING.lock().expect("pending poisoned");
                    if slot.as_ref().is_some_and(|p| p.src == req.src) {
                        // 发送端已随槽回收；持有 recv 端的等待者已醒，不会再收
                        let _ = slot.take();
                    }
                    PetEventReject::Unauthorized.to_line()
                }
            },
            AuthResult::Busy => PetEventReject::Unauthorized.to_line(),
        },
        Err(reject) => reject.to_line(),
    }
}

/// 放行：入队宠物气泡 + 回 OK 行。AUTH 门位——parse 之后、enqueue 之前。
fn deliver(kind: &gesso_core::PetNotice, src: &str) -> String {
    engine::enqueue(engine::EngineAction::PetBehavior(
        gesso_core::PetBehaviorEvent::Notify(*kind),
    ));
    println!("[pet-events] 事件入队 kind={kind:?} src={src}");
    gesso_core::pet_events::OK_LINE.to_string()
}

fn approval_timeout() -> Duration {
    #[cfg(debug_assertions)]
    if let Ok(secs) = std::env::var("GESSO_PET_APPROVAL_TIMEOUT_SECS") {
        if let Ok(n) = secs.parse::<u64>() {
            return Duration::from_secs(n);
        }
    }
    APPROVAL_TIMEOUT
}

/// 引擎真源 → 监听侧镜像（启动/审批/撤销/重置各点调用）。
pub fn sync_allowed(sources: &[String]) {
    *ALLOWED.lock().expect("allowed poisoned") = sources.iter().cloned().collect();
}

/// 审批窗结论回灌（引擎 PetApprovalDecision 臂调用）：唤醒等待中的监听线程。
/// 槽已被超时清掉时静默丢弃（客户端早已收到 unauthorized）。
pub fn resolve_pending(src: &str, approved: bool) {
    let waiter = PENDING
        .lock()
        .expect("pending poisoned")
        .take_if(|p| p.src == src);
    if let Some(p) = waiter {
        let _ = p.tx.send(approved);
    }
}

/// 当前待审批的源名（确认窗渲染用）；无待审批时为 None。
pub fn pending_src() -> Option<String> {
    PENDING
        .lock()
        .expect("pending poisoned")
        .as_ref()
        .map(|p| p.src.clone())
}

/// 启动监听线程；失败只留日志（功能缺失不致命，应用其余部分照常）。
pub fn spawn_listener(initial_allowed: &[String]) {
    sync_allowed(initial_allowed);
    #[cfg(target_os = "windows")]
    let io = super::pet_events_windows::listen;
    #[cfg(target_os = "macos")]
    let io = super::pet_events_unix::listen;
    if std::thread::Builder::new()
        .name("pet-events".to_string())
        .spawn(io)
        .is_err()
    {
        println!("[pet-events] 监听线程启动失败（忽略）");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gesso_core::{PetBehaviorEvent, PetEventRequest, PetNotice};

    fn req(kind: PetNotice, src: &str) -> String {
        PetEventRequest {
            kind,
            src: src.to_string(),
        }
        .to_line()
    }

    /// 清空动作队列并断言其中没有宠物事件（PetApprovalNeeded 合成动作不算——
    /// 真应用由引擎循环消费开窗，单测里无人消费）。
    fn drained_any_pet_event() -> bool {
        engine::drain().iter().any(|a| {
            matches!(
                a,
                engine::EngineAction::PetBehavior(PetBehaviorEvent::Notify(_))
            )
        })
    }

    // 全局动作队列 + 全局审批槽无法并行断言——用例合并串行（先 drain 清场）。
    #[test]
    fn auth_gate_contract() {
        engine::drain();

        // 1) 名单内：直接放行
        sync_allowed(&["ci-agent".to_string()]);
        assert_eq!(
            process_line(&req(PetNotice::Warning, "ci-agent")),
            gesso_core::pet_events::OK_LINE
        );
        assert!(matches!(
            engine::drain().last(),
            Some(engine::EngineAction::PetBehavior(PetBehaviorEvent::Notify(
                PetNotice::Warning
            )))
        ));

        // 2) 未知源：发起审批 → 批准 → 放行（镜像同步由引擎臂负责，单测外）
        let solver = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(50));
            resolve_pending("new-agent", true);
        });
        let resp = process_line_for(
            &req(PetNotice::NewResult, "new-agent"),
            Duration::from_secs(5),
        );
        solver.join().unwrap();
        assert_eq!(resp, gesso_core::pet_events::OK_LINE);
        assert!(matches!(
            engine::drain().last(),
            Some(engine::EngineAction::PetBehavior(PetBehaviorEvent::Notify(
                PetNotice::NewResult
            )))
        ));

        // 3) 未知源：超时 → unauthorized，且槽被清理可再次发起。
        //    PetApprovalNeeded 合成动作入队是预期（真应用由引擎循环消费开窗，
        //    单测无人消费）——断言的是「没有宠物事件入队」，不是队列全空。
        sync_allowed(&[]);
        let resp = process_line_for(
            &req(PetNotice::NewResult, "slow-agent"),
            Duration::from_millis(30),
        );
        assert_eq!(resp, "{\"err\":\"unauthorized\"}\n");
        assert!(!drained_any_pet_event());
        assert!(pending_src().is_none());

        // 4) 审批窗占用：第二个未知源直接拒绝（Busy），不排队
        let first = std::thread::spawn(|| {
            process_line_for(&req(PetNotice::Warning, "first"), Duration::from_secs(10))
        });
        std::thread::sleep(Duration::from_millis(80)); // 等 first 进入等待（槽已占）
        assert_eq!(
            process_line_for(
                &req(PetNotice::NewResult, "second"),
                Duration::from_millis(200)
            ),
            "{\"err\":\"unauthorized\"}\n"
        );
        resolve_pending("first", false);
        assert_eq!(first.join().unwrap(), "{\"err\":\"unauthorized\"}\n");
        assert!(!drained_any_pet_event());

        // 5) 坏行不受鉴权影响
        sync_allowed(&["ci-agent".to_string()]);
        assert_eq!(
            process_line(r#"{"v":1,"kind":"new_result","src":"a","text":"hi"}"#),
            "{\"err\":\"bad-request\"}\n"
        );
        assert!(engine::drain().is_empty());
        assert!(pending_src().is_none());
    }
}
