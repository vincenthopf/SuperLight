use crate::actions::Action;

pub const SOURCE_COUNT: usize = 12;
pub const INJECTION_MARKER: i64 = 0x4d4f5554;
pub const INVERT_MARKER: i64 = 0x4d4f5553;
pub const HOLD_WATCHDOG_MS: u64 = 20_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Down,
    Up,
    Tap,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dispatch {
    pub source: u8,
    pub action: Action,
    pub phase: Phase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Pass,
    Block,
    BlockAndReleaseAll,
}

#[derive(Clone, Copy)]
enum Capture {
    Active(Action),
    Cancelled,
}

#[derive(Default)]
pub struct Router {
    captured: [Option<Capture>; SOURCE_COUNT],
}

impl Router {
    pub fn route(
        &mut self,
        source: usize,
        down: bool,
        action: Action,
        mut emit: impl FnMut(Dispatch) -> bool,
    ) -> Decision {
        let Some(slot) = self.captured.get_mut(source) else {
            return Decision::Pass;
        };
        if down {
            if matches!(slot, Some(Capture::Active(_))) {
                return Decision::Block;
            }
            *slot = None;
            if action == Action::None {
                return Decision::Pass;
            }
            let phase = if matches!(action, Action::Mouse(_)) {
                Phase::Down
            } else {
                Phase::Tap
            };
            if !emit(Dispatch {
                source: source as u8,
                action,
                phase,
            }) {
                return Decision::Pass;
            }
            *slot = Some(Capture::Active(action));
            Decision::Block
        } else if let Some(captured) = slot.take() {
            let Capture::Active(captured) = captured else {
                return Decision::Block;
            };
            if matches!(captured, Action::Mouse(_))
                && !emit(Dispatch {
                    source: source as u8,
                    action: captured,
                    phase: Phase::Up,
                })
            {
                Decision::BlockAndReleaseAll
            } else {
                Decision::Block
            }
        } else {
            Decision::Pass
        }
    }

    pub fn cancel(&mut self) {
        for slot in &mut self.captured {
            if slot.is_some() {
                *slot = Some(Capture::Cancelled);
            }
        }
    }

    pub fn captured_mouse(&self, source: usize) -> Option<u8> {
        match self.captured.get(source).copied().flatten() {
            Some(Capture::Active(Action::Mouse(button))) => Some(button),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct HeldButtons {
    sources: [Option<(u8, u64)>; SOURCE_COUNT],
    counts: [u8; 5],
}

impl HeldButtons {
    pub fn press(&mut self, source: usize, button: u8, now_ms: u64) -> bool {
        if source >= SOURCE_COUNT
            || usize::from(button) >= self.counts.len()
            || self.sources[source].is_some()
        {
            return false;
        }
        self.sources[source] = Some((button, now_ms));
        let count = &mut self.counts[usize::from(button)];
        *count += 1;
        *count == 1
    }

    pub fn release(&mut self, source: usize) -> Option<u8> {
        let (button, _) = self.sources.get_mut(source)?.take()?;
        let count = &mut self.counts[usize::from(button)];
        *count = count.saturating_sub(1);
        (*count == 0).then_some(button)
    }

    pub fn release_all(&mut self) -> [bool; 5] {
        let released = self.counts.map(|count| count != 0);
        self.sources.fill(None);
        self.counts.fill(0);
        released
    }

    pub fn expire(&mut self, now_ms: u64) -> [bool; 5] {
        let mut released = [false; 5];
        for source in 0..SOURCE_COUNT {
            if self.sources[source]
                .is_some_and(|(_, since)| now_ms.saturating_sub(since) >= HOLD_WATCHDOG_MS)
                && let Some(button) = self.release(source)
            {
                released[usize::from(button)] = true;
            }
        }
        released
    }

    pub fn any(&self) -> bool {
        self.counts.iter().any(|count| *count != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_capture_blocks_orphan_release_but_accepts_a_new_press() {
        let mut router = Router::default();
        router.route(0, true, Action::Mouse(0), |_| true);
        router.cancel();
        assert_eq!(router.captured_mouse(0), None);
        assert_eq!(
            router.route(0, false, Action::None, |_| panic!()),
            Decision::Block
        );
        router.route(0, true, Action::Mouse(0), |_| true);
        router.cancel();
        let mut events = Vec::new();
        assert_eq!(
            router.route(0, true, Action::Mouse(1), |event| {
                events.push(event);
                true
            }),
            Decision::Block
        );
        assert_eq!(events[0].action, Action::Mouse(1));
    }

    #[test]
    fn profile_change_cannot_release_a_different_mouse_button() {
        let mut router = Router::default();
        let mut events = Vec::new();
        assert_eq!(
            router.route(2, true, Action::Mouse(0), |event| {
                events.push(event);
                true
            }),
            Decision::Block
        );
        assert_eq!(
            router.route(2, false, Action::Mouse(1), |event| {
                events.push(event);
                true
            }),
            Decision::Block
        );
        assert_eq!(events[0].action, Action::Mouse(0));
        assert_eq!(events[1].action, Action::Mouse(0));
        assert_eq!(events[1].phase, Phase::Up);
    }

    #[test]
    fn failed_press_fails_open_and_does_not_swallow_release() {
        let mut router = Router::default();
        assert_eq!(
            router.route(2, true, Action::Mouse(0), |_| false),
            Decision::Pass
        );
        assert_eq!(
            router.route(2, false, Action::Mouse(0), |_| panic!()),
            Decision::Pass
        );
    }

    #[test]
    fn failed_release_requires_emergency_cleanup() {
        let mut router = Router::default();
        assert_eq!(
            router.route(0, true, Action::Mouse(0), |_| true),
            Decision::Block
        );
        assert_eq!(
            router.route(0, false, Action::None, |_| false),
            Decision::BlockAndReleaseAll
        );
    }

    #[test]
    fn pausing_preserves_the_pairing_of_captured_releases() {
        let mut router = Router::default();
        router.route(0, true, Action::Mouse(0), |_| true);
        assert_eq!(
            router.route(0, false, Action::None, |_| true),
            Decision::Block
        );
        assert_eq!(
            router.route(0, true, Action::None, |_| panic!()),
            Decision::Pass
        );
    }

    #[test]
    fn two_physical_buttons_share_one_held_output() {
        let mut held = HeldButtons::default();
        assert!(held.press(0, 0, 0));
        assert!(!held.press(2, 0, 1));
        assert_eq!(held.release(0), None);
        assert_eq!(held.release(2), Some(0));
        assert!(!held.any());
    }

    #[test]
    fn duplicate_presses_are_not_duplicate_injections() {
        let mut held = HeldButtons::default();
        assert!(held.press(0, 0, 0));
        assert!(!held.press(0, 0, 1));
        assert_eq!(held.release(0), Some(0));
        assert_eq!(held.release(0), None);
    }

    #[test]
    fn watchdog_and_disconnect_release_all_outputs() {
        let mut held = HeldButtons::default();
        held.press(0, 0, 0);
        held.press(2, 1, 1000);
        assert_eq!(held.expire(19_999), [false; 5]);
        assert_eq!(held.expire(20_000), [true, false, false, false, false]);
        assert_eq!(held.release_all(), [false, true, false, false, false]);
        assert!(!held.any());
    }
}
