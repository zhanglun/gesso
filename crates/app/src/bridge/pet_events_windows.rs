//! Windows 传输：命名管道（`\\.\pipe\gesso-pet-events`）。
//!
//! 每个实例循环「创建管道实例 → ConnectNamedPipe → 逐行处理 → 断开」；
//! 客户端可一行一连接，也可持有连接连发多行（每行必有响应）。
//! `FILE_FLAG_FIRST_PIPE_INSTANCE` 保证监听权单实例持有：`GESSO_LOCK=dev`
//! 双开时第二实例创建失败即静默退出（事件归属第一实例，不抢）。

use windows::core::{w, HRESULT};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, HANDLE};
use windows::Win32::Storage::FileSystem::{
    ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

use super::pet_events::{self, PIPE_NAME};
use gesso_core::pet_events::{PetEventReject, MAX_LINE_BYTES};

pub fn listen() {
    loop {
        let pipe = unsafe {
            CreateNamedPipeW(
                w!(r"\\.\pipe\gesso-pet-events"),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                1024,
                1024,
                0,
                None,
            )
        };
        if pipe.is_invalid() {
            println!(
                "[pet-events] 管道创建失败（多半已有实例持有监听，本实例不参与）：{:?}",
                unsafe { GetLastError() }
            );
            return;
        }
        unsafe {
            // 客户端在 Create 与 Connect 之间连入 → ERROR_PIPE_CONNECTED，视作已连
            if let Err(e) = ConnectNamedPipe(pipe, None) {
                if e.code() != HRESULT::from(ERROR_PIPE_CONNECTED) {
                    let _ = CloseHandle(pipe);
                    continue;
                }
            }
        }
        serve(pipe);
        unsafe {
            let _ = DisconnectNamedPipe(pipe);
            let _ = CloseHandle(pipe);
        }
    }
}

/// 已连接实例上的逐行服务；客户端断开/出错即返回（实例由调用方回收）。
fn serve(pipe: HANDLE) {
    let mut buf = [0u8; 1024];
    let mut acc: Vec<u8> = Vec::new();
    loop {
        let mut read = 0u32;
        if unsafe { ReadFile(pipe, Some(&mut buf), Some(&mut read), None) }.is_err() || read == 0 {
            return; // EOF / 断开 / 出错
        }
        acc.extend_from_slice(&buf[..read as usize]);
        while let Some(pos) = acc.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = acc.drain(..=pos).collect();
            let resp = match std::str::from_utf8(&line[..pos]) {
                Ok(s) if s.len() <= MAX_LINE_BYTES => pet_events::process_line(s),
                _ => PetEventReject::BadRequest.to_line(),
            };
            if write_line(pipe, resp.as_bytes()).is_err() {
                return;
            }
        }
        if acc.len() > MAX_LINE_BYTES {
            // 不带换行的超长行：回 bad-request 后断开（读侧上限，选型文档 §5）
            let _ = write_line(pipe, PetEventReject::BadRequest.to_line().as_bytes());
            return;
        }
    }
}

fn write_line(pipe: HANDLE, bytes: &[u8]) -> Result<(), ()> {
    let mut written = 0u32;
    match unsafe { WriteFile(pipe, Some(bytes), Some(&mut written), None) } {
        Ok(()) if written as usize == bytes.len() => Ok(()),
        _ => Err(()),
    }
}

// PIPE_NAME 仅用于文档/调试（w! 宏直嵌名字）；保留导出避免 cfg 双平台漂移。
#[allow(unused)]
pub fn name() -> &'static str {
    PIPE_NAME
}
