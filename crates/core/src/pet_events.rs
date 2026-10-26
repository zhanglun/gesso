//! 宠物助手事件接口——线协议（S1：常量 + 行解析，纯逻辑零平台依赖）。
//!
//! 线格式已冻结（《宠物助手事件接口选型.md》v1.0）：一行 JSON + `\n`，
//! 请求 `{"v":1,"kind":"new_result","src":"my-agent"}`，响应 `{"ok":true}`
//! 或 `{"err":"..."}`。type-not-text 是冻结约束：本解析器**拒绝未知字段**，
//! 任何携带 text 的行在此即被拒（纵深防御，不依赖发送方自律）。

use serde_json::Value;

use crate::PetNotice;

/// 线协议版本；服务端只接受 1。
pub const PROTOCOL_VERSION: u32 = 1;
/// 单行字节上限——读侧超限即断连（512 字节足够 `{v,kind,src}`）。
pub const MAX_LINE_BYTES: usize = 512;
/// `src` 自我声明名的长度上限（按字符计，CJK 名不占便宜不吃亏）。
pub const MAX_SRC_CHARS: usize = 64;

/// 成功响应行（含行尾换行）。
pub const OK_LINE: &str = "{\"ok\":true}\n";

/// Windows 传输端点：命名管道路径（集成方契约，冻结；服务端/CLI 共用此单源）。
pub const PIPE_PATH: &str = "\\\\.\\pipe\\gesso-pet-events";
/// macOS 传输端点：UDS 文件名，挂每用户临时目录下（集成方契约，冻结）。
pub const SOCKET_FILE_NAME: &str = "gesso-pet-events.sock";

/// 请求被拒的原因；`err_code()` 即线上 `err` 字段的冻结字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PetEventReject {
    /// JSON 不合法 / 非对象 / 字段缺失或类型错 / 未知字段 / 版本非 1 / src 非法。
    BadRequest,
    /// kind 是字符串但不在三个冻结档位内。
    UnknownKind,
    /// 源未在授权名单（S3 名单层产生，解析器不产生）。
    Unauthorized,
    /// 连接级限速触发（S2 监听层产生，随即断连）。
    RateLimited,
}

impl PetEventReject {
    pub fn err_code(self) -> &'static str {
        match self {
            PetEventReject::BadRequest => "bad-request",
            PetEventReject::UnknownKind => "unknown-kind",
            PetEventReject::Unauthorized => "unauthorized",
            PetEventReject::RateLimited => "rate-limited",
        }
    }

    /// 拒绝响应行，如 `{"err":"unknown-kind"}\n`。
    pub fn to_line(self) -> String {
        format!("{{\"err\":\"{}\"}}\n", self.err_code())
    }
}

/// 合法请求行解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PetEventRequest {
    pub kind: PetNotice,
    pub src: String,
}

impl PetEventRequest {
    /// 请求行（S4 CLI 复用）：`{"v":1,"kind":"...","src":"..."}\n`。
    pub fn to_line(&self) -> String {
        let kind = match self.kind {
            PetNotice::NewResult => "new_result",
            PetNotice::NeedsAttention => "needs_attention",
            PetNotice::Warning => "warning",
        };
        format!(
            "{{\"v\":{},\"kind\":\"{}\",\"src\":\"{}\"}}\n",
            PROTOCOL_VERSION, kind, self.src
        )
    }
}

