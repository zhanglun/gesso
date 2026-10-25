//! 系统通知面（best-effort，§4.7 通知第 1 层）：失败只留日志，不阻塞调用方。
//! 各通知层独立、互不阻塞（界面稿 §4.7「软件更新」通知分层）；本模块只管
//! 「送到系统通知面」，不持有事件语义——事件分类与跨面去重归调用方。

/// 发送一条系统通知：macOS 走 osascript（免 bundle/签名限制，UNUserNotificationCenter
/// 需要 .app bundle，开发裸进程弹不出），Windows 走 WinRT toast（便携版未注册 AUMID
/// 时预期失败；精简系统 WinRT 运行时可能缺失）。
pub fn send(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    {
        // 文案经 -e 参数传入，不经 shell 解析；净化引号/反斜杠/换行防 AppleScript
        // 注入与语法错误（正文可能含远端来源文本，如 release tag）。
        let safe = |s: &str| {
            s.replace('\\', "")
                .replace('"', "'")
                .replace('\n', " ")
                .replace('\r', " ")
        };
        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            safe(body),
            safe(title)
        );
        if let Err(e) = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .spawn()
        {
            println!("[notify] 系统通知发送失败（忽略）：{e}");
        }
    }
    #[cfg(target_os = "windows")]
    {
        use tauri_winrt_notification::Toast;
        if let Err(e) = Toast::new("Gesso").title(title).text1(body).show() {
            println!("[notify] 系统 toast 发送失败（便携版未注册 AUMID 时预期内）：{e}");
        }
    }
}
