use crate::shared::{Input, Shared};
use std::sync::{Arc, atomic::Ordering};
use superlight_core::{actions::Action, gesture::Source, input::{Decision, Router}};

pub struct Hook {
    shared: Arc<Shared>,
    router: Router,
    epoch: u64,
}

impl Hook {
    pub fn new(shared: Arc<Shared>) -> Self {
        let epoch = shared.input_epoch.load(Ordering::Acquire);
        Self { shared, router: Router::default(), epoch }
    }

    fn synchronize(&mut self) {
        let epoch = self.shared.input_epoch.load(Ordering::Acquire);
        if epoch != self.epoch {
            self.router.cancel();
            self.epoch = epoch;
        }
    }

    pub fn button(&mut self, source: usize, down: bool) -> bool {
        self.synchronize();
        let shared = &self.shared;
        let action = if shared.allowed() { shared.policy.load().action(source) } else { Action::None };
        match self.router.route(source, down, action, |event| shared.emit(Input::Dispatch(event))) {
            Decision::Pass => false,
            Decision::Block => true,
            Decision::BlockAndReleaseAll => { shared.release_all(); true }
        }
    }

    pub fn dragged_button(&mut self, source: usize) -> Option<u8> {
        self.synchronize();
        self.router.captured_mouse(source)
    }

    pub fn wheel(&mut self, source: u8, delta: f64) -> bool {
        self.synchronize();
        if !self.shared.allowed() || !delta.is_finite() || delta == 0.0 { return false; }
        let policy = self.shared.policy.load();
        let action = policy.action(usize::from(source));
        action != Action::None && self.shared.emit(Input::Wheel {
            source, action, delta, threshold: policy.horizontal_threshold, at: self.shared.now_ms(),
        })
    }

    pub fn movement(&mut self, x: f64, y: f64) -> bool {
        self.synchronize();
        if !self.shared.allowed() || !self.shared.gesture_held.load(Ordering::Acquire)
            || !self.shared.gesture_motion.load(Ordering::Acquire) { return false; }
        if self.shared.gesture_hid.load(Ordering::Acquire) { return true; }
        if self.shared.emit(Input::GestureMove { x, y, source: Source::Native, at: self.shared.now_ms() }) { return true; }
        self.shared.release_all();
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use superlight_core::{config, input::Phase, policy::Policy};

    fn setup() -> (Arc<Shared>, crossbeam_channel::Receiver<crate::shared::QueuedInput>, Hook) {
        let (shared, receiver, _) = Shared::new(config::defaults(), "test".into(), true).unwrap();
        shared.native_ready.store(true, Ordering::Release);
        shared.device_connected.store(true, Ordering::Release);
        let mut policy = Policy { paused: false, ..Policy::default() };
        policy.mappings[0] = Action::Mouse(0);
        shared.policy.store(Arc::new(policy));
        let hook = Hook::new(Arc::clone(&shared));
        (shared, receiver, hook)
    }

    #[test]
    fn a_profile_change_keeps_the_original_mouse_release() {
        let (shared, receiver, mut hook) = setup();
        assert!(hook.button(0, true));
        let mut policy = **shared.policy.load();
        policy.mappings[0] = Action::Mouse(1);
        shared.policy.store(Arc::new(policy));
        assert!(hook.button(0, false));
        let events: Vec<_> = receiver.try_iter().collect();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[1].event, Input::Dispatch(event) if event.action == Action::Mouse(0) && event.phase == Phase::Up));
    }

    #[test]
    fn cancellation_clears_drag_routing_without_replaying_a_stale_press() {
        let (shared, _, mut hook) = setup();
        assert!(hook.button(0, true));
        assert_eq!(hook.dragged_button(0), Some(0));
        shared.release_all();
        assert_eq!(hook.dragged_button(0), None);
        assert!(hook.button(0, false));
        shared.emergency.store(false, Ordering::Release);
        assert!(hook.button(0, true));
        assert_eq!(hook.dragged_button(0), Some(0));
    }

    #[test]
    fn input_overload_fails_open_for_new_presses() {
        let (shared, _, mut hook) = setup();
        for _ in 0..crate::shared::INPUT_CAPACITY { assert!(shared.emit(Input::Wake)); }
        assert!(!hook.button(0, true));
        assert!(!hook.button(0, false));
    }
}
