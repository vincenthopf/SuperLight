from pathlib import Path


def patch(path, before, after):
    file = Path(path)
    text = file.read_text()
    if before in text:
        assert text.count(before) == 1, path
        file.write_text(text.replace(before, after))
    elif after not in text:
        raise RuntimeError(f"Source changed unexpectedly: {path}")


def write(path, content):
    file = Path(path)
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(content)


patch("crates/superlight-core/src/lib.rs", "pub mod queue;", "pub mod queue;\npub mod reports;")

write("crates/superlight-core/src/reports.rs", r'''use crate::{hidpp, session::{Divert, Report}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceEvent {
    Button { source: u8, down: bool },
    Motion { x: i16, y: i16 },
}

pub struct Notifications {
    device: u8,
    feature: u8,
    diverts: [Option<Divert>; 3],
    held: [bool; 3],
}

impl Notifications {
    pub fn new(device: u8, feature: u8) -> Self {
        Self { device, feature, diverts: [None; 3], held: [false; 3] }
    }

    pub fn configure(&mut self, diverts: [Option<Divert>; 3]) {
        for (index, divert) in diverts.iter().enumerate() {
            if *divert != self.diverts[index] { self.held[index] = false; }
        }
        self.diverts = diverts;
    }

    pub fn clear(&mut self) { self.held.fill(false); }

    pub fn process(&mut self, report: Report, mut emit: impl FnMut(DeviceEvent)) {
        if report.device != self.device || report.feature != self.feature || report.software != 0 { return; }
        if report.function == 0 && report.len >= 2 {
            for (index, source) in [1, 6, 7].into_iter().enumerate() {
                let down = self.diverts[index].is_some_and(|divert| hidpp::contains_cid(report.params(), divert.cid));
                if down != self.held[index] {
                    self.held[index] = down;
                    emit(DeviceEvent::Button { source, down });
                }
            }
        } else if report.function == 1 && self.held[0]
            && self.diverts[0].is_some_and(|divert| divert.raw_xy)
            && let Some((x, y)) = hidpp::signed_xy(report.params()) {
            emit(DeviceEvent::Motion { x, y });
        }
    }
}

#[derive(Clone, Copy)]
struct Packet {
    bytes: [u8; 64],
    len: u8,
}

const EMPTY_PACKET: Packet = Packet { bytes: [0; 64], len: 0 };
pub const PACKET_CAPACITY: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueFault { Full, InvalidLength }

pub struct PacketQueue {
    packets: [Packet; PACKET_CAPACITY],
    head: usize,
    len: usize,
}

impl Default for PacketQueue {
    fn default() -> Self { Self { packets: [EMPTY_PACKET; PACKET_CAPACITY], head: 0, len: 0 } }
}

impl PacketQueue {
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), QueueFault> {
        if bytes.is_empty() || bytes.len() > 64 { return Err(QueueFault::InvalidLength); }
        if self.len == PACKET_CAPACITY { return Err(QueueFault::Full); }
        let index = (self.head + self.len) % PACKET_CAPACITY;
        self.packets[index].bytes[..bytes.len()].copy_from_slice(bytes);
        self.packets[index].len = bytes.len() as u8;
        self.len += 1;
        Ok(())
    }

    pub fn pop_into(&mut self, buffer: &mut [u8; 64]) -> Option<usize> {
        if self.len == 0 { return None; }
        let packet = self.packets[self.head];
        let len = usize::from(packet.len);
        buffer[..len].copy_from_slice(&packet.bytes[..len]);
        self.head = (self.head + 1) % PACKET_CAPACITY;
        self.len -= 1;
        Some(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(device: u8, feature: u8, function: u8, software: u8, params: &[u8]) -> Report {
        let mut parameters = [0; 16];
        parameters[..params.len()].copy_from_slice(params);
        Report { device, feature, function, software, parameters, len: params.len() as u8 }
    }

    fn state() -> Notifications {
        let mut state = Notifications::new(2, 7);
        state.configure([
            Some(Divert { cid: 0xc3, raw_xy: true }),
            Some(Divert { cid: 0xc4, raw_xy: false }),
            Some(Divert { cid: 0xfd, raw_xy: false }),
        ]);
        state
    }

    #[test]
    fn simultaneous_controls_have_independent_edges() {
        let mut state = state();
        let mut events = Vec::new();
        let down = report(2, 7, 0, 0, &[0, 0xc3, 0, 0xc4, 0, 0xfd, 0, 0]);
        state.process(down, |event| events.push(event));
        state.process(down, |event| events.push(event));
        state.process(report(2, 7, 0, 0, &[0, 0xc4, 0, 0]), |event| events.push(event));
        assert_eq!(events, [
            DeviceEvent::Button { source: 1, down: true },
            DeviceEvent::Button { source: 6, down: true },
            DeviceEvent::Button { source: 7, down: true },
            DeviceEvent::Button { source: 1, down: false },
            DeviceEvent::Button { source: 7, down: false },
        ]);
    }

    #[test]
    fn replies_other_slots_and_other_features_never_generate_input() {
        let mut state = state();
        for message in [
            report(1, 7, 0, 0, &[0, 0xc3]),
            report(2, 8, 0, 0, &[0, 0xc3]),
            report(2, 7, 0, 0xa, &[0, 0xc3]),
            report(2, 7, 2, 0, &[0, 0xc3]),
            report(2, 7, 0, 0, &[]),
        ] { state.process(message, |_| panic!("Unrelated report reached input")); }
    }

    #[test]
    fn raw_motion_requires_a_held_raw_xy_control() {
        let mut state = state();
        let movement = report(2, 7, 1, 0, &[0xff, 0xfe, 0x80, 0]);
        state.process(movement, |_| panic!("Motion before press"));
        state.process(report(2, 7, 0, 0, &[0, 0xc3]), |_| {});
        let mut events = Vec::new();
        state.process(movement, |event| events.push(event));
        assert_eq!(events, [DeviceEvent::Motion { x: -2, y: -32768 }]);
        state.configure([Some(Divert { cid: 0xc3, raw_xy: false }), None, None]);
        state.process(movement, |_| panic!("Motion without raw diversion"));
    }

    #[test]
    fn disconnect_cancellation_never_turns_into_a_gesture_click() {
        let mut state = state();
        state.process(report(2, 7, 0, 0, &[0, 0xc3]), |_| {});
        state.clear();
        state.process(report(2, 7, 0, 0, &[0, 0]), |_| panic!("Cancellation generated a click"));
        let mut events = Vec::new();
        state.process(report(2, 7, 0, 0, &[0, 0xc3]), |event| events.push(event));
        assert_eq!(events, [DeviceEvent::Button { source: 1, down: true }]);
    }

    #[test]
    fn packet_queue_is_bounded_and_never_overwrites_unread_reports() {
        let mut queue = PacketQueue::default();
        let mut buffer = [0; 64];
        for value in 0..PACKET_CAPACITY { queue.push(&[value as u8]).unwrap(); }
        for _ in 0..100_000 { assert_eq!(queue.push(&[255]), Err(QueueFault::Full)); }
        for value in 0..PACKET_CAPACITY {
            assert_eq!(queue.pop_into(&mut buffer), Some(1));
            assert_eq!(buffer[0], value as u8);
        }
        assert_eq!(queue.pop_into(&mut buffer), None);
        assert!(std::mem::size_of::<PacketQueue>() < 9000);
    }

    #[test]
    fn packet_queue_wraparound_and_length_validation() {
        let mut queue = PacketQueue::default();
        let mut buffer = [0; 64];
        assert_eq!(queue.push(&[]), Err(QueueFault::InvalidLength));
        assert_eq!(queue.push(&[0; 65]), Err(QueueFault::InvalidLength));
        for value in 0..10_000_u16 {
            let bytes = value.to_be_bytes();
            queue.push(&bytes).unwrap();
            assert_eq!(queue.pop_into(&mut buffer), Some(2));
            assert_eq!(&buffer[..2], &bytes);
        }
    }
}
''')

