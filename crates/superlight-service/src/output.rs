use crate::{native, shared::{Command, Input, Shared}};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use std::{io, sync::{Arc, atomic::Ordering}, time::Duration};
use superlight_core::{actions::{Action, Chord}, gesture::{Gesture, Source, WheelAccumulator}, input::{Dispatch, HeldButtons, Phase}, policy::Policy};

pub trait Output {
    fn mouse(&mut self, button: u8, down: bool) -> io::Result<()>;
    fn chord(&mut self, chord: Chord) -> io::Result<()>;
    fn media(&mut self, key: u8) -> io::Result<()>;
    fn system(&mut self, action: u8) -> io::Result<()>;
    fn scroll(&mut self, horizontal: bool, delta: i32) -> io::Result<()>;
}

pub struct NativeOutput;

impl Output for NativeOutput {
    fn mouse(&mut self, button: u8, down: bool) -> io::Result<()> { native::mouse(button, down) }
    fn chord(&mut self, chord: Chord) -> io::Result<()> { native::chord(chord) }
    fn media(&mut self, key: u8) -> io::Result<()> { native::media(key) }
    fn system(&mut self, action: u8) -> io::Result<()> { native::system(action) }
    fn scroll(&mut self, horizontal: bool, delta: i32) -> io::Result<()> { native::scroll(horizontal, delta) }
}

pub struct Dispatcher<O: Output> {
    output: O,
    held: HeldButtons,
}

impl<O: Output> Dispatcher<O> {
    pub fn new(output: O) -> Self { Self { output, held: HeldButtons::default() } }

    pub fn any_held(&self) -> bool { self.held.any() }

    pub fn dispatch(&mut self, event: Dispatch, now: u64) -> io::Result<Option<Action>> {
        match (event.phase, event.action) {
            (Phase::Up, _) => {
                if let Some(button) = self.held.release(usize::from(event.source)) { self.output.mouse(button, false)?; }
            }
            (phase, Action::Mouse(button)) => {
                if self.held.press(usize::from(event.source), button, now)
                    && let Err(error) = self.output.mouse(button, true) {
                    self.held.release(usize::from(event.source));
                    return Err(error);
                }
                if phase == Phase::Tap
                    && let Some(button) = self.held.release(usize::from(event.source)) { self.output.mouse(button, false)?; }
            }
            (_, Action::Chord(chord)) => self.output.chord(chord)?,
            (_, Action::Media(key)) => self.output.media(key)?,
            (_, Action::System(action)) => self.output.system(action)?,
            (_, action) if action.is_device() => return Ok(Some(action)),
            _ => {}
        }
        Ok(None)
    }

    fn release_set(&mut self, buttons: [bool; 5]) -> io::Result<()> {
        let mut error = None;
        for (button, release) in buttons.into_iter().enumerate() {
            if release && let Err(failure) = self.output.mouse(button as u8, false) { error = Some(failure); }
        }
        if let Some(error) = error { Err(error) } else { Ok(()) }
    }

    pub fn release_all(&mut self) -> io::Result<()> {
        let buttons = self.held.release_all();
        self.release_set(buttons)
    }

    pub fn expire(&mut self, now: u64) -> io::Result<()> {
        let buttons = self.held.expire(now);
        self.release_set(buttons)
    }
}

impl<O: Output> Drop for Dispatcher<O> {
    fn drop(&mut self) { let _ = self.release_all(); }
}

fn execute(dispatcher: &mut Dispatcher<NativeOutput>, shared: &Shared, event: Dispatch) {
    match dispatcher.dispatch(event, shared.now_ms()) {
        Ok(Some(action)) => { shared.command(Command::Action(action)); }
        Err(error) => shared.report(format!("Input action failed: {error}")),
        _ => {}
    }
}

