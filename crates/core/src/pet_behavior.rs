//! 桌面宠物的纯行为状态机。
//!
//! 状态机不依赖平台时钟或渲染器；app 层以单调递增的 `Duration` 驱动它，
//! 渲染层只需要消费当前状态即可。

use std::time::Duration;

use crate::pet_save::PetActivity;

const WATCHING_TIMEOUT: Duration = Duration::from_secs(2);
const POKED_DURATION: Duration = Duration::from_millis(450);
const TOUCHED_DURATION: Duration = Duration::from_millis(650);
const WALKING_DURATION: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PetBehaviorState {
    Idle,
    Watching,
    Walking,
    Sleeping,
    Poked,
    Touched,
    Dragging,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PetBehaviorEvent {
    MouseMoved,
    Poke,
    Touch,
    StartWalking,
    FinishWalking,
    DragStarted,
    DragEnded,
    Hidden,
    Shown,
    Notify(PetNotice),
}

/// 助手气泡事件；优先级 Warning > NeedsAttention > NewResult。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PetNotice {
    NewResult,
    NeedsAttention,
    Warning,
}

impl PetNotice {
    pub fn priority(self) -> u8 {
        match self {
            PetNotice::NewResult => 0,
            PetNotice::NeedsAttention => 1,
            PetNotice::Warning => 2,
        }
    }

    fn important(self) -> bool {
        self.priority() > 0
    }

