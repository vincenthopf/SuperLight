from pathlib import Path


def write(path, content):
    file = Path(path)
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(content)


write("crates/superlight-service/src/shared.rs", r'''use arc_swap::ArcSwap;
use crossbeam_channel::{Receiver, Sender, bounded};
use serde_json::Value;
use std::sync::{Arc, Condvar, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}};
use std::time::{Duration, Instant};
use superlight_core::{actions::{Action, Platform}, gesture::Source, input::Dispatch, policy::Policy};
use superlight_ipc::{DeviceStatus, Permissions, Request, Response, Snapshot};

pub const INPUT_CAPACITY: usize = 256;
pub const COMMAND_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug)]
pub enum Input {
    Dispatch(Dispatch),
    GesturePress { at: u64 },
    GestureRelease,
    GestureMove { x: f64, y: f64, source: Source, at: u64 },
    Wheel { source: u8, action: Action, delta: f64, threshold: f64, at: u64 },
    Scroll { horizontal: bool, delta: i32 },
    Wake,
}

#[derive(Clone, Copy, Debug)]
pub struct QueuedInput {
    pub epoch: u64,
    pub event: Input,
}

pub enum Command {
    Rpc { request: Request, reply: Sender<Response> },
    Request(Request),
    Device(Option<DeviceStatus>),
    HardwarePending(bool),
    Error(String),
    Action(Action),
    Native { ready: bool, permissions: Permissions },
    ForegroundChanged,
}

pub struct Shared {
    pub policy: ArcSwap<Policy>,
    pub config: ArcSwap<Value>,
    pub input: Sender<QueuedInput>,
    pub commands: Sender<Command>,
    pub snapshot: Mutex<Snapshot>,
    pub quit: AtomicBool,
    pub suspended: AtomicBool,
    pub native_ready: AtomicBool,
    pub device_connected: AtomicBool,
    pub restricted: AtomicBool,
    pub emergency: AtomicBool,
    pub reconnect: AtomicBool,
    pub refresh: AtomicBool,
    pub generation: AtomicU64,
    pub input_epoch: AtomicU64,
    pub dropped: AtomicU64,
    pub gesture_held: AtomicBool,
    pub gesture_motion: AtomicBool,
    pub gesture_hid: AtomicBool,
    pub headless: bool,
    started: Instant,
    sleep: (Mutex<()>, Condvar),
}

impl Shared {
    pub fn new(value: Value, instance: String, headless: bool) -> Result<(Arc<Self>, Receiver<QueuedInput>, Receiver<Command>), String> {
        let policy = Policy::compile(&value, "default", Platform::current(), false)?;
        let (input, input_receiver) = bounded(INPUT_CAPACITY);
        let (commands, command_receiver) = bounded(COMMAND_CAPACITY);
        let snapshot = Snapshot { instance, config: value.clone(), active_profile: "default".into(), ..Snapshot::default() };
        let shared = Arc::new(Self {
            policy: ArcSwap::from_pointee(policy), config: ArcSwap::from_pointee(value), input, commands,
            snapshot: Mutex::new(snapshot), quit: AtomicBool::new(false), suspended: AtomicBool::new(false),
            native_ready: AtomicBool::new(false), device_connected: AtomicBool::new(false), restricted: AtomicBool::new(false),
            emergency: AtomicBool::new(false), reconnect: AtomicBool::new(false), refresh: AtomicBool::new(false),
            generation: AtomicU64::new(1), input_epoch: AtomicU64::new(1), dropped: AtomicU64::new(0),
            gesture_held: AtomicBool::new(false), gesture_motion: AtomicBool::new(false), gesture_hid: AtomicBool::new(false),
            headless, started: Instant::now(), sleep: (Mutex::new(()), Condvar::new()),
        });
        Ok((shared, input_receiver, command_receiver))
    }

    pub fn now_ms(&self) -> u64 { self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64 }

    pub fn stopping(&self) -> bool { self.quit.load(Ordering::Acquire) }

    pub fn allowed(&self) -> bool {
        self.native_ready.load(Ordering::Acquire) && self.device_connected.load(Ordering::Acquire)
            && !self.suspended.load(Ordering::Acquire) && !self.restricted.load(Ordering::Acquire)
            && !self.emergency.load(Ordering::Acquire) && !self.stopping() && !self.policy.load().paused
    }

    pub fn emit(&self, event: Input) -> bool {
        let message = QueuedInput { epoch: self.input_epoch.load(Ordering::Acquire), event };
        if self.input.try_send(message).is_ok() { return true; }
        self.dropped.fetch_add(1, Ordering::Relaxed);
        false
    }

    pub fn command(&self, command: Command) -> bool {
        if self.commands.try_send(command).is_ok() { return true; }
        self.dropped.fetch_add(1, Ordering::Relaxed);
        false
    }

    pub fn report(&self, error: impl ToString) {
        self.command(Command::Error(error.to_string().chars().take(512).collect()));
    }

    pub fn release_all(&self) {
        self.input_epoch.fetch_add(1, Ordering::AcqRel);
        self.emergency.store(true, Ordering::Release);
        self.gesture_held.store(false, Ordering::Release);
        self.gesture_motion.store(false, Ordering::Release);
        self.gesture_hid.store(false, Ordering::Release);
        self.emit(Input::Wake);
    }

    pub fn wake_hid(&self) { self.sleep.1.notify_all(); }

    pub fn wait(&self, duration: Duration) {
        let guard = self.sleep.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !self.stopping() && !self.reconnect.load(Ordering::Acquire) {
            let _result = self.sleep.1.wait_timeout(guard, duration);
        }
    }

    pub fn request_reconnect(&self) {
        self.reconnect.store(true, Ordering::Release);
        self.release_all();
        self.wake_hid();
    }

    pub fn stop(&self) {
        self.quit.store(true, Ordering::Release);
        self.release_all();
        self.wake_hid();
        if !self.headless { crate::native::post(crate::native::UiEvent::Quit); }
    }

    pub fn status(&self) -> Snapshot {
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        snapshot.dropped_events = self.dropped.load(Ordering::Relaxed);
        snapshot.suspended = self.suspended.load(Ordering::Acquire);
        snapshot.native_ready = self.native_ready.load(Ordering::Acquire);
        if !self.device_connected.load(Ordering::Acquire) { snapshot.device = None; }
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use superlight_core::{config, input::{Dispatch, Phase}};

    #[test]
    fn input_queue_never_grows_and_cancellation_invalidates_old_events() {
        let (shared, input, _) = Shared::new(config::defaults(), "test".into(), true).unwrap();
        let event = Input::Dispatch(Dispatch { source: 0, action: Action::Mouse(0), phase: Phase::Down });
        for _ in 0..INPUT_CAPACITY { assert!(shared.emit(event)); }
        assert!(!shared.emit(event));
        shared.release_all();
        let epoch = shared.input_epoch.load(Ordering::Acquire);
        for queued in input.try_iter() { assert_ne!(queued.epoch, epoch); }
        assert!(shared.emergency.load(Ordering::Acquire));
        assert!(shared.emit(event));
        assert_eq!(input.recv().unwrap().epoch, epoch);
    }

    #[test]
    fn native_input_requires_connection_permissions_and_an_active_policy() {
        let (shared, _, _) = Shared::new(superlight_core::config::defaults(), "test".into(), true).unwrap();
        assert!(!shared.allowed());
        shared.native_ready.store(true, Ordering::Release);
        assert!(!shared.allowed());
        shared.device_connected.store(true, Ordering::Release);
        assert!(shared.allowed());
        shared.suspended.store(true, Ordering::Release);
        assert!(!shared.allowed());
        shared.suspended.store(false, Ordering::Release);
        shared.restricted.store(true, Ordering::Release);
        assert!(!shared.allowed());
        shared.restricted.store(false, Ordering::Release);
        shared.release_all();
        assert!(!shared.allowed());
    }
}
''')

