//! gesso-pet-notify：给桌面宠物发一行事件的 CLI（选型文档 §7.3，随 v1 交付）。
//!
//! 用法：`gesso-pet-notify <new_result|needs_attention|warning> [--src <名>]`
//!
//! 连接本地管道（Win）/ UDS（mac），发一行 JSON，读一行响应：
//! - 退出码 0 = `{"ok":true}`；2 = `unauthorized`（源未授权，批准后重试即成）；
//!   1 = 其余错误（连接失败 / bad-request / unknown-kind）。
//!
//! 首连未授权时会弹出 Gesso 的审批窗，本命令将阻塞至用户决定或超时（默认 90s）。

use std::io::{Read, Write};
use std::process::ExitCode;

#[cfg(target_os = "windows")]
use gesso_core::pet_events::{self, PetEventReject, PIPE_PATH};
#[cfg(target_os = "macos")]
use gesso_core::pet_events::{self, PetEventReject, SOCKET_FILE_NAME};

const KINDS: [&str; 3] = ["new_result", "needs_attention", "warning"];
const EXIT_OK: u8 = 0;
const EXIT_ERROR: u8 = 1;
const EXIT_UNAUTHORIZED: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut kind_str = None;
    let mut src = String::from("gesso-pet-notify");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--src" | "-s" => match args.get(i + 1) {
                Some(v) => {
                    src = v.clone();
                    i += 1;
                }
                None => return fail("--src 需要一个值"),
            },
            "--help" | "-h" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            v => kind_str = Some(v.to_string()),
        }
        i += 1;
    }
    let Some(kind_str) = kind_str else {
        return fail("缺少事件档位；--help 查看用法");
    };
    if !KINDS.contains(&kind_str.as_str()) {
        return fail(&format!("未知档位 {kind_str:?}；可选：{KINDS:?}"));
    }

    let kind = match kind_str.as_str() {
        "new_result" => gesso_core::PetNotice::NewResult,
        "needs_attention" => gesso_core::PetNotice::NeedsAttention,
        _ => gesso_core::PetNotice::Warning,
    };
    let line = pet_events::PetEventRequest { kind, src }.to_line();

    let mut conn = match connect() {
        Ok(c) => c,
        Err(e) => return fail(&format!("连接失败（Gesso 在运行吗？）：{e}")),
    };
    if let Err(e) = conn.write_all(line.as_bytes()) {
        return fail(&format!("发送失败：{e}"));
    }
    let resp = match read_line(&mut conn) {
        Some(r) => r,
        None => return fail("未收到响应（连接中断）"),
    };
    if resp == pet_events::OK_LINE.trim_end() {
        println!("ok");
        return ExitCode::from(EXIT_OK);
    }
    let err = resp
        .strip_prefix("{\"err\":\"")
        .and_then(|r| r.strip_suffix("\"}"))
        .unwrap_or("unknown-response");
    eprintln!("{err}");
    ExitCode::from(if err == PetEventReject::Unauthorized.err_code() {
        eprintln!("该源尚未授权：批准 Gesso 弹出的确认窗后重试本命令即可");
        EXIT_UNAUTHORIZED
    } else {
        EXIT_ERROR
    })
}

/// 连接本地传输端点：Windows 命名管道按文件打开（std 即可，无需 win32 绑定）；
/// macOS UDS 挂每用户临时目录（与服务端 socket_path 同规则）。
fn connect() -> std::io::Result<Box<dyn ReadWrite>> {
    #[cfg(target_os = "windows")]
    {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(PIPE_PATH)?;
        Ok(Box::new(f))
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::net::UnixStream;
        let path = std::env::temp_dir().join(SOCKET_FILE_NAME);
        let s = UnixStream::connect(path)?;
        Ok(Box::new(s))
    }
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

/// 读一行（至 `\n`）；超 MAX_LINE_BYTES 或 EOF 无行视为失败。
fn read_line(conn: &mut dyn Read) -> Option<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while buf.len() <= pet_events::MAX_LINE_BYTES {
        match conn.read(&mut byte) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {
                if byte[0] == b'\n' {
                    return Some(String::from_utf8_lossy(&buf).trim_end().to_string());
                }
                buf.push(byte[0]);
            }
        }
    }
    None
}

fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::from(EXIT_ERROR)
}

fn print_help() {
    println!(
        "用法：gesso-pet-notify <new_result|needs_attention|warning> [--src <名>]\n\
         给 Gesso 桌面宠物发一行事件（气泡由宠物角色包装，不接收文本）。\n\
         --src 自我声明名（默认 gesso-pet-notify；首连会弹 Gesso 审批窗）。"
    );
}
