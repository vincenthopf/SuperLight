use crate::{hook::Hook, shared::Shared};
use evdev::{EventType, InputEvent, KeyCode, RelativeAxisCode};
use std::io;

pub const FRAME_CAPACITY: usize = 64;

pub struct Frame {
    events: [InputEvent; FRAME_CAPACITY],
    len: usize,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            events: std::array::from_fn(|_| InputEvent::new(0, 0, 0)),
            len: 0,
        }
    }
}

impl Frame {
    pub fn push(&mut self, event: InputEvent) -> io::Result<()> {
        if self.len == FRAME_CAPACITY {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "The mouse frame exceeds the bounded event capacity",
            ));
        }
        self.events[self.len] = event;
        self.len += 1;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn events(&self) -> &[InputEvent] {
        &self.events[..self.len]
    }
}

fn button_source(code: u16) -> Option<usize> {
    match KeyCode(code) {
        KeyCode::BTN_MIDDLE => Some(0),
        KeyCode::BTN_SIDE | KeyCode::BTN_BACK => Some(2),
        KeyCode::BTN_EXTRA | KeyCode::BTN_FORWARD => Some(3),
        _ => None,
    }
}

pub fn filter(
    frame: &Frame,
    hook: &mut Hook,
    shared: &Shared,
    output: &mut Frame,
) -> io::Result<()> {
    output.clear();
    let (mut x, mut y, mut horizontal, mut high_resolution) = (0i32, 0i32, 0i32, None::<i32>);
    for event in frame.events() {
        if event.event_type() != EventType::RELATIVE {
            continue;
        }
        match RelativeAxisCode(event.code()) {
            RelativeAxisCode::REL_X => x = x.saturating_add(event.value()),
            RelativeAxisCode::REL_Y => y = y.saturating_add(event.value()),
            RelativeAxisCode::REL_HWHEEL => horizontal = horizontal.saturating_add(event.value()),
            RelativeAxisCode::REL_HWHEEL_HI_RES => {
                high_resolution = Some(high_resolution.unwrap_or(0).saturating_add(event.value()))
            }
            _ => {}
        }
    }
    let motion_captured = (x != 0 || y != 0) && hook.movement(f64::from(x), f64::from(y));
    let wheel = high_resolution.map_or(f64::from(horizontal), |value| f64::from(value) / 120.0);
    let wheel_captured = wheel != 0.0 && hook.wheel(if wheel > 0.0 { 5 } else { 4 }, wheel);
    let policy = shared.policy.load();
    let allowed = shared.allowed();
    for event in frame.events() {
        match event.event_type() {
            EventType::SYNCHRONIZATION => {}
            EventType::KEY => {
                if let Some(source) = button_source(event.code())
                    && (event.value() == 2 || hook.button(source, event.value() != 0))
                {
                    continue;
                }
                output.push(*event)?;
            }
            EventType::RELATIVE => {
                let axis = RelativeAxisCode(event.code());
                if motion_captured
                    && matches!(axis, RelativeAxisCode::REL_X | RelativeAxisCode::REL_Y)
                {
                    continue;
                }
                let is_horizontal = matches!(
                    axis,
                    RelativeAxisCode::REL_HWHEEL | RelativeAxisCode::REL_HWHEEL_HI_RES
                );
                let is_vertical = matches!(
                    axis,
                    RelativeAxisCode::REL_WHEEL | RelativeAxisCode::REL_WHEEL_HI_RES
                );
                if wheel_captured && is_horizontal {
                    continue;
                }
                let invert = allowed
                    && ((is_horizontal && policy.invert_horizontal)
                        || (is_vertical && policy.invert_vertical));
                output.push(if invert {
                    InputEvent::new(
                        EventType::RELATIVE.0,
                        event.code(),
                        event.value().saturating_neg(),
                    )
                } else {
                    *event
                })?;
            }
            EventType::MISC => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Unsupported event type from the selected mouse",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::Input;
    use std::sync::{Arc, atomic::Ordering};
    use superlight_core::{
        actions::{Action, Platform},
        config,
        policy::Policy,
    };

    fn setup() -> (
        Arc<Shared>,
        crossbeam_channel::Receiver<crate::shared::QueuedInput>,
        Hook,
    ) {
        let (shared, receiver, _) = Shared::new(config::defaults(), "test".into(), true).unwrap();
        shared.native_ready.store(true, Ordering::Release);
        shared.device_connected.store(true, Ordering::Release);
        let hook = Hook::new(Arc::clone(&shared));
        (shared, receiver, hook)
    }

    fn frame(events: &[(u16, u16, i32)]) -> Frame {
        let mut frame = Frame::default();
        for &(kind, code, value) in events {
            frame.push(InputEvent::new(kind, code, value)).unwrap();
        }
        frame
    }

    fn values(frame: &Frame) -> Vec<(u16, u16, i32)> {
        frame
            .events()
            .iter()
            .map(|event| (event.event_type().0, event.code(), event.value()))
            .collect()
    }

    #[test]
    fn high_resolution_and_legacy_wheel_events_produce_one_mapping_event() {
        let (shared, receiver, mut hook) = setup();
        let input = frame(&[(2, 6, 1), (2, 12, 120)]);
        let mut output = Frame::default();
        filter(&input, &mut hook, &shared, &mut output).unwrap();
        assert!(output.events().is_empty());
        let events: Vec<_> = receiver.try_iter().collect();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0].event,
            Input::Wheel {
                source: 5,
                delta: 1.0,
                ..
            }
        ));
    }

    #[test]
    fn unmapped_frames_preserve_motion_buttons_and_both_scroll_resolutions() {
        let (shared, _, mut hook) = setup();
        shared.policy.store(Arc::new(Policy {
            paused: false,
            ..Policy::default()
        }));
        let input = frame(&[
            (2, 0, 7),
            (2, 1, -9),
            (1, 272, 1),
            (2, 6, 1),
            (2, 12, 120),
            (2, 8, -1),
            (2, 11, -120),
        ]);
        let mut output = Frame::default();
        filter(&input, &mut hook, &shared, &mut output).unwrap();
        assert_eq!(values(&input), values(&output));
    }

    #[test]
    fn scroll_inversion_changes_each_reported_resolution_exactly_once() {
        let (shared, _, mut hook) = setup();
        shared.policy.store(Arc::new(Policy {
            paused: false,
            invert_vertical: true,
            invert_horizontal: true,
            ..Policy::default()
        }));
        let input = frame(&[(2, 6, 1), (2, 12, 120), (2, 8, -1), (2, 11, -120)]);
        let mut output = Frame::default();
        filter(&input, &mut hook, &shared, &mut output).unwrap();
        assert_eq!(
            values(&output),
            [(2, 6, -1), (2, 12, -120), (2, 8, 1), (2, 11, 120)]
        );
    }

    #[test]
    fn unavailable_native_input_is_pass_through() {
        let (shared, receiver, mut hook) = setup();
        shared.native_ready.store(false, Ordering::Release);
        let input = frame(&[(1, 274, 1), (1, 274, 0), (2, 6, 1)]);
        let mut output = Frame::default();
        filter(&input, &mut hook, &shared, &mut output).unwrap();
        assert_eq!(values(&input), values(&output));
        assert!(receiver.is_empty());
    }

    #[test]
    fn configured_middle_button_is_not_forwarded_as_an_extra_physical_click() {
        let (shared, receiver, mut hook) = setup();
        let mut policy =
            Policy::compile(&config::defaults(), "default", Platform::Linux, false).unwrap();
        policy.mappings[0] = Action::Mouse(0);
        shared.policy.store(Arc::new(policy));
        let input = frame(&[(1, 274, 1), (1, 274, 0)]);
        let mut output = Frame::default();
        filter(&input, &mut hook, &shared, &mut output).unwrap();
        assert!(output.events().is_empty());
        assert_eq!(receiver.len(), 2);
    }

    #[test]
    fn frame_capacity_is_fixed_and_overflow_is_explicit() {
        let mut frame = Frame::default();
        for _ in 0..FRAME_CAPACITY {
            frame.push(InputEvent::new(2, 0, 1)).unwrap();
        }
        assert!(frame.push(InputEvent::new(2, 0, 1)).is_err());
        assert_eq!(frame.events().len(), FRAME_CAPACITY);
        frame.clear();
        assert!(frame.events().is_empty());
    }
}
