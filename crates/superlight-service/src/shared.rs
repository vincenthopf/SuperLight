use arc_swap::ArcSwap;
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
