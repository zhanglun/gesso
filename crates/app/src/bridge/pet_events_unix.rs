//! macOS 传输：Unix domain socket（每用户临时目录下 `gesso-pet-events.sock`）。
//!
//! macOS 的 `TMPDIR` 每用户隔离，再叠文件权限 0600 双保险（L1 落到文件系统）。
//! 客户端可一行一连接，也可持有连接连发多行（每行必有响应，读超时 30s 兜底）。
//! 崩溃残留的 socket 文件在绑定时清理；`GESSO_LOCK=dev` 双开是开发场景，
//! 后绑者接管文件、先绑者静默失联（Windows 侧用 FIRST_PIPE_INSTANCE 反向处理）。

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::Duration;

use super::pet_events;
use gesso_core::pet_events::{PetEventReject, MAX_LINE_BYTES};

pub fn listen() {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            println!("[pet-events] UDS 绑定失败（{}）：{e}", path.display());
            return;
        }
    };
    if let Ok(meta) = std::fs::metadata(&path) {
        let mut perm = meta.permissions();
        perm.set_mode(0o600);
        let _ = std::fs::set_permissions(&path, perm);
    }
    println!("[pet-events] UDS 监听 {}", path.display());
    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            std::thread::spawn(move || serve(stream));
        }
    }
}

fn socket_path() -> PathBuf {
    std::env::temp_dir().join(pet_events::SOCKET_FILE)
}

fn serve(mut stream: std::os::unix::net::UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut buf = [0u8; 1024];
    let mut acc: Vec<u8> = Vec::new();
    loop {
        let read = match stream.read(&mut buf) {
            Ok(0) | Err(_) => return, // EOF / 断开 / 超时
            Ok(n) => n,
        };
        acc.extend_from_slice(&buf[..read]);
        while let Some(pos) = acc.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = acc.drain(..=pos).collect();
            let resp = match std::str::from_utf8(&line[..pos]) {
                Ok(s) if s.len() <= MAX_LINE_BYTES => pet_events::process_line(s),
                _ => PetEventReject::BadRequest.to_line(),
            };
            if stream.write_all(resp.as_bytes()).is_err() {
                return;
            }
        }
        if acc.len() > MAX_LINE_BYTES {
            let _ = stream.write_all(PetEventReject::BadRequest.to_line().as_bytes());
            return;
        }
    }
}
