use crate::shared::{Input, Shared};
use std::sync::{Arc, atomic::Ordering};
use superlight_core::{actions::Action, gesture::Source, input::{Decision, Router}};

pub struct Hook {
    pub shared: Arc<Shared>,
    router: Router,
}

impl Hook {
    pub fn new(shared: Arc<Shared>) -> Self { Self { shared, router: Router::default() } }

    pub fn button(&mut self, source: usize, down: bool) -> bool {
        let action = if self.shared.allowed() { self.shared.policy.load().action(source) } else { Action::None };
        match self.router.route(source, down, action, |event| self.shared.emit(Input::Dispatch(event))) {
            Decision::Pass => false,
            Decision::Block => true,
            Decision::BlockAndReleaseAll => { self.shared.release_all(); true }
        }
    }

    pub fn dragged_button(&self, source: usize) -> Option<u8> { self.router.captured_mouse(source) }

    pub fn wheel(&self, source: usize, delta: f64) -> bool {
        if !self.shared.allowed() || !delta.is_finite() || delta == 0.0 { return false; }
        let policy = self.shared.policy.load();
        let action = policy.action(source);
        if action == Action::None { return false; }
        self.shared.emit(Input::Wheel { source: source as u8, action, delta, threshold: policy.horizontal_threshold, at: self.shared.now_ms() })
    }

    pub fn movement(&self, x: f64, y: f64) -> bool {
        if !self.shared.allowed() || !self.shared.gesture_held.load(Ordering::Acquire)
            || !self.shared.gesture_motion.load(Ordering::Acquire) { return false; }
        if self.shared.gesture_hid.load(Ordering::Acquire) { return true; }
        self.shared.emit(Input::GestureMove { x, y, source: Source::Native, at: self.shared.now_ms() })
    }
}