patch("crates/superlight-core/src/input.rs", "#[derive(Default)]\npub struct Router {\n    captured: [Option<Action>; SOURCE_COUNT],\n}", "#[derive(Clone, Copy)]\nenum Capture { Active(Action), Cancelled }\n\n#[derive(Default)]\npub struct Router {\n    captured: [Option<Capture>; SOURCE_COUNT],\n}")
patch("crates/superlight-core/src/input.rs", "if slot.is_some() { return Decision::Block; }", "if matches!(slot, Some(Capture::Active(_))) { return Decision::Block; }\n            *slot = None;")
patch("crates/superlight-core/src/input.rs", "*slot = Some(action);", "*slot = Some(Capture::Active(action));")
patch("crates/superlight-core/src/input.rs", "} else if let Some(captured) = slot.take() {\n            if matches!(captured, Action::Mouse(_))", "} else if let Some(captured) = slot.take() {\n            let Capture::Active(captured) = captured else { return Decision::Block; };\n            if matches!(captured, Action::Mouse(_))")
patch("crates/superlight-core/src/input.rs", "    pub fn captured_mouse(&self, source: usize) -> Option<u8> {\n        match self.captured.get(source).copied().flatten() { Some(Action::Mouse(button)) => Some(button), _ => None }\n    }", "    pub fn cancel(&mut self) {\n        for slot in &mut self.captured {\n            if slot.is_some() { *slot = Some(Capture::Cancelled); }\n        }\n    }\n\n    pub fn captured_mouse(&self, source: usize) -> Option<u8> {\n        match self.captured.get(source).copied().flatten() { Some(Capture::Active(Action::Mouse(button))) => Some(button), _ => None }\n    }")
patch("crates/superlight-core/src/input.rs", "mod tests {\n    use super::*;", "mod tests {\n    use super::*;\n\n    #[test]\n    fn cancelled_capture_blocks_orphan_release_but_accepts_a_new_press() {\n        let mut router = Router::default();\n        router.route(0, true, Action::Mouse(0), |_| true);\n        router.cancel();\n        assert_eq!(router.captured_mouse(0), None);\n        assert_eq!(router.route(0, false, Action::None, |_| panic!()), Decision::Block);\n        router.route(0, true, Action::Mouse(0), |_| true);\n        router.cancel();\n        let mut events = Vec::new();\n        assert_eq!(router.route(0, true, Action::Mouse(1), |event| { events.push(event); true }), Decision::Block);\n        assert_eq!(events[0].action, Action::Mouse(1));\n    }")
patch("crates/superlight-core/src/session.rs", "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub struct Report", "impl<T: Transport + ?Sized> Transport for Box<T> {\n    fn write_report(&mut self, report: &[u8; 20]) -> Result<(), Error> { (**self).write_report(report) }\n    fn read_wait(&mut self, buffer: &mut [u8; 64], timeout: Duration) -> Result<usize, Error> { (**self).read_wait(buffer, timeout) }\n}\n\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub struct Report")
patch("crates/superlight-ipc/src/store.rs", "        let durable = atomic_write(&self.paths.config, &bytes)?;\n        self.value = value;\n        self.revision = self.revision.checked_add(1).ok_or_else(|| io::Error::other(\"Configuration revision exhausted\"))?;", "        let next_revision = self.revision.checked_add(1).ok_or_else(|| io::Error::other(\"Configuration revision exhausted\"))?;\n        let durable = atomic_write(&self.paths.config, &bytes)?;\n        self.value = value;\n        self.revision = next_revision;")