write("crates/superlight-service/src/hook.rs", r'''use crate::shared::{Input, Shared};
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
''')

write("crates/superlight-service/src/output.rs", r'''use crate::{native, shared::{Command, Input, QueuedInput, Shared}};
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
    pending_releases: [bool; 5],
}

impl<O: Output> Dispatcher<O> {
    pub fn new(output: O) -> Self { Self { output, held: HeldButtons::default(), pending_releases: [false; 5] } }

    pub fn any_held(&self) -> bool { self.held.any() || self.pending_releases.iter().any(|pending| *pending) }

    fn mouse_up(&mut self, button: u8) -> io::Result<()> {
        let result = self.output.mouse(button, false);
        if let Some(pending) = self.pending_releases.get_mut(usize::from(button)) { *pending = result.is_err(); }
        result
    }

    pub fn dispatch(&mut self, event: Dispatch, now: u64) -> io::Result<Option<Action>> {
        match (event.phase, event.action) {
            (Phase::Up, _) => {
                if let Some(button) = self.held.release(usize::from(event.source)) { self.mouse_up(button)?; }
            }
            (phase, Action::Mouse(button)) => {
                if self.pending_releases.get(usize::from(button)).copied().unwrap_or(false) { self.mouse_up(button)?; }
                if self.held.press(usize::from(event.source), button, now)
                    && let Err(error) = self.output.mouse(button, true) {
                    if let Some(button) = self.held.release(usize::from(event.source)) { let _ = self.mouse_up(button); }
                    return Err(error);
                }
                if phase == Phase::Tap
                    && let Some(button) = self.held.release(usize::from(event.source)) { self.mouse_up(button)?; }
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
                && let Err(failure) = self.mouse_up(button as u8) { error = Some(failure); }
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
        Err(error) => { shared.report(format!("Input action failed: {error}")); shared.release_all(); }
        _ => {}
    }
}

fn begin_gesture(gesture: &mut Gesture, policy: Policy, at: u64) {
    gesture.options = policy.gestures;
    gesture.press(at);
}

pub fn run(shared: Arc<Shared>, receiver: Receiver<QueuedInput>) {
    let mut dispatcher = Dispatcher::new(NativeOutput);
    let mut gesture = Gesture::default();
    let mut gesture_policy = Policy::default();
    let mut wheels = [WheelAccumulator::default(); 2];
    loop {
        if shared.emergency.swap(false, Ordering::AcqRel) || shared.stopping() {
            if let Err(error) = dispatcher.release_all() { shared.report(error); }
            gesture.cancel();
            shared.gesture_held.store(false, Ordering::Release);
            shared.gesture_motion.store(false, Ordering::Release);
            shared.gesture_hid.store(false, Ordering::Release);
            wheels = [WheelAccumulator::default(); 2];
        }
        if shared.stopping() { break; }
        let queued = if dispatcher.any_held() {
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
        if shared.emergency.load(Ordering::Acquire) || queued.epoch != shared.input_epoch.load(Ordering::Acquire) { continue; }
        let event = queued.event;
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
                begin_gesture(&mut gesture, gesture_policy, at);
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
    struct Fake {
        events: Rc<RefCell<Vec<(u8, bool)>>>,
        failed_release: bool,
        fail_once: bool,
    }

    impl Output for Fake {
        fn mouse(&mut self, button: u8, down: bool) -> io::Result<()> {
            self.events.borrow_mut().push((button, down));
            if !down && self.fail_once && !self.failed_release {
                self.failed_release = true;
                return Err(io::Error::other("temporary output failure"));
            }
            Ok(())
        }
        fn chord(&mut self, _: Chord) -> io::Result<()> { Ok(()) }
        fn media(&mut self, _: u8) -> io::Result<()> { Ok(()) }
        fn system(&mut self, _: u8) -> io::Result<()> { Ok(()) }
        fn scroll(&mut self, _: bool, _: i32) -> io::Result<()> { Ok(()) }
    }

    #[test]
    fn held_mouse_remap_keeps_down_up_and_drop_cleanup() {
        let fake = Fake::default();
        let events = Rc::clone(&fake.events);
        let mut dispatcher = Dispatcher::new(fake);
        dispatcher.dispatch(Dispatch { source: 0, action: Action::Mouse(0), phase: Phase::Down }, 0).unwrap();
        assert_eq!(*events.borrow(), [(0, true)]);
        drop(dispatcher);
        assert_eq!(*events.borrow(), [(0, true), (0, false)]);
    }

    #[test]
    fn tap_on_an_already_held_output_does_not_break_dragging() {
        let fake = Fake::default();
        let events = Rc::clone(&fake.events);
        let mut dispatcher = Dispatcher::new(fake);
        dispatcher.dispatch(Dispatch { source: 0, action: Action::Mouse(0), phase: Phase::Down }, 0).unwrap();
        dispatcher.dispatch(Dispatch { source: 4, action: Action::Mouse(0), phase: Phase::Tap }, 1).unwrap();
        assert_eq!(*events.borrow(), [(0, true)]);
        dispatcher.dispatch(Dispatch { source: 0, action: Action::None, phase: Phase::Up }, 2).unwrap();
        assert_eq!(*events.borrow(), [(0, true), (0, false)]);
    }

    #[test]
    fn failed_releases_remain_pending_until_successful_cleanup() {
        let fake = Fake { fail_once: true, ..Fake::default() };
        let events = Rc::clone(&fake.events);
        let mut dispatcher = Dispatcher::new(fake);
        dispatcher.dispatch(Dispatch { source: 0, action: Action::Mouse(0), phase: Phase::Down }, 0).unwrap();
        assert!(dispatcher.dispatch(Dispatch { source: 0, action: Action::None, phase: Phase::Up }, 1).is_err());
        assert!(dispatcher.any_held());
        dispatcher.expire(2).unwrap();
        assert!(!dispatcher.any_held());
        assert_eq!(*events.borrow(), [(0, true), (0, false), (0, false)]);
    }

    #[test]
    fn device_actions_are_returned_without_running_hid_on_the_input_thread() {
        let mut dispatcher = Dispatcher::new(Fake::default());
        let action = Action::SwitchScrollMode;
        assert_eq!(dispatcher.dispatch(Dispatch { source: 6, action, phase: Phase::Tap }, 0).unwrap(), Some(action));
    }

    #[test]
    fn separate_gesture_presses_preserve_the_global_cooldown() {
        let mut gesture = Gesture::default();
        let mut policy = Policy::default();
        policy.gestures.enabled = true;
        policy.gestures.cooldown_ms = 500;
        begin_gesture(&mut gesture, policy, 0);
        assert!(gesture.movement(60.0, 0.0, Source::Hid, 1).is_some());
        assert!(!gesture.release());
        begin_gesture(&mut gesture, policy, 100);
        assert_eq!(gesture.movement(60.0, 0.0, Source::Hid, 101), None);
        assert!(gesture.movement(60.0, 0.0, Source::Hid, 501).is_some());
    }
}
''')
