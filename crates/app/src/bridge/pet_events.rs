//! 宠物助手事件监听（P3-S2）：本地传输 → S1 线协议解析 → 引擎动作入队。
//!
//! 平台缝只有「接受连接 + 读写原始行」（`pet_events_windows` / `pet_events_unix`）；
//! 协议与档位语义全部在 `gesso_core::pet_events`（S1，冻结）。
//! S3 将在本模块的 AUTH 门位插入 opt-in 名单校验（parse 之后、enqueue 之前）。
//!
//! 信任模型（选型文档 §2）：同用户进程可连已接受（S3 收口），跨用户由传输层挡——
//! Windows 管道默认 DACL 仅创建者用户/管理员/SYSTEM，macOS UDS 文件 0600。

use crate::engine;

/// Windows 命名管道名（集成方契约，冻结）。
#[cfg(target_os = "windows")]
pub const PIPE_NAME: &str = r"\\.\pipe\gesso-pet-events";

/// macOS UDS 套接字文件名（挂在每用户临时目录下，集成方契约，冻结）。
#[cfg(target_os = "macos")]
pub const SOCKET_FILE: &str = "gesso-pet-events.sock";

/// 单连接行处理：解析 → 入队 → 返回**完整响应行（含行尾 `\n`）**。
/// 响应是面向客户端的线契约——必须带换行（S2 冒烟曾因 trim 掉 `\n` 卡死客户端）。
/// 恰好一进一出：每行必有响应；连接级限速（行处理之外，S3 一并落）。
pub fn process_line(line: &str) -> String {
    match gesso_core::pet_events::parse_line(line) {
        Ok(req) => {
            // AUTH 门位（S3）：opt-in 名单校验——parse 之后、enqueue 之前。
            engine::enqueue(engine::EngineAction::PetBehavior(
                gesso_core::PetBehaviorEvent::Notify(req.kind),
            ));
            println!("[pet-events] 事件入队 kind={:?} src={}", req.kind, req.src);
            gesso_core::pet_events::OK_LINE.to_string()
        }
        Err(reject) => {
            println!("[pet-events] 拒绝 {}", reject.err_code());
            reject.to_line()
        }
    }
}

/// 启动监听线程；失败只留日志（功能缺失不致命，应用其余部分照常）。
pub fn spawn_listener() {
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

    // 全局动作队列无法并行断言——两个用例合并串行（先 drain 清场）。
    #[test]
    fn line_processing_contract() {
        engine::drain();
        let resp = process_line(
            &PetEventRequest {
                kind: PetNotice::Warning,
                src: "ci-agent".to_string(),
            }
            .to_line(),
        );
        assert_eq!(resp, gesso_core::pet_events::OK_LINE);
        let drained = engine::drain();
        assert!(matches!(
            drained.last(),
            Some(engine::EngineAction::PetBehavior(PetBehaviorEvent::Notify(
                PetNotice::Warning
            )))
        ));

        let resp = process_line(r#"{"v":1,"kind":"new_result","src":"a","text":"hi"}"#);
        assert_eq!(resp, "{\"err\":\"bad-request\"}\n");
        assert!(engine::drain().is_empty());
    }
}
