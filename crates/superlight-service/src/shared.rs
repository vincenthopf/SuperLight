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

pub enum Command {
    Rpc { request: Request, reply: Sender<Response> },
    Request(Request),
    Device(Option<DeviceStatus>),
    HardwarePending(bool),
    Error(String),
    Action(Action),
    Native { ready: bool, permissions: Permissions },
}

pub struct Shared {
    pub policy: ArcSwap<Policy>,
    pub config: ArcSwap<Value>,
    pub input: Sender<Input>,
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
    pub dropped: AtomicU64,
    pub gesture_held: AtomicBool,
    pub gesture_motion: AtomicBool,
    pub gesture_hid: AtomicBool,
    started: Instant,
    sleep: (Mutex<()>, Condvar),
}

impl Shared {
    pub fn new(value: Value, instance: String) -> Result<(Arc<Self>, Receiver<Input>, Receiver<Command>), String> {
        let policy = Policy::compile(&value, "default", Platform::current(), false)?;
        let (input, input_receiver) = bounded(INPUT_CAPACITY);
        let (commands, command_receiver) = bounded(COMMAND_CAPACITY);
        let snapshot = Snapshot { instance, config: value.clone(), active_profile: "default".into(), ..Snapshot::default() };
        let shared = Arc::new(Self {
            policy: ArcSwap::from_pointee(policy), config: ArcSwap::from_pointee(value), input, commands,
            snapshot: Mutex::new(snapshot), quit: AtomicBool::new(false), suspended: AtomicBool::new(false),
            native_ready: AtomicBool::new(false), device_connected: AtomicBool::new(false), restricted: AtomicBool::new(false),
            emergency: AtomicBool::new(false), reconnect: AtomicBool::new(false), refresh: AtomicBool::new(false),
            generation: AtomicU64::new(1), dropped: AtomicU64::new(0), gesture_held: AtomicBool::new(false),
            gesture_motion: AtomicBool::new(false), gesture_hid: AtomicBool::new(false),
            started: Instant::now(), sleep: (Mutex::new(()), Condvar::new()),
        });
        Ok((shared, input_receiver, command_receiver))
    }

    pub fn now_ms(&self) -> u64 { self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64 }

    pub fn stopping(&self) -> bool { self.quit.load(Ordering::Acquire) }

    pub fn allowed(&self) -> bool {
        self.native_ready.load(Ordering::Acquire) && self.device_connected.load(Ordering::Acquire)
            && !self.suspended.load(Ordering::Acquire) && !self.restricted.load(Ordering::Acquire)
            && !self.stopping() && !self.policy.load().paused
    }

    pub fn emit(&self, event: Input) -> bool {
        if self.input.try_send(event).is_ok() { return true; }
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
        self.emergency.store(true, Ordering::Release);
        self.gesture_held.store(false, Ordering::Release);
        self.gesture_motion.store(false, Ordering::Release);
        self.gesture_hid.store(false, Ordering::Release);
        self.emit(Input::Wake);
    }

    pub fn wake_hid(&self) { self.sleep.1.notify_all(); }

    pub fn wait(&self, duration: Duration) {
        let guard = self.sleep.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = self.sleep.1.wait_timeout_while(guard, duration, |_| !self.stopping() && !self.reconnect.load(Ordering::Acquire));
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
        crate::native::post(crate::native::UiEvent::Quit);
    }

    pub fn status(&self) -> Snapshot {
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        snapshot.dropped_events = self.dropped.load(Ordering::Relaxed);
        snapshot.suspended = self.suspended.load(Ordering::Acquire);
        snapshot.native_ready = self.native_ready.load(Ordering::Acquire);
        snapshot
    }
}
