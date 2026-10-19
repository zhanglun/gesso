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
        NSFloatingWindowLevel, NSMenu, NSPanel, NSTrackingArea, NSTrackingAreaOptions, NSView,
        NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_core_foundation::CGAffineTransform;
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
    use objc2_quartz_core::CALayer;

    use super::{MonitorInfo, PetSave, PetSize, PetSpecies};
    use crate::engine::{self, EngineAction};
    use gesso_core::{PetBehaviorEvent, PetBehaviorState, PetInteraction};

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
        hit_radius: Cell<f64>,
        scale: Cell<f64>,
        activity: Cell<gesso_core::PetActivity>,
        size: Cell<PetSize>,
    }

    impl Default for PetHitIvars {
        fn default() -> Self {
            Self {
                press: RefCell::new(None),
                pending_click: RefCell::new(None),
                hit_radius: Cell::new(0.0),
                scale: Cell::new(1.0),
                activity: Cell::new(gesso_core::PetActivity::Normal),
                size: Cell::new(PetSize::Medium),
            }
        }
    }

    const BODY_W: f64 = 92.0;
    const BODY_H: f64 = 100.0;
    // 给 Walking 的左右/上下平移预留透明空间，避免内容被 NSView 边界裁掉。
    const MOTION_PADDING: f64 = 24.0;

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
                let scale = self.ivars().scale.get();
                let body = NSRect::new(
                    NSPoint::new(MOTION_PADDING / 2.0, MOTION_PADDING / 2.0),
                    NSSize::new(BODY_W * scale, BODY_H * scale),
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
                scale: Cell::new(scale),
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
                apply_feedback(hit_view.layer().expect("宠物窗口 layer 缺失"), behavior);
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
            let (px, py) = window_origin((x, y, w, h), rx, ry, window_w, window_h);
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

    fn apply_feedback(root: Retained<CALayer>, behavior: gesso_core::PetBehavior) {
        let (scale, angle) = if reduced_motion() {
            (1.0, 0.0)
        } else {
            let progress = behavior.elapsed().as_secs_f64();
            match behavior.state() {
                PetBehaviorState::Poked => {
                    let phase = (progress / 0.45).min(1.0);
                    (1.0 + 0.08 * (std::f64::consts::PI * phase).sin(), 0.0)
                }
                PetBehaviorState::Touched => {
                    let phase = (progress / 0.65).min(1.0);
                    (
                        1.0 + 0.04 * (std::f64::consts::PI * phase).sin(),
                        0.06 * (std::f64::consts::TAU * phase).sin(),
                    )
                }
                PetBehaviorState::Dragging => (1.03, 0.0),
                PetBehaviorState::Sleeping => (0.84, 0.0),
                PetBehaviorState::Idle => {
                    (1.0 + 0.015 * (std::f64::consts::TAU * progress).sin(), 0.0)
                }
                PetBehaviorState::Watching => (1.02, 0.0),
                PetBehaviorState::Walking => (1.0, 0.0),
            }
        };
        let (sin, cos) = angle.sin_cos();
        let (tx, ty) = match behavior.state() {
            PetBehaviorState::Walking => {
                let phase = (behavior.elapsed().as_secs_f64() / 3.0) * std::f64::consts::TAU;
                (phase.sin() * 10.0, phase.cos().abs() * 3.0)
            }
            _ => (0.0, 0.0),
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
            BODY_H * scale + MOTION_PADDING,
        )
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

    fn add_layer(root: &CALayer, frame: NSRect, fill: &NSColor, radius: f64) {
        let layer = CALayer::layer();
        layer.setFrame(frame);
        let cg_color = fill.CGColor();
        layer.setBackgroundColor(Some(&cg_color));
        layer.setCornerRadius(radius);
        layer.setMasksToBounds(true);
        root.addSublayer(&layer);
    }

    fn create_window(
        species: PetSpecies,
        appearance: &gesso_core::AppearanceParams,
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

        let hit_radius = if species == PetSpecies::Cat {
            34.0
        } else {
            24.0
        };
        let hit_view = PetHitView::new(
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(window_w, window_h)),
            mtm,
            hit_radius,
            scale,
        );
        let ox = MOTION_PADDING / 2.0;
        let oy = MOTION_PADDING / 2.0;
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

        let coat = match species {
            PetSpecies::Cat => {
                let palettes = [(0.95, 0.67, 0.45), (0.72, 0.52, 0.38), (0.88, 0.79, 0.62)];
                palettes[usize::from(appearance.coat_color) % palettes.len()]
            }
            PetSpecies::Jelly => (0.42, 0.78, 0.95),
        };
        let body = color(coat.0, coat.1, coat.2);
        let face = color(0.12, 0.10, 0.14);
        let highlight = color(1.0, 1.0, 1.0);
        let accent = match species {
            PetSpecies::Cat => color(0.96, 0.44, 0.43),
            PetSpecies::Jelly => color(0.72, 0.44, 0.95),
        };

        add_layer(
            &root,
            NSRect::new(
                NSPoint::new(ox, oy),
                NSSize::new(BODY_W * scale, BODY_H * scale),
            ),
            &body,
            if species == PetSpecies::Cat {
                34.0
            } else {
                24.0
            },
        );
        add_layer(
            &root,
            NSRect::new(
                NSPoint::new(ox + 18.0 * scale, oy + 51.0 * scale),
                NSSize::new(12.0 * scale, 18.0 * scale),
            ),
            &face,
            6.0,
        );
        add_layer(
            &root,
            NSRect::new(
                NSPoint::new(ox + 62.0 * scale, oy + 51.0 * scale),
                NSSize::new(12.0 * scale, 18.0 * scale),
            ),
            &face,
            6.0,
        );
        add_layer(
            &root,
            NSRect::new(
                NSPoint::new(ox + 21.0 * scale, oy + 59.0 * scale),
                NSSize::new(4.0 * scale, 5.0 * scale),
            ),
            &highlight,
            2.0,
        );
        add_layer(
            &root,
            NSRect::new(
                NSPoint::new(ox + 65.0 * scale, oy + 59.0 * scale),
                NSSize::new(4.0 * scale, 5.0 * scale),
            ),
            &highlight,
            2.0,
        );
        add_layer(
            &root,
            NSRect::new(
                NSPoint::new(ox + 40.0 * scale, oy + 36.0 * scale),
                NSSize::new(12.0 * scale, 7.0 * scale),
            ),
            &accent,
            4.0,
        );

        (window, hit_view)
    }

    #[cfg(test)]
    mod tests {
        use super::{
            classify_click, size_scale, window_dimensions, window_origin, ClickOutcome,
            DOUBLE_CLICK_WINDOW, DRAG_THRESHOLD,
        };
        use crate::pin::MonitorInfo;
        use gesso_core::PetSize;
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
            assert_close(window_dimensions(PetSize::Small), (97.6, 104.0));
            assert_close(window_dimensions(PetSize::Medium), (116.0, 124.0));
            assert_close(window_dimensions(PetSize::Large), (139.0, 149.0));
        }

        #[test]
        fn pet_window_origin_stays_inside_monitor_when_normalized() {
            let (window_w, window_h) = window_dimensions(PetSize::Medium);
            assert_close(
                window_origin((0.0, 0.0, 1440.0, 900.0), 0.88, 0.08, window_w, window_h),
                (1165.12, 62.08),
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