    fn duration(self) -> Duration {
        match self {
            PetNotice::NewResult => Duration::from_secs(4),
            _ => Duration::from_secs(8),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PetBehavior {
    state: PetBehaviorState,
    activity: PetActivity,
    state_elapsed: Duration,
    idle_elapsed: Duration,
    notice: Option<(PetNotice, Duration)>,
}

impl PetBehavior {
    pub fn new(activity: PetActivity) -> Self {
        Self {
            state: PetBehaviorState::Idle,
            activity,
            state_elapsed: Duration::ZERO,
            idle_elapsed: Duration::ZERO,
            notice: None,
        }
    }

    pub fn state(self) -> PetBehaviorState {
        self.state
    }

    pub fn activity(self) -> PetActivity {
        self.activity
    }

    pub fn elapsed(self) -> Duration {
        self.state_elapsed
    }

    /// 当前显示中的助手气泡；由 tick 递减到期自动清除。
    pub fn notice(self) -> Option<PetNotice> {
        self.notice.map(|(kind, _)| kind)
    }

    pub fn set_activity(&mut self, activity: PetActivity) {
        self.activity = activity;
    }

    pub fn handle(&mut self, event: PetBehaviorEvent) {
        match event {
            PetBehaviorEvent::MouseMoved => {
                if self.state == PetBehaviorState::Sleeping {
                    self.enter(PetBehaviorState::Idle);
                } else if self.state != PetBehaviorState::Dragging
                    && !matches!(
                        self.state,
                        PetBehaviorState::Poked | PetBehaviorState::Touched
                    )
                {
                    self.enter(PetBehaviorState::Watching);
                }
                self.idle_elapsed = Duration::ZERO;
            }
            PetBehaviorEvent::Poke | PetBehaviorEvent::Touch => {
                self.notice = None; // 点击即确认，气泡消失
                self.enter(if matches!(event, PetBehaviorEvent::Poke) {
                    PetBehaviorState::Poked
                } else {
                    PetBehaviorState::Touched
                });
            }
            PetBehaviorEvent::Notify(kind) => {
                if self.state == PetBehaviorState::Dragging {
                    return; // 拖拽期间丢弃
                }
                if self.state == PetBehaviorState::Sleeping && !kind.important() {
                    return; // 睡眠中普通事件不打扰
                }
                let replace = self
                    .notice
                    .is_none_or(|(current, _)| kind.priority() > current.priority());
                if replace {
                    self.notice = Some((kind, kind.duration()));
                }
                if kind.important()
                    && matches!(
                        self.state,
                        PetBehaviorState::Sleeping | PetBehaviorState::Walking
                    )
                {
                    self.enter(PetBehaviorState::Idle);
                }
            }
            PetBehaviorEvent::StartWalking => {
                if !matches!(self.state, PetBehaviorState::Dragging) {
                    self.enter(PetBehaviorState::Walking);
                }
            }
            PetBehaviorEvent::FinishWalking => {
                if self.state == PetBehaviorState::Walking {
                    self.enter(PetBehaviorState::Idle);
                }
            }
            PetBehaviorEvent::DragStarted => {
                self.notice = None;
                self.enter(PetBehaviorState::Dragging);
            }
            PetBehaviorEvent::DragEnded => self.enter(PetBehaviorState::Idle),
            PetBehaviorEvent::Hidden => {
                self.notice = None;
                self.enter(PetBehaviorState::Sleeping);
            }
            PetBehaviorEvent::Shown => {
                if self.state == PetBehaviorState::Sleeping {
                    self.enter(PetBehaviorState::Idle);
                }
            }
        }
    }

    pub fn tick(&mut self, elapsed: Duration) {
        self.state_elapsed = self.state_elapsed.saturating_add(elapsed);
        if self.state != PetBehaviorState::Dragging {
            self.idle_elapsed = self.idle_elapsed.saturating_add(elapsed);
        }
        if let Some((kind, remaining)) = self.notice {
            let remaining = remaining.saturating_sub(elapsed);
            self.notice = if remaining.is_zero() {
                None
            } else {
                Some((kind, remaining))
            };
        }

        match self.state {
            PetBehaviorState::Watching if self.state_elapsed >= WATCHING_TIMEOUT => {
                self.enter(PetBehaviorState::Idle)
            }
            PetBehaviorState::Walking if self.state_elapsed >= WALKING_DURATION => {
                self.enter(PetBehaviorState::Idle)
            }
            PetBehaviorState::Poked if self.state_elapsed >= POKED_DURATION => {
                self.enter(PetBehaviorState::Idle)
            }
            PetBehaviorState::Touched if self.state_elapsed >= TOUCHED_DURATION => {
                self.enter(PetBehaviorState::Idle)
            }
            PetBehaviorState::Idle if self.idle_elapsed >= self.sleep_after() => {
                self.enter(PetBehaviorState::Sleeping)
            }
            PetBehaviorState::Idle if self.idle_elapsed >= self.walk_after() => {
                self.enter(PetBehaviorState::Walking)
            }
            _ => {}
        }
    }

    fn sleep_after(self) -> Duration {
        match self.activity {
            PetActivity::Quiet => Duration::from_secs(45),
            PetActivity::Normal => Duration::from_secs(90),
            PetActivity::Lively => Duration::from_secs(180),
        }
    }

    fn walk_after(self) -> Duration {
        match self.activity {
            PetActivity::Quiet => Duration::from_secs(30),
            PetActivity::Normal => Duration::from_secs(20),
            PetActivity::Lively => Duration::from_secs(10),
        }
    }

    fn enter(&mut self, state: PetBehaviorState) {
        self.state = state;
        self.state_elapsed = Duration::ZERO;
        if matches!(
            state,
            PetBehaviorState::Idle
                | PetBehaviorState::Poked
                | PetBehaviorState::Touched
                | PetBehaviorState::Walking
        ) {
            self.idle_elapsed = Duration::ZERO;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interaction_feedback_returns_to_idle() {
        let mut behavior = PetBehavior::new(PetActivity::Normal);
        behavior.handle(PetBehaviorEvent::Poke);
        assert_eq!(behavior.state(), PetBehaviorState::Poked);
        behavior.tick(POKED_DURATION);
        assert_eq!(behavior.state(), PetBehaviorState::Idle);

        behavior.handle(PetBehaviorEvent::Touch);
        behavior.tick(TOUCHED_DURATION);
        assert_eq!(behavior.state(), PetBehaviorState::Idle);
    }

    #[test]
    fn watching_and_walking_have_bounded_lifetimes() {
        let mut behavior = PetBehavior::new(PetActivity::Normal);
        behavior.handle(PetBehaviorEvent::MouseMoved);
        behavior.tick(WATCHING_TIMEOUT);
        assert_eq!(behavior.state(), PetBehaviorState::Idle);

        behavior.handle(PetBehaviorEvent::StartWalking);
        behavior.tick(WALKING_DURATION);
        assert_eq!(behavior.state(), PetBehaviorState::Idle);
    }

    #[test]
    fn activity_controls_sleep_timeout() {
        let mut quiet = PetBehavior::new(PetActivity::Quiet);
        quiet.tick(Duration::from_secs(30));
        assert_eq!(quiet.state(), PetBehaviorState::Walking);
        quiet.handle(PetBehaviorEvent::FinishWalking);
        quiet.tick(Duration::from_secs(45));
        assert_eq!(quiet.state(), PetBehaviorState::Sleeping);

        let mut lively = PetBehavior::new(PetActivity::Lively);
        lively.tick(Duration::from_secs(179));
        assert_eq!(lively.state(), PetBehaviorState::Walking);
        lively.handle(PetBehaviorEvent::FinishWalking);
        lively.tick(Duration::from_secs(180));
        assert_eq!(lively.state(), PetBehaviorState::Sleeping);
    }

    #[test]
    fn dragging_blocks_other_activity_until_released() {
        let mut behavior = PetBehavior::new(PetActivity::Normal);
        behavior.handle(PetBehaviorEvent::DragStarted);
        behavior.handle(PetBehaviorEvent::StartWalking);
        behavior.handle(PetBehaviorEvent::MouseMoved);
        assert_eq!(behavior.state(), PetBehaviorState::Dragging);
        behavior.handle(PetBehaviorEvent::DragEnded);
        assert_eq!(behavior.state(), PetBehaviorState::Idle);
    }

    #[test]
    fn sleeping_pet_wakes_from_mouse_or_shown() {
        let mut behavior = PetBehavior::new(PetActivity::Normal);
        behavior.tick(Duration::from_secs(90));
        assert_eq!(behavior.state(), PetBehaviorState::Sleeping);
        behavior.handle(PetBehaviorEvent::MouseMoved);
        assert_eq!(behavior.state(), PetBehaviorState::Idle);
        behavior.tick(Duration::from_secs(19));
        assert_eq!(behavior.state(), PetBehaviorState::Idle);

        behavior.handle(PetBehaviorEvent::Hidden);
        behavior.handle(PetBehaviorEvent::Shown);
        assert_eq!(behavior.state(), PetBehaviorState::Idle);
        behavior.tick(Duration::from_secs(19));
        assert_eq!(behavior.state(), PetBehaviorState::Idle);
    }

    #[test]
    fn notices_merge_by_priority_and_expire() {
        let mut behavior = PetBehavior::new(PetActivity::Normal);
        behavior.handle(PetBehaviorEvent::Notify(PetNotice::NewResult));
        assert_eq!(behavior.notice(), Some(PetNotice::NewResult));

        // 同级与低级不替换
        behavior.handle(PetBehaviorEvent::Notify(PetNotice::NeedsAttention));
        assert_eq!(behavior.notice(), Some(PetNotice::NeedsAttention));
        behavior.handle(PetBehaviorEvent::Notify(PetNotice::NewResult));
        assert_eq!(behavior.notice(), Some(PetNotice::NeedsAttention));

        // 更高级替换并重置时长
        behavior.handle(PetBehaviorEvent::Notify(PetNotice::Warning));
        behavior.tick(Duration::from_secs(4)); // 越过普通气泡时长
        assert_eq!(behavior.notice(), Some(PetNotice::Warning));
        behavior.tick(Duration::from_secs(4));
        assert_eq!(behavior.notice(), None);

        // 到期后普通气泡可再次显示并按 4 秒到期
        behavior.handle(PetBehaviorEvent::Notify(PetNotice::NewResult));
        behavior.tick(Duration::from_secs(4));
        assert_eq!(behavior.notice(), None);
    }

    #[test]
    fn important_notice_interrupts_sleep_and_walking() {
        let mut behavior = PetBehavior::new(PetActivity::Normal);
        behavior.tick(Duration::from_secs(90));
        assert_eq!(behavior.state(), PetBehaviorState::Sleeping);
        behavior.handle(PetBehaviorEvent::Notify(PetNotice::Warning));
        assert_eq!(behavior.state(), PetBehaviorState::Idle);
        assert_eq!(behavior.notice(), Some(PetNotice::Warning));

        behavior.handle(PetBehaviorEvent::StartWalking);
        assert_eq!(behavior.state(), PetBehaviorState::Walking);
        behavior.handle(PetBehaviorEvent::Notify(PetNotice::NeedsAttention));
        assert_eq!(behavior.state(), PetBehaviorState::Idle);
    }

    #[test]
    fn notice_boundaries_drop_or_clear() {
        let mut behavior = PetBehavior::new(PetActivity::Normal);
        behavior.handle(PetBehaviorEvent::Notify(PetNotice::NewResult));
        behavior.handle(PetBehaviorEvent::Poke); // 点击确认
        assert_eq!(behavior.notice(), None);

        behavior.handle(PetBehaviorEvent::DragStarted); // 拖拽清空且期间丢弃
        behavior.handle(PetBehaviorEvent::Notify(PetNotice::Warning));
        assert_eq!(behavior.notice(), None);

        let mut sleeping = PetBehavior::new(PetActivity::Normal);
        sleeping.tick(Duration::from_secs(90));
        sleeping.handle(PetBehaviorEvent::Notify(PetNotice::NewResult));
        assert_eq!(sleeping.state(), PetBehaviorState::Sleeping);
        assert_eq!(sleeping.notice(), None);

        let mut hidden = PetBehavior::new(PetActivity::Normal);
        hidden.handle(PetBehaviorEvent::Notify(PetNotice::Warning));
        hidden.handle(PetBehaviorEvent::Hidden);
        assert_eq!(hidden.notice(), None);
    }
}