pub fn run(shared: Arc<Shared>, receiver: Receiver<Input>) {
    let mut dispatcher = Dispatcher::new(NativeOutput);
    let mut gesture = Gesture::default();
    let mut gesture_policy = Policy::default();
    let mut wheels = [WheelAccumulator::default(); 2];
    loop {
        if shared.emergency.swap(false, Ordering::AcqRel) || shared.stopping() {
            if let Err(error) = dispatcher.release_all() { shared.report(error); }
            gesture.cancel();
            shared.gesture_held.store(false, Ordering::Release);
            shared.gesture_hid.store(false, Ordering::Release);
        }
        if shared.stopping() { break; }
        let event = if dispatcher.any_held() {
            match receiver.recv_timeout(Duration::from_millis(250)) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => {
                    if let Err(error) = dispatcher.expire(shared.now_ms()) { shared.report(error); }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match receiver.recv() { Ok(event) => event, Err(_) => break }
        };
        if shared.emergency.load(Ordering::Acquire) { continue; }
        if !shared.allowed() {
            if let Input::Dispatch(event) = event
                && event.phase == Phase::Up { execute(&mut dispatcher, &shared, event); }
            gesture.cancel();
            continue;
        }
        match event {
            Input::Dispatch(event) => execute(&mut dispatcher, &shared, event),
            Input::GesturePress { at } => {
                gesture_policy = **shared.policy.load();
                gesture = Gesture::new(gesture_policy.gestures);
                gesture.press(at);
                shared.gesture_motion.store(gesture_policy.gestures.enabled, Ordering::Release);
            }
            Input::GestureRelease => {
                if gesture.release() {
                    execute(&mut dispatcher, &shared, Dispatch { source: 1, action: gesture_policy.action(1), phase: Phase::Tap });
                }
                shared.gesture_hid.store(false, Ordering::Release);
            }
            Input::GestureMove { x, y, source, at } => {
                if let Some(direction) = gesture.movement(x, y, source, at) {
                    let index = direction.mapping_index();
                    execute(&mut dispatcher, &shared, Dispatch { source: index as u8, action: gesture_policy.action(index), phase: Phase::Tap });
                }
                if source == Source::Hid { shared.gesture_hid.store(true, Ordering::Release); }
            }
            Input::Wheel { source, action, delta, threshold, at } => {
                let index = usize::from(source.saturating_sub(4)).min(1);
                if wheels[index].step(delta, threshold, action.is_volume(), at) {
                    execute(&mut dispatcher, &shared, Dispatch { source, action, phase: Phase::Tap });
                }
            }
            Input::Scroll { horizontal, delta } => {
                if let Err(error) = dispatcher.output.scroll(horizontal, delta) { shared.report(error); }
            }
            Input::Wake => {}
        }
        if let Err(error) = dispatcher.expire(shared.now_ms()) { shared.report(error); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Default)]
    struct Fake(Rc<RefCell<Vec<(u8, bool)>>>);
    impl Output for Fake {
        fn mouse(&mut self, button: u8, down: bool) -> io::Result<()> { self.0.borrow_mut().push((button, down)); Ok(()) }
        fn chord(&mut self, _: Chord) -> io::Result<()> { Ok(()) }
        fn media(&mut self, _: u8) -> io::Result<()> { Ok(()) }
        fn system(&mut self, _: u8) -> io::Result<()> { Ok(()) }
        fn scroll(&mut self, _: bool, _: i32) -> io::Result<()> { Ok(()) }
    }

    #[test]
    fn held_mouse_remap_keeps_down_up_and_drop_cleanup() {
        let fake = Fake::default();
        let events = Rc::clone(&fake.0);
        let mut dispatcher = Dispatcher::new(fake);
        dispatcher.dispatch(Dispatch { source: 0, action: Action::Mouse(0), phase: Phase::Down }, 0).unwrap();
        assert_eq!(*events.borrow(), [(0, true)]);
        drop(dispatcher);
        assert_eq!(*events.borrow(), [(0, true), (0, false)]);
    }

    #[test]
    fn tap_on_an_already_held_output_does_not_break_dragging() {
        let fake = Fake::default();
        let events = Rc::clone(&fake.0);
        let mut dispatcher = Dispatcher::new(fake);
        dispatcher.dispatch(Dispatch { source: 0, action: Action::Mouse(0), phase: Phase::Down }, 0).unwrap();
        dispatcher.dispatch(Dispatch { source: 4, action: Action::Mouse(0), phase: Phase::Tap }, 1).unwrap();
        assert_eq!(*events.borrow(), [(0, true)]);
        dispatcher.dispatch(Dispatch { source: 0, action: Action::None, phase: Phase::Up }, 2).unwrap();
        assert_eq!(*events.borrow(), [(0, true), (0, false)]);
    }

    #[test]
    fn device_actions_are_returned_without_running_hid_on_the_input_thread() {
        let mut dispatcher = Dispatcher::new(Fake::default());
        let action = Action::SwitchScrollMode;
        assert_eq!(dispatcher.dispatch(Dispatch { source: 6, action, phase: Phase::Tap }, 0).unwrap(), Some(action));
    }
}