/// 解析一行请求。行尾单个 `\r` 容忍（CRLF 行尾归一化）；其余一律严格——
/// 未知字段、错误类型、多字节内容都拒绝而不是忽略（"不解析不猜"）。
pub fn parse_line(line: &str) -> Result<PetEventRequest, PetEventReject> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let v: Value = serde_json::from_str(line).map_err(|_| PetEventReject::BadRequest)?;
    let obj = v.as_object().ok_or(PetEventReject::BadRequest)?;
    // 恰好三个冻结字段：多一个（如偷带 text）即拒。
    if obj.len() != 3 {
        return Err(PetEventReject::BadRequest);
    }
    match obj.get("v").and_then(Value::as_u64) {
        Some(n) if n == u64::from(PROTOCOL_VERSION) => {}
        _ => return Err(PetEventReject::BadRequest),
    }
    let kind_str = obj
        .get("kind")
        .and_then(Value::as_str)
        .ok_or(PetEventReject::BadRequest)?;
    let kind = match kind_str {
        "new_result" => PetNotice::NewResult,
        "needs_attention" => PetNotice::NeedsAttention,
        "warning" => PetNotice::Warning,
        _ => return Err(PetEventReject::UnknownKind),
    };
    let src = obj
        .get("src")
        .and_then(Value::as_str)
        .ok_or(PetEventReject::BadRequest)?;
    if src.is_empty() || src.chars().count() > MAX_SRC_CHARS || src.chars().any(char::is_control) {
        return Err(PetEventReject::BadRequest);
    }
    Ok(PetEventRequest {
        kind,
        src: src.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_req(kind: PetNotice, src: &str) -> Result<PetEventRequest, PetEventReject> {
        parse_line(
            &PetEventRequest {
                kind,
                src: src.to_string(),
            }
            .to_line(),
        )
    }

    #[test]
    fn parses_all_three_kinds_round_trip() {
        for kind in [
            PetNotice::NewResult,
            PetNotice::NeedsAttention,
            PetNotice::Warning,
        ] {
            assert_eq!(
                ok_req(kind, "my-agent"),
                Ok(PetEventRequest {
                    kind,
                    src: "my-agent".to_string()
                })
            );
        }
    }

    #[test]
    fn line_shapes_match_frozen_format() {
        assert_eq!(OK_LINE, "{\"ok\":true}\n");
        assert_eq!(
            PetEventRequest {
                kind: PetNotice::Warning,
                src: "a".to_string()
            }
            .to_line(),
            "{\"v\":1,\"kind\":\"warning\",\"src\":\"a\"}\n"
        );
        for r in [
            PetEventReject::BadRequest,
            PetEventReject::UnknownKind,
            PetEventReject::Unauthorized,
            PetEventReject::RateLimited,
        ] {
            assert_eq!(r.to_line(), format!("{{\"err\":\"{}\"}}\n", r.err_code()));
        }
    }

    #[test]
    fn rejects_malformed_and_non_object_lines() {
        for bad in ["{", "[]", "\"x\"", "123", "null", "", "   "] {
            assert_eq!(parse_line(bad), Err(PetEventReject::BadRequest), "{bad:?}");
        }
    }

    #[test]
    fn rejects_wrong_version_shapes() {
        for bad in [
            r#"{"kind":"new_result","src":"a"}"#,         // 缺 v
            r#"{"v":0,"kind":"new_result","src":"a"}"#,   // 旧版本
            r#"{"v":2,"kind":"new_result","src":"a"}"#,   // 未来版本
            r#"{"v":"1","kind":"new_result","src":"a"}"#, // 类型错
            r#"{"v":1.5,"kind":"new_result","src":"a"}"#, // 非 u64
        ] {
            assert_eq!(parse_line(bad), Err(PetEventReject::BadRequest), "{bad:?}");
        }
    }

    #[test]
    fn rejects_unknown_or_mistyped_kind() {
        assert_eq!(
            parse_line(r#"{"v":1,"kind":"task_done","src":"a"}"#),
            Err(PetEventReject::UnknownKind)
        );
        // kind 非字符串是形态错，不是未知档位
        assert_eq!(
            parse_line(r#"{"v":1,"kind":123,"src":"a"}"#),
            Err(PetEventReject::BadRequest)
        );
    }

    #[test]
    fn rejects_text_smuggling_via_extra_field() {
        // type-not-text 纵深：携带任意文本字段的行在此即拒，不看内容
        assert_eq!(
            parse_line(r#"{"v":1,"kind":"new_result","src":"a","text":"hi"}"#),
            Err(PetEventReject::BadRequest)
        );
        // 少字段同样拒
        assert_eq!(
            parse_line(r#"{"v":1,"kind":"new_result"}"#),
            Err(PetEventReject::BadRequest)
        );
    }

    #[test]
    fn rejects_bad_src() {
        // 缺失 / 空串 / 非字符串 / 超 64 字符 / 控制字符
        assert_eq!(
            parse_line(r#"{"v":1,"kind":"new_result"}"#),
            Err(PetEventReject::BadRequest)
        );
        assert_eq!(
            parse_line(r#"{"v":1,"kind":"new_result","src":""}"#),
            Err(PetEventReject::BadRequest)
        );
        assert_eq!(
            parse_line(r#"{"v":1,"kind":"new_result","src":7}"#),
            Err(PetEventReject::BadRequest)
        );
        let long = "x".repeat(MAX_SRC_CHARS + 1);
        assert_eq!(
            parse_line(&format!(r#"{{"v":1,"kind":"new_result","src":"{long}"}}"#)),
            Err(PetEventReject::BadRequest)
        );
        assert_eq!(
            parse_line(r#"{"v":1,"kind":"new_result","src":"a\nb"}"#),
            Err(PetEventReject::BadRequest)
        );
    }

    #[test]
    fn tolerates_crlf_and_max_boundary() {
        assert_eq!(
            parse_line("{\"v\":1,\"kind\":\"warning\",\"src\":\"a\"}\r"),
            Ok(PetEventRequest {
                kind: PetNotice::Warning,
                src: "a".to_string()
            })
        );
        let max = "x".repeat(MAX_SRC_CHARS);
        assert!(parse_line(&format!(r#"{{"v":1,"kind":"new_result","src":"{max}"}}"#)).is_ok());
        // CJK 名按字符计
        assert!(parse_line(r#"{"v":1,"kind":"new_result","src":"我的助手"}"#).is_ok());
    }
}
