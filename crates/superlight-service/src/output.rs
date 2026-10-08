use crate::{
    native,
    shared::{Command, Input, QueuedInput, Shared},
};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use std::{
    io,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use superlight_core::{
    actions::{Action, Chord},
    gesture::{Gesture, Source, WheelAccumulator},
    input::{Dispatch, HeldButtons, Phase},
    policy::Policy,
};

pub trait Output {
    fn mouse(&mut self, button: u8, down: bool) -> io::Result<()>;
    fn chord(&mut self, chord: Chord) -> io::Result<()>;
    fn media(&mut self, key: u8) -> io::Result<()>;
    fn system(&mut self, action: u8) -> io::Result<()>;
    fn scroll(&mut self, horizontal: bool, delta: i32) -> io::Result<()>;
}

pub struct NativeOutput;

impl Output for NativeOutput {
    fn mouse(&mut self, button: u8, down: bool) -> io::Result<()> {
        native::mouse(button, down)
    }
    fn chord(&mut self, chord: Chord) -> io::Result<()> {
        native::chord(chord)
    }
    fn media(&mut self, key: u8) -> io::Result<()> {
        native::media(key)
    }
    fn system(&mut self, action: u8) -> io::Result<()> {
        native::system(action)
    }
    fn scroll(&mut self, horizontal: bool, delta: i32) -> io::Result<()> {
        native::scroll(horizontal, delta)
    }
}

pub struct Dispatcher<O: Output> {
    output: O,
    held: HeldButtons,
    pending_releases: [bool; 5],
}

impl<O: Output> Dispatcher<O> {
    pub fn new(output: O) -> Self {
        Self {
            output,
            held: HeldButtons::default(),
            pending_releases: [false; 5],
        }
    }

    pub fn any_held(&self) -> bool {
        self.held.any() || self.pending_releases.iter().any(|pending| *pending)
    }

    fn mouse_up(&mut self, button: u8) -> io::Result<()> {
        let result = self.output.mouse(button, false);
        if let Some(pending) = self.pending_releases.get_mut(usize::from(button)) {
            *pending = result.is_err();
        }
        result
    }

    pub fn dispatch(&mut self, event: Dispatch, now: u64) -> io::Result<Option<Action>> {
        match (event.phase, event.action) {
            (Phase::Up, _) => {
                if let Some(button) = self.held.release(usize::from(event.source)) {
                    self.mouse_up(button)?;
                }
            }
            (phase, Action::Mouse(button)) => {
                if self
                    .pending_releases
                    .get(usize::from(button))
                    .copied()
                    .unwrap_or(false)
                {
                    self.mouse_up(button)?;
                }
                if self.held.press(usize::from(event.source), button, now)
                    && let Err(error) = self.output.mouse(button, true)
                {
                    if let Some(button) = self.held.release(usize::from(event.source)) {
                        let _ = self.mouse_up(button);
                    }
                    return Err(error);
                }
                if phase == Phase::Tap
                    && let Some(button) = self.held.release(usize::from(event.source))
                {
                    self.mouse_up(button)?;
                }
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
            if (release || self.pending_releases[button])
                && let Err(failure) = self.mouse_up(button as u8)
            {
                error = Some(failure);
            }
        }
        error.map_or(Ok(()), Err)
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
    fn drop(&mut self) {
        let _ = self.release_all();
    }
}

fn execute(dispatcher: &mut Dispatcher<NativeOutput>, shared: &Shared, event: Dispatch) {
    match dispatcher.dispatch(event, shared.now_ms()) {
        Ok(Some(action)) => {
            shared.command(Command::Action(action));
        }
        Err(error) => {
            shared.report(format!("Input action failed: {error}"));
            shared.release_all();
        }
        _ => {}
    }
}

pub fn run(shared: Arc<Shared>, receiver: Receiver<QueuedInput>) {
    let mut dispatcher = Dispatcher::new(NativeOutput);
    let mut gesture = Gesture::default();
    let mut gesture_policy = Policy::default();
    let mut wheels = [WheelAccumulator::default(); 2];
    loop {
        if shared.emergency.swap(false, Ordering::AcqRel) || shared.stopping() {
            if let Err(error) = dispatcher.release_all() {
                shared.report(error);
            }
            gesture.cancel();
            shared.gesture_held.store(false, Ordering::Release);
            shared.gesture_motion.store(false, Ordering::Release);
            shared.gesture_hid.store(false, Ordering::Release);
            wheels = [WheelAccumulator::default(); 2];
        }
        if shared.stopping() {
            break;
        }
        let queued = if dispatcher.any_held() {
            match receiver.recv_timeout(Duration::from_millis(250)) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => {
                    if let Err(error) = dispatcher.expire(shared.now_ms()) {
                        shared.report(error);
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match receiver.recv() {
                Ok(event) => event,
                Err(_) => break,
            }
        };
        if shared.emergency.load(Ordering::Acquire)
            || queued.epoch != shared.input_epoch.load(Ordering::Acquire)
        {
            continue;
        }
        let event = queued.event;
        if !shared.allowed() {
            if let Input::Dispatch(event) = event
                && event.phase == Phase::Up
            {
                execute(&mut dispatcher, &shared, event);
            }
            gesture.cancel();
            continue;
        }
        match event {
            Input::Dispatch(event) => execute(&mut dispatcher, &shared, event),
            Input::GesturePress { at } => {
                gesture_policy = **shared.policy.load();
                gesture.options = gesture_policy.gestures;
                gesture.press(at);
                shared
                    .gesture_motion
                    .store(gesture_policy.gestures.enabled, Ordering::Release);
            }
            Input::GestureRelease => {
                if gesture.release() {
                    execute(
                        &mut dispatcher,
                        &shared,
                        Dispatch {
                            source: 1,
                            action: gesture_policy.action(1),
                            phase: Phase::Tap,
                        },
                    );
                }
                shared.gesture_hid.store(false, Ordering::Release);
            }
            Input::GestureMove { x, y, source, at } => {
                if let Some(direction) = gesture.movement(x, y, source, at) {
                    let index = direction.mapping_index();
                    execute(
                        &mut dispatcher,
                        &shared,
                        Dispatch {
                            source: index as u8,
                            action: gesture_policy.action(index),
                            phase: Phase::Tap,
                        },
                    );
                }
                if source == Source::Hid {
                    shared.gesture_hid.store(true, Ordering::Release);
                }
            }
            Input::Wheel {
                source,
                action,
                delta,
                threshold,
                at,
            } => {
                let index = usize::from(source.saturating_sub(4)).min(1);
                if wheels[index].step(delta, threshold, action.is_volume(), at) {
                    execute(
                        &mut dispatcher,
                        &shared,
                        Dispatch {
                            source,
                            action,
                            phase: Phase::Tap,
                        },
                    );
                }
            }
            Input::Scroll { horizontal, delta } => {
                if let Err(error) = dispatcher.output.scroll(horizontal, delta) {
                    shared.report(error);
                }
            }
            Input::Wake => {}
        }
        if let Err(error) = dispatcher.expire(shared.now_ms()) {
            shared.report(error);
        }
    }
}
