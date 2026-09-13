use crate::{
    shared::{Command, Input, Shared},
    transport::{self, Candidate, HidTransport},
};
use hidapi::HidApi;
use serde_json::Value;
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use superlight_core::{
    actions::Action,
    devices,
    gesture::Source,
    hidpp::{self, SmartShift},
    input::{Decision, Router},
    policy::{self, Policy},
    reports::{DeviceEvent, Notifications},
    session::{Error, Report, Session, Transport},
};
use superlight_ipc::DeviceStatus;

const READ_WAIT: Duration = Duration::from_millis(250);
const PROBE_WAIT: Duration = Duration::from_millis(350);
const RETRY_WAIT: Duration = Duration::from_secs(3);
const HEALTH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Desired {
    pub dpi: Option<u16>,
    pub smart_shift: Option<SmartShift>,
    pub diverts: [bool; 3],
}

impl Desired {
    pub fn from_config(
        value: &Value,
        policy: &Policy,
        status: &DeviceStatus,
        native_ready: bool,
    ) -> Self {
        let enabled = native_ready && !policy.paused;
        Self {
            dpi: status.supports_dpi.then(|| {
                policy::number(value, "dpi", 1000.0)
                    .clamp(f64::from(status.dpi_min), f64::from(status.dpi_max))
                    as u16
            }),
            smart_shift: status
                .supports_smart_shift
                .then(|| policy::smart_shift(value)),
            diverts: [
                enabled
                    && status.supports_gesture
                    && (policy.action(1) != Action::None || policy.gestures.enabled),
                enabled && status.supports_mode_shift && policy.action(6) != Action::None,
                enabled && status.supports_dpi_switch && policy.action(7) != Action::None,
            ],
        }
    }
}

pub fn verified_mouse(known_model: bool, kind: Option<u8>) -> bool {
    match kind {
        Some(3 | 5) => true,
        Some(_) => false,
        None => known_model,
    }
}

pub struct Pump {
    shared: Arc<Shared>,
    notifications: Notifications,
    router: Router,
    epoch: u64,
    held: [bool; 3],
    accepted_gesture: bool,
}

impl Pump {
    pub fn new(shared: Arc<Shared>, device: u8, feature: u8) -> Self {
        let epoch = shared.input_epoch.load(Ordering::Acquire);
        Self {
            shared,
            notifications: Notifications::new(device, feature),
            router: Router::default(),
            epoch,
            held: [false; 3],
            accepted_gesture: false,
        }
    }

    fn synchronize(&mut self) {
        let epoch = self.shared.input_epoch.load(Ordering::Acquire);
        if self.epoch != epoch {
            self.epoch = epoch;
            self.router.cancel();
            self.accepted_gesture = false;
        }
    }

    pub fn busy(&self) -> bool {
        self.held.iter().any(|held| *held)
    }

    pub fn configure(&mut self, diverts: [Option<superlight_core::session::Divert>; 3]) {
        self.notifications.configure(diverts);
    }

