//! 会话状态机 —— 纯函数转移，与图《03 运行时状态机》逐一对应。
//!
//! 每显示器一个会话，状态独立。暂停语义分级见技术方案 §5.3/§9。

/// 会话状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// 未指派壁纸。
    Idle,
    /// 宿主页装载内容中。
    Loading,
    /// 唯一满负荷常驻态。
    Playing,
    /// 托盘手动暂停。
    PausedUser,
    /// 自动暂停（全屏应用 / 电池供电共用，事件驱动恢复）。
    Autopause,
    /// 素材缺失 / 解码失败，可恢复。
    Error,
    /// 终态：退出 / 卸载。
    Stopped,
}

/// 触发转移的事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    /// 指派壁纸（Idle → Loading）。
    Assign,
    /// 内容就绪（Loading → Playing）。
    Loaded,
    /// 素材缺失 / 解码失败（Loading → Error）。
    LoadFailed,
    /// 用户重选内容（Error → Idle）。
    Reselect,
    /// 托盘手动暂停。
    UserPause,
    /// 手动恢复。
    UserResume,
    /// 全屏应用出现 / 切到电池。
    AutoPauseTrigger,
    /// 全屏退出 / 接通电源。
    AutoPauseClear,
    /// 退出应用 / 卸载壁纸（任何非终态 → Stopped）。
    Quit,
}

/// 纯函数状态转移；无效组合返回原状态（不 panic，规格 §3.6 错误约定）。
pub fn transfer(state: SessionState, event: SessionEvent) -> SessionState {
    use SessionEvent as E;
    use SessionState as S;

    if event == E::Quit {
        return match state {
            S::Stopped => S::Stopped,
            _ => S::Stopped, // §9：任何态 ──退出──▶ Stopped
        };
    }

    match (state, event) {
        (S::Idle, E::Assign) => S::Loading,
        (S::Loading, E::Loaded) => S::Playing,
        (S::Loading, E::LoadFailed) => S::Error,
        (S::Error, E::Reselect) => S::Idle,

        (S::Playing, E::UserPause) => S::PausedUser,
        (S::PausedUser, E::UserResume) => S::Playing,

        (S::Playing, E::AutoPauseTrigger) => S::Autopause,
        (S::Autopause, E::AutoPauseClear) => S::Playing,
        // 暂停态下直接指派新壁纸 = 回到装载
        (S::PausedUser | S::Autopause | S::Error, E::Assign) => S::Loading,

        (s, _) => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(events: &[SessionEvent]) -> SessionState {
        let mut s = SessionState::Idle;
        for e in events {
            s = transfer(s, *e);
        }
        s
    }

    #[test]
    fn happy_path() {
        use SessionEvent as E;
        use SessionState as S;
        assert_eq!(
            walk(&[E::Assign, E::Loaded]),
            S::Playing,
            "指派→就绪→播放（主轨）"
        );
    }

    #[test]
    fn failure_loop_recovers() {
        use SessionEvent as E;
        use SessionState as S;
        assert_eq!(
            walk(&[E::Assign, E::LoadFailed, E::Reselect, E::Assign, E::Loaded]),
            S::Playing,
            "失败→重选→再指派→成功"
        );
    }

    #[test]
    fn user_pause_cycle() {
        use SessionEvent as E;
        use SessionState as S;
        assert_eq!(
            walk(&[E::Assign, E::Loaded, E::UserPause, E::UserResume]),
            S::Playing
        );
    }

    #[test]
    fn autopause_cycle() {
        use SessionEvent as E;
        use SessionState as S;
        assert_eq!(
            walk(&[E::Assign, E::Loaded, E::AutoPauseTrigger, E::AutoPauseClear]),
            S::Playing,
            "全屏/电池暂停由事件驱动恢复"
        );
    }

    #[test]
    fn reassign_while_paused() {
        use SessionEvent as E;
        use SessionState as S;
        assert_eq!(
            walk(&[E::Assign, E::Loaded, E::UserPause, E::Assign, E::Loaded]),
            S::Playing
        );
    }

    #[test]
    fn quit_from_any_state() {
        use SessionEvent as E;
        use SessionState as S;
        for s in [
            S::Idle,
            S::Loading,
            S::Playing,
            S::PausedUser,
            S::Autopause,
            S::Error,
        ] {
            assert_eq!(transfer(s, E::Quit), S::Stopped, "quit from {s:?}");
        }
    }

    #[test]
    fn invalid_transitions_are_noops() {
        use SessionEvent as E;
        use SessionState as S;
        // Idle 上直接 Loaded / Playing 上再 Assign 之外的无效应保持原状
        assert_eq!(transfer(S::Idle, E::Loaded), S::Idle);
        assert_eq!(transfer(S::Playing, E::Reselect), S::Playing);
        assert_eq!(transfer(S::PausedUser, E::AutoPauseClear), S::PausedUser);
        assert_eq!(transfer(S::Stopped, E::Assign), S::Stopped, "终态不可逆");
    }
}
