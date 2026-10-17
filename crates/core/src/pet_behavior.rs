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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PetBehavior {
    state: PetBehaviorState,
    activity: PetActivity,
    state_elapsed: Duration,
    idle_elapsed: Duration,
}

impl PetBehavior {
    pub fn new(activity: PetActivity) -> Self {
        Self {
            state: PetBehaviorState::Idle,
            activity,
            state_elapsed: Duration::ZERO,
            idle_elapsed: Duration::ZERO,
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

    pub fn set_activity(&mut self, activity: PetActivity) {
        self.activity = activity;
    }

    pub fn handle(&mut self, event: PetBehaviorEvent) {
        match event {
            PetBehaviorEvent::MouseMoved => {
                if self.state != PetBehaviorState::Dragging
                    && !matches!(
                        self.state,
                        PetBehaviorState::Poked | PetBehaviorState::Touched
                    )
                {
                    self.enter(PetBehaviorState::Watching);
                }
                self.idle_elapsed = Duration::ZERO;
            }
            PetBehaviorEvent::Poke => self.enter(PetBehaviorState::Poked),
            PetBehaviorEvent::Touch => self.enter(PetBehaviorState::Touched),
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
            PetBehaviorEvent::DragStarted => self.enter(PetBehaviorState::Dragging),
            PetBehaviorEvent::DragEnded => self.enter(PetBehaviorState::Idle),
            PetBehaviorEvent::Hidden => self.enter(PetBehaviorState::Sleeping),
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

    fn enter(&mut self, state: PetBehaviorState) {
        self.state = state;
        self.state_elapsed = Duration::ZERO;
        if matches!(state, PetBehaviorState::Poked | PetBehaviorState::Touched) {
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
        quiet.tick(Duration::from_secs(45));
        assert_eq!(quiet.state(), PetBehaviorState::Sleeping);

        let mut lively = PetBehavior::new(PetActivity::Lively);
        lively.tick(Duration::from_secs(179));
        assert_eq!(lively.state(), PetBehaviorState::Idle);
        lively.tick(Duration::from_secs(1));
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
    fn hidden_pet_wakes_when_shown() {
        let mut behavior = PetBehavior::new(PetActivity::Normal);
        behavior.handle(PetBehaviorEvent::Hidden);
        assert_eq!(behavior.state(), PetBehaviorState::Sleeping);
        behavior.handle(PetBehaviorEvent::Shown);
        assert_eq!(behavior.state(), PetBehaviorState::Idle);
    }
}