    pub fn process(&mut self, report: Report) {
        self.synchronize();
        let Self {
            shared,
            notifications,
            router,
            held,
            accepted_gesture,
            ..
        } = self;
        notifications.process(report, |event| match event {
            DeviceEvent::Button { source: 1, down } => {
                held[0] = down;
                if down {
                    if !shared.allowed() {
                        return;
                    }
                    let policy = **shared.policy.load();
                    shared.gesture_held.store(true, Ordering::Release);
                    shared
                        .gesture_motion
                        .store(policy.gestures.enabled, Ordering::Release);
                    shared.gesture_hid.store(false, Ordering::Release);
                    *accepted_gesture = shared.emit(Input::GesturePress {
                        at: shared.now_ms(),
                    });
                    if !*accepted_gesture {
                        shared.release_all();
                    }
                } else {
                    shared.gesture_held.store(false, Ordering::Release);
                    shared.gesture_motion.store(false, Ordering::Release);
                    if *accepted_gesture && shared.allowed() && !shared.emit(Input::GestureRelease)
                    {
                        shared.release_all();
                    }
                    *accepted_gesture = false;
                }
            }
            DeviceEvent::Button { source, down } => {
                if source == 6 {
                    held[1] = down;
                }
                if source == 7 {
                    held[2] = down;
                }
                let action = if shared.allowed() {
                    shared.policy.load().action(usize::from(source))
                } else {
                    Action::None
                };
                if router.route(usize::from(source), down, action, |event| {
                    shared.emit(Input::Dispatch(event))
                }) == Decision::BlockAndReleaseAll
                {
                    shared.release_all();
                }
            }
            DeviceEvent::Motion { x, y } => {
                if *accepted_gesture
                    && shared.allowed()
                    && shared.gesture_motion.load(Ordering::Acquire)
                {
                    if shared.emit(Input::GestureMove {
                        x: f64::from(x),
                        y: f64::from(y),
                        source: Source::Hid,
                        at: shared.now_ms(),
                    }) {
                        shared.gesture_hid.store(true, Ordering::Release);
                    } else {
                        shared.release_all();
                    }
                }
            }
        });
    }
}

fn probe<T: Transport>(
    session: &mut Session<T>,
    candidate: &Candidate,
) -> Result<(DeviceStatus, Vec<u16>), Error> {
    let mut ignore = |_| {};
    session.timeout = PROBE_WAIT;
    session.features.reprog = session.feature(hidpp::REPROG, &mut ignore)?;
    if session.features.reprog.is_none() {
        return Err(Error::Unsupported);
    }
    session.timeout = Duration::from_secs(2);
    session.features.name = session.feature(hidpp::DEVICE_NAME, &mut ignore)?;
    let name = session
        .read_name(&mut ignore)
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| candidate.name.clone());
    let spec = devices::resolve(candidate.product_id, &name);
    let kind = session
        .features
        .name
        .and_then(|feature| session.request(feature, 2, &[], &mut ignore).ok())
        .and_then(|report| report.params().first().copied());
    if !verified_mouse(spec.is_some(), kind) {
        return Err(Error::Unsupported);
    }
    let controls = session.discover_controls(&mut ignore)?;
    session.discover_optional_features(&mut ignore)?;
    let gestures = hidpp::gesture_candidates(
        &controls,
        spec.map_or(&hidpp::GESTURE_CIDS, |spec| spec.gesture_cids),
    );
    let supports_gesture = !gestures.is_empty();
    let supports_mode_shift = controls
        .iter()
        .any(|control| control.cid == hidpp::MODE_SHIFT_CID && control.flags & 0x0020 != 0);
    let supports_dpi_switch = controls
        .iter()
        .any(|control| control.cid == hidpp::DPI_SWITCH_CID && control.flags & 0x0020 != 0);
    let model_key = spec.map_or("generic", |spec| spec.key).to_owned();
    let status = DeviceStatus {
        name: if name.is_empty() {
            "Logitech HID++ mouse".into()
        } else {
            name
        },
        model_key: model_key.clone(),
        layout_key: model_key,
        product_id: candidate.product_id,
        receiver_slot: session.device_index,
        transport: if !candidate.bluetooth && session.device_index == 0xff {
            "USB"
        } else {
            hidpp::transport_label(session.device_index, candidate.product_id)
        }
        .into(),
        backend: if cfg!(target_os = "macos") {
            candidate.backend()
        } else {
            "Windows native HID"
        }
        .into(),
        dpi_min: spec.map_or(200, |spec| spec.dpi_min),
        dpi_max: spec.map_or(8000, |spec| spec.dpi_max),
        supports_dpi: session.features.dpi.is_some(),
        supports_smart_shift: session.features.smart_shift.is_some(),
        supports_gesture,
        supports_mode_shift,
        supports_dpi_switch,
        controls,
        ..DeviceStatus::default()
    };
    Ok((status, gestures))
}

struct Connected {
    session: Session<HidTransport>,
    pump: Pump,
    status: DeviceStatus,
    gestures: Vec<u16>,
}

