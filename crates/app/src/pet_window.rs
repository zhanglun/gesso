//! macOS 宠物窗口：独立 AppKit overlay，不经 GPUI/SessionManager。
//!
//! 当前使用程序化占位外观；窗口事件通过 EngineAction 进入引擎，生命周期和显示状态由主循环同步。

use crate::pin::MonitorInfo;
use gesso_core::{PetSave, PetSize, PetSpecies};

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::{Cell, RefCell};
    use std::time::{Duration, Instant};

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::sel;
    use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
    use objc2_app_kit::{
        NSBackingStoreType, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSEvent,
        NSFloatingWindowLevel, NSImage, NSMenu, NSPanel, NSTrackingArea, NSTrackingAreaOptions,
        NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_core_foundation::CGAffineTransform;
    use objc2_foundation::{MainThreadMarker, NSData, NSPoint, NSRect, NSSize, NSString};
    use objc2_quartz_core::{CALayer, CATextLayer, CATransaction};

    use super::{MonitorInfo, PetSave, PetSize, PetSpecies};
    use crate::engine::{self, EngineAction};
    use gesso_core::{PetBehaviorEvent, PetBehaviorState, PetInteraction, PetNotice};

    const DRAG_THRESHOLD: f64 = 6.0;
    const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(300);

    fn rounded_rect_contains(point: NSPoint, rect: NSRect, radius: f64) -> bool {
        if point.x < rect.origin.x
            || point.x > rect.origin.x + rect.size.width
            || point.y < rect.origin.y
            || point.y > rect.origin.y + rect.size.height
        {
            return false;
        }
        let radius = radius
            .min(rect.size.width / 2.0)
            .min(rect.size.height / 2.0);
        let left = rect.origin.x + radius;
        let right = rect.origin.x + rect.size.width - radius;
        let bottom = rect.origin.y + radius;
        let top = rect.origin.y + rect.size.height - radius;
        if (left..=right).contains(&point.x) || (bottom..=top).contains(&point.y) {
            return true;
        }
        let corner_x = if point.x < left { left } else { right };
        let corner_y = if point.y < bottom { bottom } else { top };
        let dx = point.x - corner_x;
        let dy = point.y - corner_y;
        dx * dx + dy * dy <= radius * radius
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ClickOutcome {
        Poke,
        Touch,
        Drag,
    }

    fn classify_click(distance: f64, elapsed_since_last_click: Option<Duration>) -> ClickOutcome {
        if distance > DRAG_THRESHOLD {
            ClickOutcome::Drag
        } else if elapsed_since_last_click.is_some_and(|elapsed| elapsed <= DOUBLE_CLICK_WINDOW) {
            ClickOutcome::Touch
        } else {
            ClickOutcome::Poke
        }
    }

    struct PressState {
        start: NSPoint,
        start_origin: NSPoint,
        dragged: bool,
    }

    struct PetHitIvars {
        press: RefCell<Option<PressState>>,
        pending_click: RefCell<Option<Instant>>,
        last_mouse_moved: RefCell<Option<Instant>>,
        bubble: RefCell<Option<BubbleLayers>>,
        sprite_layer: RefCell<Option<Retained<CALayer>>>,
        sprites: RefCell<[Option<Retained<objc2_core_graphics::CGImage>>; 3]>,
        sprite_frame: Cell<u8>,
        hit_radius: Cell<f64>,
        activity: Cell<gesso_core::PetActivity>,
        size: Cell<PetSize>,
    }

    /// 助手气泡的占位图层；shown 缓存当前显示的事件，变更时才重设文案与 frame。
    struct BubbleLayers {
        bg: Retained<CALayer>,
        text: Retained<CATextLayer>,
        shown: Option<PetNotice>,
    }

    impl Default for PetHitIvars {
        fn default() -> Self {
            Self {
                press: RefCell::new(None),
                pending_click: RefCell::new(None),
                last_mouse_moved: RefCell::new(None),
                bubble: RefCell::new(None),
                sprite_layer: RefCell::new(None),
                sprites: RefCell::new([None, None, None]),
                sprite_frame: Cell::new(0),
                hit_radius: Cell::new(0.0),
                activity: Cell::new(gesso_core::PetActivity::Normal),
                size: Cell::new(PetSize::Medium),
            }
        }
    }

    const BODY_W: f64 = 92.0;
    const BODY_H: f64 = 100.0;
    // 给 Walking 的左右/上下平移预留透明空间，避免内容被 NSView 边界裁掉。
    const MOTION_PADDING: f64 = 24.0;
    // 给头顶的助手气泡预留透明空间；命中检测只看身体矩形，气泡不可点击。
    const BUBBLE_HEADROOM: f64 = 40.0;
    const BUBBLE_HEIGHT: f64 = 24.0;
    // 首发角色「小蜡烛」基准稿位图（candle-4，见角色设计方向稿 §7）。
    // 三帧：清醒 / 火苗偏摆（两帧交替即摇曳）/ 睡眠（火苗缩小变暗 + 闭眼）。
    const CANDLE_IDLE: &[u8] = include_bytes!("../assets/pets/candle-idle-0.png");
    const CANDLE_FLICKER: &[u8] = include_bytes!("../assets/pets/candle-idle-1.png");
    const CANDLE_SLEEP: &[u8] = include_bytes!("../assets/pets/candle-sleep.png");
    // 蜡烛可见蜡身宽度（body 区域内居中）；命中检测按此宽度收窄。
    const CANDLE_BODY_W: f64 = 50.0;
    const HIT_RADIUS: f64 = 22.0;

    define_class!(
        #[unsafe(super(NSPanel))]
        #[thread_kind = MainThreadOnly]
        struct PetPanel;

        impl PetPanel {
            #[unsafe(method(canBecomeKeyWindow))]
            fn can_become_key_window(&self) -> bool {
                false
            }

            #[unsafe(method(canBecomeMainWindow))]
            fn can_become_main_window(&self) -> bool {
                false
            }
        }
    );

    define_class!(
        #[unsafe(super(NSView))]
        #[thread_kind = MainThreadOnly]
        #[ivars = PetHitIvars]
        struct PetHitView;

        impl PetHitView {
            #[unsafe(method(hitTest:))]
            fn hit_test(&self, point: NSPoint) -> *mut AnyObject {
                let size = self.ivars().size.get();
                let full = body_rect(size);
                let scale = size_scale(size);
                let inset = (BODY_W - CANDLE_BODY_W) * scale / 2.0;
                let body = NSRect::new(
                    NSPoint::new(full.origin.x + inset, full.origin.y),
                    NSSize::new(CANDLE_BODY_W * scale, full.size.height),
                );
                let inside = rounded_rect_contains(point, body, self.ivars().hit_radius.get());
                if inside {
                    self as *const Self as *mut AnyObject
                } else {
                    std::ptr::null_mut()
                }
            }

            #[unsafe(method(mouseMoved:))]
            fn mouse_moved(&self, _event: &NSEvent) {
                let now = Instant::now();
                let mut last = self.ivars().last_mouse_moved.borrow_mut();
                if last.is_some_and(|at| now.duration_since(at) < Duration::from_millis(200)) {
                    return;
                }
                *last = Some(now);
                engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::MouseMoved));
            }

            #[unsafe(method(rightMouseDown:))]
            fn right_mouse_down(&self, event: &NSEvent) {
                self.show_context_menu(event);
            }

            #[unsafe(method(petSettings:))]
            fn pet_settings(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::OpenPetSettings);
            }

            #[unsafe(method(togglePet:))]
            fn toggle_pet(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::TogglePetVisibility);
            }

            #[unsafe(method(petActivityQuiet:))]
            fn pet_activity_quiet(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::SetPetActivity(gesso_core::PetActivity::Quiet));
            }

            #[unsafe(method(petActivityNormal:))]
            fn pet_activity_normal(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::SetPetActivity(gesso_core::PetActivity::Normal));
            }

            #[unsafe(method(petActivityLively:))]
            fn pet_activity_lively(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::SetPetActivity(gesso_core::PetActivity::Lively));
            }

            #[unsafe(method(petSizeSmall:))]
            fn pet_size_small(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::SetPetSize(PetSize::Small));
            }

            #[unsafe(method(petSizeMedium:))]
            fn pet_size_medium(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::SetPetSize(PetSize::Medium));
            }

            #[unsafe(method(petSizeLarge:))]
            fn pet_size_large(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::SetPetSize(PetSize::Large));
            }

            #[unsafe(method(petNoticeNewResult:))]
            fn pet_notice_new_result(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::Notify(
                    PetNotice::NewResult,
                )));
            }

            #[unsafe(method(petNoticeAttention:))]
            fn pet_notice_attention(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::Notify(
                    PetNotice::NeedsAttention,
                )));
            }

            #[unsafe(method(petNoticeWarning:))]
            fn pet_notice_warning(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::Notify(
                    PetNotice::Warning,
                )));
            }

            #[unsafe(method(quitGesso:))]
            fn quit_gesso(&self, _sender: Option<&AnyObject>) {
                engine::enqueue(EngineAction::Quit);
            }

            #[unsafe(method(mouseDown:))]
            fn mouse_down(&self, _event: &NSEvent) {
                let window = self.window().expect("宠物命中视图必须属于窗口");
                let cursor = NSEvent::mouseLocation();
                *self.ivars().press.borrow_mut() = Some(PressState {
                    start: cursor,
                    start_origin: window.frame().origin,
                    dragged: false,
                });
            }

            #[unsafe(method(mouseDragged:))]
            fn mouse_dragged(&self, _event: &NSEvent) {
                let Some(mut press) = self.ivars().press.borrow_mut().take() else {
                    return;
                };
                let point = NSEvent::mouseLocation();
                let dx = point.x - press.start.x;
                let dy = point.y - press.start.y;
                if !press.dragged
                    && classify_click((dx * dx + dy * dy).sqrt(), None) == ClickOutcome::Drag
                {
                    press.dragged = true;
                    self.ivars().pending_click.replace(None);
                    engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::DragStarted));
                }
                if press.dragged {
                    let window = self.window().expect("宠物命中视图必须属于窗口");
                    // 仅更新原点，不触碰窗口尺寸或动画，避免拖动过程中出现裁剪。
                    window.setFrameOrigin(NSPoint::new(
                        press.start_origin.x + dx,
                        press.start_origin.y + dy,
                    ));
                }
                *self.ivars().press.borrow_mut() = Some(press);
            }

            #[unsafe(method(mouseUp:))]
            fn mouse_up(&self, _event: &NSEvent) {
                let Some(press) = self.ivars().press.borrow_mut().take() else {
                    return;
                };
                if press.dragged {
                    engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::DragEnded));
                    return;
                }
                let now = Instant::now();
                let mut pending = self.ivars().pending_click.borrow_mut();
                match classify_click(
                    0.0,
                    pending.map(|last| now.duration_since(last)),
                ) {
                    ClickOutcome::Touch => {
                        *pending = None;
                        engine::enqueue(EngineAction::PetInteraction(PetInteraction::Touch));
                        engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::Touch));
                    }
                    ClickOutcome::Poke => *pending = Some(now),
                    ClickOutcome::Drag => {}
                }
            }
        }
    );

    impl PetHitView {
        fn new(
            frame: NSRect,
            mtm: MainThreadMarker,
            hit_radius: f64,
            scale: f64,
        ) -> Retained<Self> {
            let allocated = Self::alloc(mtm).set_ivars(PetHitIvars {
                hit_radius: Cell::new(hit_radius * scale),
                activity: Cell::new(gesso_core::PetActivity::Normal),
                size: Cell::new(PetSize::Medium),
                ..PetHitIvars::default()
            });
            let view: Retained<Self> = unsafe { msg_send![super(allocated), initWithFrame: frame] };
            view.setWantsLayer(true);
            view
        }

        fn is_dragging(&self) -> bool {
            self.ivars().press.borrow().is_some()
        }

        fn show_context_menu(&self, event: &NSEvent) {
            let menu = NSMenu::new(MainThreadMarker::new().unwrap());
            let target = Some(self.as_ref() as &AnyObject);
            let key = NSString::from_str("");
            let title = |value: &str| NSString::from_str(value);

            let settings = unsafe {
                menu.addItemWithTitle_action_keyEquivalent(
                    &title(crate::ui::strings::TRAY_PET_SETTINGS()),
                    Some(sel!(petSettings:)),
                    &key,
                )
            };
            unsafe {
                settings.setTarget(target);
            }

            let activity = NSMenu::new(MainThreadMarker::new().unwrap());
            for (label, action, selected) in [
                (
                    crate::ui::strings::PET_ACTIVITY_QUIET(),
                    sel!(petActivityQuiet:),
                    self.ivars().activity.get() == gesso_core::PetActivity::Quiet,
                ),
                (
                    crate::ui::strings::PET_ACTIVITY_NORMAL(),
                    sel!(petActivityNormal:),
                    self.ivars().activity.get() == gesso_core::PetActivity::Normal,
                ),
                (
                    crate::ui::strings::PET_ACTIVITY_LIVELY(),
                    sel!(petActivityLively:),
                    self.ivars().activity.get() == gesso_core::PetActivity::Lively,
                ),
            ] {
                let item = unsafe {
                    activity.addItemWithTitle_action_keyEquivalent(
                        &title(label),
                        Some(action),
                        &key,
                    )
                };
                unsafe {
                    item.setTarget(target);
                    item.setState(if selected {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    });
                }
            }
            let activity_item = unsafe {
                menu.addItemWithTitle_action_keyEquivalent(
                    &title(crate::ui::strings::SET_PET_ACTIVITY()),
                    None,
                    &key,
                )
            };
            menu.setSubmenu_forItem(Some(&activity), &activity_item);

            let sizes = NSMenu::new(MainThreadMarker::new().unwrap());
            for (label, action, selected) in [
                (
                    crate::ui::strings::PET_SIZE_SMALL(),
                    sel!(petSizeSmall:),
                    self.ivars().size.get() == PetSize::Small,
                ),
                (
                    crate::ui::strings::PET_SIZE_MEDIUM(),
                    sel!(petSizeMedium:),
                    self.ivars().size.get() == PetSize::Medium,
                ),
                (
                    crate::ui::strings::PET_SIZE_LARGE(),
                    sel!(petSizeLarge:),
                    self.ivars().size.get() == PetSize::Large,
                ),
            ] {
                let item = unsafe {
                    sizes.addItemWithTitle_action_keyEquivalent(&title(label), Some(action), &key)
                };
                unsafe {
                    item.setTarget(target);
                    item.setState(if selected {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    });
                }
            }
            let size_item = unsafe {
                menu.addItemWithTitle_action_keyEquivalent(
                    &title(crate::ui::strings::SET_PET_SIZE()),
                    None,
                    &key,
                )
            };
            menu.setSubmenu_forItem(Some(&sizes), &size_item);

            let notices = NSMenu::new(MainThreadMarker::new().unwrap());
            for (label, action) in [
                (
                    crate::ui::strings::PET_NOTICE_NEW(),
                    sel!(petNoticeNewResult:),
                ),
                (
                    crate::ui::strings::PET_NOTICE_ATTENTION(),
                    sel!(petNoticeAttention:),
                ),
                (
                    crate::ui::strings::PET_NOTICE_WARNING(),
                    sel!(petNoticeWarning:),
                ),
            ] {
                let item = unsafe {
                    notices.addItemWithTitle_action_keyEquivalent(&title(label), Some(action), &key)
                };
                unsafe {
                    item.setTarget(target);
                }
            }
            let notice_item = unsafe {
                menu.addItemWithTitle_action_keyEquivalent(
                    &title(crate::ui::strings::PET_NOTICE_GROUP()),
                    None,
                    &key,
                )
            };
            menu.setSubmenu_forItem(Some(&notices), &notice_item);

            let stats = unsafe {
                menu.addItemWithTitle_action_keyEquivalent(
                    &title(crate::ui::strings::PET_STATS()),
                    Some(sel!(petSettings:)),
                    &key,
                )
            };
            let hide = unsafe {
                menu.addItemWithTitle_action_keyEquivalent(
                    &title(crate::ui::strings::TRAY_PET_HIDE()),
                    Some(sel!(togglePet:)),
                    &key,
                )
            };
            let quit = unsafe {
                menu.addItemWithTitle_action_keyEquivalent(
                    &title(crate::ui::strings::TRAY_QUIT()),
                    Some(sel!(quitGesso:)),
                    &key,
                )
            };
            for item in [&stats, &hide, &quit] {
                unsafe {
                    item.setTarget(target);
                }
            }
            NSMenu::popUpContextMenu_withEvent_forView(&menu, event, self);
        }

        fn update_menu_state(&self, activity: gesso_core::PetActivity, size: PetSize) {
            self.ivars().activity.set(activity);
            self.ivars().size.set(size);
        }

        fn flush_pending_click(&self) -> bool {
            let mut pending = self.ivars().pending_click.borrow_mut();
            if pending.is_some_and(|at| at.elapsed() > DOUBLE_CLICK_WINDOW) {
                *pending = None;
                if classify_click(0.0, None) == ClickOutcome::Poke {
                    engine::enqueue(EngineAction::PetInteraction(PetInteraction::Poke));
                    engine::enqueue(EngineAction::PetBehavior(PetBehaviorEvent::Poke));
                }
                true
            } else {
                false
            }
        }
    }

    #[derive(Default)]
    pub struct PetWindowManager {
        window: Option<Retained<PetPanel>>,
        hit_view: Option<Retained<PetHitView>>,
        pet_id: Option<String>,
        pet_size: Option<PetSize>,
        last_reported_position: Option<(String, f32, f32)>,
        last_applied_origin: Option<(f64, f64)>,
    }

    impl gpui_kit::Global for PetWindowManager {}

    impl PetWindowManager {
        pub fn sync(
            &mut self,
            save: &PetSave,
            monitors: &[MonitorInfo],
            behavior: gesso_core::PetBehavior,
        ) -> Option<gesso_core::PetPosition> {
            let Some(pet) = save.adoption.as_ref() else {
                self.hide();
                return None;
            };
            if !save.visible {
                self.hide();
                return None;
            }
            let Some(monitor) = choose_monitor(save, monitors) else {
                self.hide();
                return None;
            };

            if self.pet_id.as_deref() != Some(&pet.pet_id) || self.pet_size != Some(save.size) {
                self.hide();
                let (window, hit_view) = create_window(pet.species, &pet.appearance, save.size);
                self.window = Some(window);
                self.hit_view = Some(hit_view);
                self.pet_id = Some(pet.pet_id.clone());
                self.pet_size = Some(save.size);
                self.last_reported_position = None;
                self.last_applied_origin = None;
            }
            let window = self.window.as_ref()?;
            if let Some(hit_view) = &self.hit_view {
                apply_feedback(hit_view, behavior);
                hit_view.update_menu_state(save.activity, save.size);
                hit_view.flush_pending_click();
                // 鼠标事件和 150ms 主循环共享主线程；拖动期间禁止同步旧存档位置，
                // 否则一次拖动会在手指位置与旧目标之间来回抢写，产生抖动。
                if hit_view.is_dragging() {
                    return None;
                }
            }
            let (x, y, w, h) = monitor.frame;
            let (window_w, window_h) = window_dimensions(save.size);
            let (rx, ry) = save
                .position
                .as_ref()
                .filter(|position| position.monitor_id == monitor.id)
                .map(|position| {
                    (
                        position.x.clamp(0.0, 1.0) as f64,
                        position.y.clamp(0.0, 1.0) as f64,
                    )
                })
                .unwrap_or((0.88, 0.08));
            let base_origin = window_origin((x, y, w, h), rx, ry, window_w, window_h);
            let (px, py) = if reduced_motion() {
                base_origin
            } else {
                walking_origin(base_origin, monitor.frame, (window_w, window_h), behavior)
            };
            let actual = window.frame();
            let at_target =
                (actual.origin.x - px).abs() <= 2.0 && (actual.origin.y - py).abs() <= 2.0;
            if !at_target {
                let target_changed = self.last_applied_origin.is_some_and(|applied| {
                    (applied.0 - px).abs() > 2.0 || (applied.1 - py).abs() > 2.0
                });
                if !target_changed {
                    if let Some(applied) = self.last_applied_origin {
                        let moved = (actual.origin.x - applied.0).abs() > 2.0
                            || (actual.origin.y - applied.1).abs() > 2.0;
                        if moved {
                            if let Some(position) = position_for_frame(actual, monitors, save.size)
                            {
                                let key = (position.monitor_id.clone(), position.x, position.y);
                                if self.last_reported_position.as_ref() != Some(&key) {
                                    self.last_reported_position = Some(key);
                                    return Some(position);
                                }
                            }
                            // 等待引擎把新位置写回后再施加目标，避免拖动被旧存档抢回。
                            return None;
                        }
                    }
                }
                // 窗口尺寸已在创建时确定；同步只校正原点，不再触碰窗口 frame。
                window.setFrameOrigin(NSPoint::new(px, py));
            }
            self.last_reported_position = None;
            self.last_applied_origin = Some((px, py));
            window.setLevel(NSFloatingWindowLevel);
            window.orderFrontRegardless();
            None
        }

        fn hide(&self) {
            if let Some(window) = &self.window {
                window.orderOut(None);
            }
        }
    }

    fn apply_feedback(view: &PetHitView, behavior: gesso_core::PetBehavior) {
        let root = view.layer().expect("宠物窗口 layer 缺失");
        let reduced = reduced_motion();
        // reduced-motion 用静态姿态替代动画：脉冲改固定微放大，
        // 走动摇摆直接停用；蜡烛睡眠不压蜡身（火苗变暗即睡眠态）。
        let scale = match behavior.state() {
            PetBehaviorState::Poked => {
                if reduced {
                    1.06
                } else {
                    let phase = (behavior.elapsed().as_secs_f64() / 0.45).min(1.0);
                    1.0 + 0.08 * (std::f64::consts::PI * phase).sin()
                }
            }
            PetBehaviorState::Touched => {
                if reduced {
                    1.03
                } else {
                    let phase = (behavior.elapsed().as_secs_f64() / 0.65).min(1.0);
                    1.0 + 0.04 * (std::f64::consts::PI * phase).sin()
                }
            }
            PetBehaviorState::Dragging => 1.03,
            _ => 1.0,
        };
        let angle = if !reduced && behavior.state() == PetBehaviorState::Touched {
            let phase = (behavior.elapsed().as_secs_f64() / 0.65).min(1.0);
            0.06 * (std::f64::consts::TAU * phase).sin()
        } else {
            0.0
        };
        let (sin, cos) = angle.sin_cos();
        let (tx, ty) = if reduced {
            (0.0, 0.0)
        } else {
            match behavior.state() {
                PetBehaviorState::Walking => {
                    let phase = (behavior.elapsed().as_secs_f64() / 3.0) * std::f64::consts::TAU;
                    (phase.sin() * 10.0, phase.cos().abs() * 3.0)
                }
                _ => (0.0, 0.0),
            }
        };
        root.setOpacity(if behavior.state() == PetBehaviorState::Sleeping {
            0.72
        } else {
            1.0
        });
        root.setAffineTransform(CGAffineTransform {
            a: scale * cos,
            b: scale * sin,
            c: -scale * sin,
            d: scale * cos,
            tx,
            ty,
        });
        sync_bubble(view, behavior.notice());

        // 像素动画：清醒时火苗两帧交替摇曳；睡眠换闭眼小火苗帧；
        // reduced-motion 下停用摇曳只用清醒帧，状态切换仍即时（静态替代）。
        let frame_idx = match behavior.state() {
            PetBehaviorState::Sleeping => 2u8,
            _ if reduced => 0,
            _ => ((behavior.elapsed().as_millis() / 550) % 2) as u8,
        };
        if view.ivars().sprite_frame.get() != frame_idx {
            let layer_guard = view.ivars().sprite_layer.borrow();
            let sprites_guard = view.ivars().sprites.borrow();
            let layer = layer_guard.as_ref().map(|l| &**l as &CALayer);
            let image = sprites_guard[frame_idx as usize]
                .as_ref()
                .map(|img| &**img as &objc2_core_graphics::CGImage);
            if let (Some(layer), Some(image)) = (layer, image) {
                CATransaction::begin();
                CATransaction::setDisableActions(true);
                let _: () = unsafe { msg_send![layer, setContents: image] };
                CATransaction::commit();
            }
            view.ivars().sprite_frame.set(frame_idx);
        }
    }

    /// 按状态机当前气泡事件显示/隐藏占位气泡；变更时才重设文案与 frame。
    fn sync_bubble(view: &PetHitView, notice: Option<PetNotice>) {
        let mut bubble = view.ivars().bubble.borrow_mut();
        let Some(layers) = bubble.as_mut() else {
            return;
        };
        if layers.shown == notice {
            return;
        }
        layers.shown = notice;
        // setFrame/setString 默认带 0.25s 隐式动画，首次显示会从零尺寸
        // 漂移到目标位置；气泡按规格静态显示，禁用本次事务的隐式动画。
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let Some(kind) = notice else {
            layers.bg.setHidden(true);
            layers.text.setHidden(true);
            CATransaction::commit();
            return;
        };
        let text = match kind {
            PetNotice::NewResult => crate::ui::strings::PET_NOTICE_NEW_TEXT(),
            PetNotice::NeedsAttention => crate::ui::strings::PET_NOTICE_ATTENTION_TEXT(),
            PetNotice::Warning => crate::ui::strings::PET_NOTICE_WARNING_TEXT(),
        };
        let size = view.ivars().size.get();
        let (window_w, _) = window_dimensions(size);
        let body_top = MOTION_PADDING / 2.0 + BODY_H * size_scale(size);
        let bottom = body_top + 5.0;
        // shortcut: 气泡宽度按字符宽度估算，超宽时截断；接入正式美术时改用 NSAttributedString 度量
        let est: f64 = text
            .chars()
            .map(|c| if c.is_ascii() { 7.4 } else { 12.0 })
            .sum();
        let bubble_w = (est + 24.0).min(window_w - 8.0);
        let x = (window_w - bubble_w) / 2.0;
        layers.bg.setFrame(NSRect::new(
            NSPoint::new(x, bottom),
            NSSize::new(bubble_w, BUBBLE_HEIGHT),
        ));
        let value = NSString::from_str(text);
        let _: () = unsafe { msg_send![&layers.text, setString: &*value] };
        layers.text.setFrame(NSRect::new(
            NSPoint::new(x + 6.0, bottom + 5.0),
            NSSize::new(bubble_w - 12.0, BUBBLE_HEIGHT - 10.0),
        ));
        layers.bg.setHidden(false);
        layers.text.setHidden(false);
        CATransaction::commit();
    }

    fn reduced_motion() -> bool {
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }

    fn size_scale(size: PetSize) -> f64 {
        match size {
            PetSize::Small => 0.8,
            PetSize::Medium => 1.0,
            PetSize::Large => 1.25,
        }
    }

    fn window_dimensions(size: PetSize) -> (f64, f64) {
        let scale = size_scale(size);
        (
            BODY_W * scale + MOTION_PADDING,
            BODY_H * scale + MOTION_PADDING + BUBBLE_HEADROOM,
        )
    }

    fn body_rect(size: PetSize) -> NSRect {
        let scale = size_scale(size);
        NSRect::new(
            NSPoint::new(MOTION_PADDING / 2.0, MOTION_PADDING / 2.0),
            NSSize::new(BODY_W * scale, BODY_H * scale),
        )
    }

    fn walking_origin(
        base: (f64, f64),
        monitor: (f64, f64, f64, f64),
        window: (f64, f64),
        behavior: gesso_core::PetBehavior,
    ) -> (f64, f64) {
        if behavior.state() != PetBehaviorState::Walking {
            return base;
        }
        let progress = (behavior.elapsed().as_secs_f64() / 3.0).clamp(0.0, 1.0);
        let excursion = (std::f64::consts::PI * progress).sin();
        let left = (base.0 - monitor.0).max(0.0);
        let right = (monitor.0 + monitor.2 - base.0 - window.0).max(0.0);
        let direction = if right >= left { 1.0 } else { -1.0 };
        let room = if direction > 0.0 { right } else { left };
        let distance = room.min(56.0);
        (base.0 + direction * distance * excursion, base.1)
    }

    fn window_origin(
        (x, y, w, h): (f64, f64, f64, f64),
        rx: f64,
        ry: f64,
        window_w: f64,
        window_h: f64,
    ) -> (f64, f64) {
        (
            x + ((w - window_w).max(0.0) * rx),
            y + ((h - window_h).max(0.0) * ry),
        )
    }

    fn position_for_frame(
        frame: objc2_foundation::NSRect,
        monitors: &[MonitorInfo],
        size: PetSize,
    ) -> Option<gesso_core::PetPosition> {
        let center = (
            frame.origin.x + frame.size.width / 2.0,
            frame.origin.y + frame.size.height / 2.0,
        );
        let monitor = monitors
            .iter()
            .find(|m| {
                center.0 >= m.frame.0
                    && center.0 <= m.frame.0 + m.frame.2
                    && center.1 >= m.frame.1
                    && center.1 <= m.frame.1 + m.frame.3
            })
            .or_else(|| {
                monitors.iter().min_by(|a, b| {
                    distance_sq(center, a.frame).total_cmp(&distance_sq(center, b.frame))
                })
            })?;
        let (window_w, window_h) = window_dimensions(size);
        let x = ((frame.origin.x - monitor.frame.0) / (monitor.frame.2 - window_w).max(1.0))
            .clamp(0.0, 1.0);
        let y = ((frame.origin.y - monitor.frame.1) / (monitor.frame.3 - window_h).max(1.0))
            .clamp(0.0, 1.0);
        Some(gesso_core::PetPosition {
            monitor_id: monitor.id.clone(),
            x: x as f32,
            y: y as f32,
        })
    }

    fn distance_sq(point: (f64, f64), frame: (f64, f64, f64, f64)) -> f64 {
        let x = point.0.clamp(frame.0, frame.0 + frame.2);
        let y = point.1.clamp(frame.1, frame.1 + frame.3);
        (point.0 - x).powi(2) + (point.1 - y).powi(2)
    }

    fn choose_monitor<'a>(save: &PetSave, monitors: &'a [MonitorInfo]) -> Option<&'a MonitorInfo> {
        save.position
            .as_ref()
            .and_then(|position| monitors.iter().find(|m| m.id == position.monitor_id))
            .or_else(|| monitors.iter().find(|m| m.is_main))
            .or_else(|| monitors.first())
    }

    fn color(red: f64, green: f64, blue: f64) -> Retained<NSColor> {
        NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, 1.0)
    }

    fn create_window(
        _species: PetSpecies,
        _appearance: &gesso_core::AppearanceParams,
        size: PetSize,
    ) -> (Retained<PetPanel>, Retained<PetHitView>) {
        let mtm = MainThreadMarker::new().expect("宠物窗口必须在主线程创建");
        let (window_w, window_h) = window_dimensions(size);
        let scale = size_scale(size);
        let window: Retained<PetPanel> = unsafe {
            msg_send![
                super(PetPanel::alloc(mtm).set_ivars(())),
                initWithContentRect: NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(window_w, window_h),
                ),
                styleMask: NSWindowStyleMask::Borderless
                    | NSWindowStyleMask::NonactivatingPanel,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };
        window.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel);
        window.setFloatingPanel(true);
        window.setBecomesKeyOnlyIfNeeded(true);
        window.setHidesOnDeactivate(false);
        window.setOpaque(false);
        window.setHasShadow(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setIgnoresMouseEvents(false);
        window.setMovable(false);
        window.setMovableByWindowBackground(false);
        window.setLevel(NSFloatingWindowLevel);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );

        let hit_view = PetHitView::new(
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(window_w, window_h)),
            mtm,
            HIT_RADIUS,
            scale,
        );
        window.setContentView(Some(&hit_view));
        let root = hit_view.layer().expect("宠物窗口 layer 缺失");
        root.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(window_w, window_h),
        ));
        root.setMasksToBounds(false);
        // 以窗口中心为缩放锚点，避免 Walking/Idle 的变换把内容推到窗口外。
        root.setAnchorPoint(objc2_core_foundation::CGPoint::new(0.5, 0.5));
        root.setPosition(objc2_core_foundation::CGPoint::new(
            window_w / 2.0,
            window_h / 2.0,
        ));
        window.setAcceptsMouseMovedEvents(true);
        let tracking = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(window_w, window_h)),
                NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect,
                Some(&hit_view as &AnyObject),
                None,
            )
        };
        hit_view.addTrackingArea(&tracking);

        // 首发角色「小蜡烛」：直接使用角色基准稿位图（角色设计方向稿 §7），
        // 等比缩放放进 body 区域；描边、火苗和点眼都来自位图本身。
        // 三帧：清醒 / 火苗偏摆（两帧交替即摇曳）/ 睡眠（火苗缩小变暗 + 闭眼）。
        let sprite_layer = CALayer::layer();
        sprite_layer.setFrame(body_rect(size));
        sprite_layer.setContentsGravity(&NSString::from_str("resizeAspect"));
        sprite_layer.setContentsScale(window.backingScaleFactor());
        let mut sprites: [Option<Retained<objc2_core_graphics::CGImage>>; 3] = [None, None, None];
        for (slot, data) in sprites
            .iter_mut()
            .zip([CANDLE_IDLE, CANDLE_FLICKER, CANDLE_SLEEP])
        {
            let data = NSData::with_bytes(data);
            let image = NSImage::initWithData(NSImage::alloc(), &data).expect("蜡烛位图加载失败");
            let cg = unsafe {
                image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)
            }
            .expect("蜡烛位图转 CGImage 失败");
            *slot = Some(cg);
        }
        if let Some(first) = sprites[0].as_ref() {
            let _: () = unsafe { msg_send![&sprite_layer, setContents: &**first] };
        }
        root.addSublayer(&sprite_layer);
        *hit_view.ivars().sprite_layer.borrow_mut() = Some(sprite_layer);
        *hit_view.ivars().sprites.borrow_mut() = sprites;

        *hit_view.ivars().bubble.borrow_mut() =
            Some(create_bubble(&root, window.backingScaleFactor()));
        (window, hit_view)
    }

    fn create_bubble(root: &CALayer, contents_scale: f64) -> BubbleLayers {
        let bg = CALayer::layer();
        bg.setCornerRadius(8.0);
        let fill = color(0.98, 0.98, 0.99);
        let fill_cg = fill.CGColor();
        bg.setBackgroundColor(Some(&fill_cg));
        bg.setBorderWidth(1.0);
        let border = color(0.80, 0.81, 0.85);
        let border_cg = border.CGColor();
        bg.setBorderColor(Some(&border_cg));
        bg.setHidden(true);
        root.addSublayer(&bg);

        let text = CATextLayer::new();
        text.setFontSize(12.0);
        let ink = color(0.11, 0.11, 0.12);
        let ink_cg = ink.CGColor();
        text.setForegroundColor(Some(&ink_cg));
        text.setAlignmentMode(&NSString::from_str("center"));
        text.setContentsScale(contents_scale);
        text.setHidden(true);
        root.addSublayer(&text);
        BubbleLayers {
            bg,
            text,
            shown: None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            classify_click, size_scale, walking_origin, window_dimensions, window_origin,
            ClickOutcome, DOUBLE_CLICK_WINDOW, DRAG_THRESHOLD,
        };
        use crate::pin::MonitorInfo;
        use gesso_core::{PetBehaviorEvent, PetSize};
        use std::time::Duration;

        #[test]
        fn click_classifier_distinguishes_poke_touch_and_drag() {
            assert_eq!(classify_click(0.0, None), ClickOutcome::Poke);
            assert_eq!(
                classify_click(0.0, Some(DOUBLE_CLICK_WINDOW)),
                ClickOutcome::Touch
            );
            assert_eq!(
                classify_click(0.0, Some(DOUBLE_CLICK_WINDOW + Duration::from_millis(1))),
                ClickOutcome::Poke
            );
            assert_eq!(classify_click(DRAG_THRESHOLD, None), ClickOutcome::Poke);
            assert_eq!(
                classify_click(DRAG_THRESHOLD + 0.001, None),
                ClickOutcome::Drag
            );
        }

        #[test]
        fn pet_size_maps_to_window_dimensions() {
            assert_eq!(size_scale(PetSize::Small), 0.8);
            assert_close(window_dimensions(PetSize::Small), (97.6, 144.0));
            assert_close(window_dimensions(PetSize::Medium), (116.0, 164.0));
            assert_close(window_dimensions(PetSize::Large), (139.0, 189.0));
        }

        #[test]
        fn walking_origin_moves_without_leaving_monitor() {
            let monitor = (0.0, 0.0, 1440.0, 900.0);
            let window = (116.0, 164.0);
            let base = (1165.12, 58.88);
            let mut behavior = gesso_core::PetBehavior::new(gesso_core::PetActivity::Normal);
            behavior.handle(PetBehaviorEvent::StartWalking);
            behavior.tick(Duration::from_millis(1500));
            let middle = walking_origin(base, monitor, window, behavior);
            assert!(middle.0 < base.0);
            assert!(middle.0 >= monitor.0);
            assert!(middle.0 + window.0 <= monitor.0 + monitor.2);
            assert_eq!(
                walking_origin(
                    base,
                    monitor,
                    window,
                    gesso_core::PetBehavior::new(gesso_core::PetActivity::Normal),
                ),
                base
            );
        }

        #[test]
        fn pet_window_origin_stays_inside_monitor_when_normalized() {
            let (window_w, window_h) = window_dimensions(PetSize::Medium);
            assert_close(
                window_origin((0.0, 0.0, 1440.0, 900.0), 0.88, 0.08, window_w, window_h),
                (1165.12, 58.88),
            );
            assert_close(
                window_origin((10.0, -20.0, 100.0, 100.0), 1.0, 1.0, window_w, window_h),
                (10.0, -20.0),
            );
        }

        fn assert_close(actual: (f64, f64), expected: (f64, f64)) {
            assert!((actual.0 - expected.0).abs() < 1e-9);
            assert!((actual.1 - expected.1).abs() < 1e-9);
        }

        #[test]
        fn position_is_clamped_to_monitor_bounds() {
            let monitors = [MonitorInfo {
                id: "main".into(),
                name: "Main".into(),
                frame: (0.0, 0.0, 1440.0, 900.0),
                is_main: true,
            }];
            let position = super::position_for_frame(
                objc2_foundation::NSRect::new(
                    objc2_foundation::NSPoint::new(-100.0, 850.0),
                    objc2_foundation::NSSize::new(180.0, 180.0),
                ),
                &monitors,
                PetSize::Medium,
            )
            .unwrap();
            assert_eq!(position.monitor_id, "main");
            assert_eq!(position.x, 0.0);
            assert_eq!(position.y, 1.0);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod macos {
    use super::{MonitorInfo, PetSave};

    #[derive(Default)]
    pub struct PetWindowManager;

    impl gpui_kit::Global for PetWindowManager {}

    impl PetWindowManager {
        pub fn sync(
            &mut self,
            _save: &PetSave,
            _monitors: &[MonitorInfo],
            _behavior: gesso_core::PetBehavior,
        ) -> Option<gesso_core::PetPosition> {
            None
        }
    }
}

pub use macos::PetWindowManager;
