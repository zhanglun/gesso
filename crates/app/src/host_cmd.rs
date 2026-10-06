//! 引擎 → 宿主页的命令契约（类型化）。
//!
//! 历史教训：Rust 侧曾在多处手拼 `__gesso&&__gesso.setFps(5)` 这类字符串，
//! 宿主端 `index.html` 各自定义，两侧靠人肉对齐；还有 `__gesso&&__gesso.`
//! 这种为防空指针的丑陋守卫。现在命令只在此枚举一次声明、序列化只发生在
//! `to_js` 一处；调用方持类型化值，改名/加命令时编译器兜住所有调用点。

/// 发往宿主页的一条命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCommand {
    /// 暂停（宿主页停 RAF / video.pause）。
    Pause,
    /// 恢复。
    Resume,
    /// 帧率上限实时变更。
    SetFps(u8),
    /// 挂钟时间脉冲（毫秒时间戳）。
    Tick,
    /// 光标 feed：present=false 表示离屏（x/y/buttons 恒为 0）。
    MouseLeave,
    /// 光标在屏：归一化量化坐标（x,y ∈ [0,10000]，原点左下）+ 按键位。
    Mouse { x: u16, y: u16, buttons: u8 },
}

impl HostCommand {
    /// 序列化为宿主页可执行的 JS。
    /// 空值守卫统一在此（页面脚本未就绪时静默丢弃，避免 ReferenceError）。
    pub fn to_js(self) -> String {
        match self {
            HostCommand::Pause => "__gesso&&__gesso.pause()".into(),
            HostCommand::Resume => "__gesso&&__gesso.resume()".into(),
            HostCommand::SetFps(n) => format!("__gesso&&__gesso.setFps({n})"),
            HostCommand::Tick => "__gesso&&__gesso.tick&&__gesso.tick(Date.now())".into(),
            HostCommand::MouseLeave => "__gesso&&__gesso.mouse&&__gesso.mouse(0,0,0,0)".into(),
            HostCommand::Mouse { x, y, buttons } => {
                format!("__gesso&&__gesso.mouse&&__gesso.mouse(1,{x},{y},{buttons})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_each_variant() {
        assert_eq!(HostCommand::Pause.to_js(), "__gesso&&__gesso.pause()");
        assert_eq!(HostCommand::SetFps(5).to_js(), "__gesso&&__gesso.setFps(5)");
        assert_eq!(
            HostCommand::SetFps(60).to_js(),
            "__gesso&&__gesso.setFps(60)"
        );
        assert_eq!(
            HostCommand::Mouse {
                x: 12,
                y: 3400,
                buttons: 1
            }
            .to_js(),
            "__gesso&&__gesso.mouse&&__gesso.mouse(1,12,3400,1)"
        );
        assert_eq!(
            HostCommand::MouseLeave.to_js(),
            "__gesso&&__gesso.mouse&&__gesso.mouse(0,0,0,0)"
        );
        assert!(HostCommand::Tick.to_js().contains("tick"));
    }
}