fn connect(api: &mut HidApi, shared: &Arc<Shared>) -> Result<Option<Connected>, Error> {
    let candidates = transport::enumerate(api)?;
    let mut last_error = None;
    for candidate in &candidates {
        if shared.stopping() || shared.suspended.load(Ordering::Acquire) {
            break;
        }
        let (transport, access) = match HidTransport::open(api, candidate, shared) {
            Ok(value) => value,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        let mut session = Session::new(transport, 0xff);
        for &slot in candidate.indices() {
            if shared.stopping() || shared.suspended.load(Ordering::Acquire) {
                break;
            }
            session.retarget(slot)?;
            let (status, gestures) = match probe(&mut session, candidate) {
                Ok(value) => value,
                Err(Error::Transport(error)) => {
                    last_error = Some(Error::Transport(error));
                    break;
                }
                Err(_) => continue,
            };
            let mut allowed_controls = gestures.clone();
            if status.supports_mode_shift {
                allowed_controls.push(hidpp::MODE_SHIFT_CID);
            }
            if status.supports_dpi_switch {
                allowed_controls.push(hidpp::DPI_SWITCH_CID);
            }
            access
                .lock()
                .map_err(|_| Error::Transport("HID access policy is unavailable".into()))?
                .confirm_mouse(slot, &allowed_controls, status.dpi_min, status.dpi_max)
                .map_err(Error::Transport)?;
            let pump = Pump::new(
                Arc::clone(shared),
                slot,
                session.features.reprog.ok_or(Error::Unsupported)?,
            );
            return Ok(Some(Connected {
                session,
                pump,
                status,
                gestures,
            }));
        }
    }
    if let Some(error) = last_error {
        Err(error)
    } else {
        Ok(None)
    }
}

impl Connected {
    fn publish(&self, shared: &Shared) {
        shared.command(Command::Device(Some(self.status.clone())));
    }

    fn refresh(&mut self) -> Result<(), Error> {
        let mut notify = |report| self.pump.process(report);
        if self.status.supports_dpi {
            self.status.dpi = Some(self.session.read_dpi(&mut notify)?);
        }
        if self.status.supports_smart_shift {
            self.status.smart_shift = Some(self.session.read_smart_shift(&mut notify)?);
        }
        if self.session.features.battery.is_some() {
            if let Ok(battery) = self.session.read_battery(&mut notify) {
                self.status.battery = Some(battery);
            }
            if self.session.needs_reconnect() {
                return Err(Error::Timeout);
            }
        }
        Ok(())
    }

    fn reconcile(
        &mut self,
        desired: Desired,
        previous: Option<Desired>,
        shared: &Shared,
    ) -> Result<(), Error> {
        if let Some(dpi) = desired.dpi
            && previous.is_none_or(|old| old.dpi != desired.dpi)
        {
            self.session
                .set_dpi(dpi, &mut |report| self.pump.process(report))?;
            self.status.dpi = Some(dpi);
        }
        if let Some(state) = desired.smart_shift
            && previous.is_none_or(|old| old.smart_shift != desired.smart_shift)
        {
            self.session
                .set_smart_shift(state, &mut |report| self.pump.process(report))?;
            self.status.smart_shift = Some(state);
        }
        for slot in 0..3 {
            if !desired.diverts[slot] && self.session.diverts()[slot].is_some() {
                self.session
                    .restore_slot(slot, &mut |report| self.pump.process(report))?;
                self.pump.configure(*self.session.diverts());
                self.session
                    .read_notification(Duration::from_millis(50), &mut |report| {
                        self.pump.process(report)
                    })?;
            }
        }
        if desired.diverts[0] && self.session.diverts()[0].is_none() {
            let divert = self
                .session
                .divert_gesture(&self.gestures, &mut |report| self.pump.process(report))?;
            if divert.is_none() {
                self.status.supports_gesture = false;
                shared.report("The mouse rejected gesture diversion. Its gesture button remains under device control.");
            }
        }
        for (slot, cid) in [(1, hidpp::MODE_SHIFT_CID), (2, hidpp::DPI_SWITCH_CID)] {
            if desired.diverts[slot] && self.session.diverts()[slot].is_none() {
                match self
                    .session
                    .divert_extra(cid, &mut |report| self.pump.process(report))
                {
                    Ok(()) => {}
                    Err(Error::Protocol(hidpp::ProtocolError::Device(_))) => {
                        if slot == 1 {
                            self.status.supports_mode_shift = false;
                        } else {
                            self.status.supports_dpi_switch = false;
                        }
                        shared.report(format!("The mouse rejected diversion for control 0x{cid:04x}. Native control is unchanged."));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        self.status.raw_xy = self.session.diverts()[0].is_some_and(|divert| divert.raw_xy);
        self.pump.configure(*self.session.diverts());
        Ok(())
    }

    fn run(&mut self, shared: &Arc<Shared>) -> Result<(), Error> {
        let _ = self.refresh();
        self.publish(shared);
        let mut generation = 0;
        let mut desired = None;
        let mut applied = None;
        let mut last_health = Instant::now();
        let mut retry_at = Instant::now();
        let mut pending = false;
        while !shared.stopping() && !shared.suspended.load(Ordering::Acquire) {
            if shared.reconnect.swap(false, Ordering::AcqRel) {
                break;
            }
            let current_generation = shared.generation.load(Ordering::Acquire);
            if generation != current_generation {
                generation = current_generation;
                let value = shared.config.load();
                let policy = shared.policy.load();
                let ready = shared.native_ready.load(Ordering::Acquire)
                    && !shared.restricted.load(Ordering::Acquire);
                desired = Some(Desired::from_config(&value, &policy, &self.status, ready));
                self.status.layout_key = value["settings"]["device_layout_overrides"]
                    [&self.status.model_key]
                    .as_str()
                    .filter(|key| devices::DEVICES.iter().any(|spec| spec.key == *key))
                    .unwrap_or(&self.status.model_key)
                    .to_owned();
                retry_at = Instant::now();
            }
            if desired != applied && !self.pump.busy() && Instant::now() >= retry_at {
                if !pending {
                    shared.command(Command::HardwarePending(true));
                    pending = true;
                }
                let wanted = desired.ok_or(Error::Unsupported)?;
                match self.reconcile(wanted, applied, shared) {
                    Ok(()) => {
                        applied = Some(wanted);
                        pending = false;
                        shared.command(Command::HardwarePending(false));
                        self.publish(shared);
                    }
                    Err(Error::Transport(error)) => return Err(Error::Transport(error)),
                    Err(error) => {
                        shared.report(format!("Hardware settings are pending: {error}"));
                        if self.session.needs_reconnect() {
                            return Err(error);
                        }
                        retry_at = Instant::now() + RETRY_WAIT;
                    }
                }
            }
            if shared.refresh.swap(false, Ordering::AcqRel)
                || last_health.elapsed() >= HEALTH_INTERVAL
            {
                self.refresh()?;
                self.publish(shared);
                last_health = Instant::now();
            }
            self.session
                .read_notification(READ_WAIT, &mut |report| self.pump.process(report))?;
        }
        Ok(())
    }
}

pub fn run(shared: Arc<Shared>) {
    let mut api = None;
    while !shared.stopping() {
        shared.reconnect.store(false, Ordering::Release);
        if shared.suspended.load(Ordering::Acquire) {
            shared.wait(RETRY_WAIT);
            continue;
        }
        if api.is_none() {
            match HidApi::new() {
                Ok(context) => api = Some(context),
                Err(error) => {
                    shared.report(format!("HID initialization: {error}"));
                    shared.wait(RETRY_WAIT);
                    continue;
                }
            }
        }
        match connect(api.as_mut().expect("Initialized HID context"), &shared) {
            Ok(Some(mut connected)) => {
                shared.device_connected.store(true, Ordering::Release);
                if let Err(error) = connected.run(&shared)
                    && !shared.stopping()
                {
                    shared.report(format!("Logitech connection: {error}"));
                }
                shared.device_connected.store(false, Ordering::Release);
                shared.release_all();
                drop(connected);
            }
            Ok(None) => {}
            Err(error) => {
                if !shared.stopping() {
                    shared.report(format!("Logitech access: {error}"));
                }
            }
        }
        shared.device_connected.store(false, Ordering::Release);
        shared.command(Command::Device(None));
        if !shared.stopping() {
            shared.wait(RETRY_WAIT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use superlight_core::{actions::Platform, config, session::Divert};

    #[test]
    fn a_known_name_never_overrides_an_explicit_keyboard_identity() {
        assert!(!verified_mouse(true, Some(0)));
        assert!(!verified_mouse(false, None));
        assert!(verified_mouse(true, None));
        assert!(verified_mouse(false, Some(3)));
        assert!(verified_mouse(false, Some(5)));
        assert!(!verified_mouse(true, Some(4)));
    }

    #[test]
    fn diversion_requires_permissions_and_an_active_mapping() {
        let mut config = config::defaults();
        config["profiles"]["default"]["mappings"]["gesture_left"] = json!("copy");
        let policy = Policy::compile(&config, "default", Platform::MacOs, false).unwrap();
        let status = DeviceStatus {
            dpi_min: 200,
            dpi_max: 4000,
            supports_dpi: true,
            supports_gesture: true,
            supports_mode_shift: true,
            ..DeviceStatus::default()
        };
        assert_eq!(
            Desired::from_config(&config, &policy, &status, false).diverts,
            [false; 3]
        );
        assert_eq!(
            Desired::from_config(&config, &policy, &status, true).diverts,
            [true, true, false]
        );
        let paused = Policy {
            paused: true,
            ..policy
        };
        assert_eq!(
            Desired::from_config(&config, &paused, &status, true).diverts,
            [false; 3]
        );
        config["settings"]["dpi"] = json!(16000);
        assert_eq!(
            Desired::from_config(&config, &policy, &status, true).dpi,
            Some(4000)
        );
    }

    fn report(params: &[u8]) -> Report {
        let mut parameters = [0; 16];
        parameters[..params.len()].copy_from_slice(params);
        Report {
            device: 2,
            feature: 7,
            function: 0,
            software: 0,
            parameters,
            len: params.len() as u8,
        }
    }

    #[test]
    fn cancelling_a_held_gesture_does_not_turn_its_release_into_a_click() {
        let (shared, events, _) = Shared::new(config::defaults(), "test".into(), true).unwrap();
        shared.native_ready.store(true, Ordering::Release);
        shared.device_connected.store(true, Ordering::Release);
        let mut pump = Pump::new(Arc::clone(&shared), 2, 7);
        pump.configure([
            Some(Divert {
                cid: 0xc3,
                raw_xy: true,
            }),
            None,
            None,
        ]);
        pump.process(report(&[0, 0xc3]));
        assert!(matches!(
            events.recv().unwrap().event,
            Input::GesturePress { .. }
        ));
        shared.release_all();
        shared.emergency.store(false, Ordering::Release);
        pump.process(report(&[0, 0]));
        assert!(!pump.busy());
        assert!(
            events
                .try_iter()
                .all(|event| !matches!(event.event, Input::GestureRelease))
        );
    }

    #[test]
    fn unrelated_receiver_slots_never_reach_the_output_queue() {
        let (shared, events, _) = Shared::new(config::defaults(), "test".into(), true).unwrap();
        shared.native_ready.store(true, Ordering::Release);
        shared.device_connected.store(true, Ordering::Release);
        let mut pump = Pump::new(shared, 2, 7);
        pump.configure([
            Some(Divert {
                cid: 0xc3,
                raw_xy: true,
            }),
            None,
            None,
        ]);
        let mut unrelated = report(&[0, 0xc3]);
        unrelated.device = 3;
        pump.process(unrelated);
        assert!(events.is_empty());
        assert!(!pump.busy());
    }
}

#[cfg(test)]
#[path = "hardware_fixtures.rs"]
mod hardware_fixtures;
