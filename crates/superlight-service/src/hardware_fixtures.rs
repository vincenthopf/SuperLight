use super::*;
use serde::Deserialize;
use std::{collections::VecDeque, ffi::CString};

#[derive(Clone, Deserialize)]
struct Fixture {
    id: String,
    provenance: String,
    pid: u16,
    name: String,
    kind: u8,
    controls: Vec<Vec<u8>>,
    features: Vec<u16>,
    expected: Expected,
}

#[derive(Clone, Deserialize)]
struct Expected {
    supported: bool,
    gesture: bool,
    mode_shift: bool,
    dpi_switch: bool,
    dpi: bool,
    smart_shift: bool,
}

struct Replay {
    fixture: Fixture,
    replies: VecDeque<Vec<u8>>,
    idless: bool,
}

impl Transport for Replay {
    fn write_report(&mut self, report: &[u8; 20]) -> Result<(), Error> {
        let request = hidpp::parse(report).unwrap();
        let feature = |id| {
            self.fixture
                .features
                .iter()
                .position(|f| *f == id)
                .map(|i| i as u8 + 2)
        };
        let params = if request.feature == 0 {
            assert_eq!(request.function, 0);
            let id = u16::from_be_bytes([request.params[0], request.params[1]]);
            vec![if id == hidpp::DEVICE_NAME {
                1
            } else {
                feature(id).unwrap_or(0)
            }]
        } else if request.feature == 1 {
            match request.function {
                0 => vec![self.fixture.name.len() as u8],
                1 => self.fixture.name.as_bytes()[usize::from(request.params[0])..]
                    .iter()
                    .take(16)
                    .copied()
                    .collect(),
                2 => vec![self.fixture.kind],
                _ => panic!("unexpected name request"),
            }
        } else if Some(request.feature) == feature(hidpp::REPROG) {
            match request.function {
                0 => vec![self.fixture.controls.len() as u8],
                1 => self.fixture.controls[usize::from(request.params[0])].clone(),
                2 => vec![request.params[0], request.params[1], 0, 0, 0, 0],
                _ => panic!("discovery must not divert controls"),
            }
        } else {
            panic!("discovery must not write hardware settings");
        };
        let mut reply = vec![
            0x11,
            request.device,
            request.feature,
            request.function << 4 | hidpp::SOFTWARE,
        ];
        reply.extend(params);
        reply.resize(20, 0);
        if self.idless {
            reply.remove(0);
        }
        self.replies.push_back(reply);
        Ok(())
    }

    fn read_wait(&mut self, buffer: &mut [u8; 64], _: Duration) -> Result<usize, Error> {
        let reply = self.replies.pop_front().expect("read without request");
        buffer[..reply.len()].copy_from_slice(&reply);
        Ok(reply.len())
    }
}

#[test]
fn protocol_fixtures_probe_every_transport_without_guessing_capabilities() {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!("fixtures/mice.json")).unwrap();
    for fixture in fixtures {
        assert!(fixture.provenance.starts_with("synthetic"));
        for (pid, bluetooth, slot) in [
            (fixture.pid, true, 0xff),
            (0xc548, false, 1),
            (0xc52b, false, 6),
            (fixture.pid, false, 0xff),
        ] {
            for idless in [false, true] {
                let candidate = Candidate {
                    path: CString::new("fixture").unwrap(),
                    product_id: pid,
                    name: fixture.name.clone(),
                    bluetooth,
                    usage_page: 0xff00,
                    usage: 2,
                    interface: 0,
                };
                let mut session = Session::new(
                    Replay {
                        fixture: fixture.clone(),
                        replies: VecDeque::new(),
                        idless,
                    },
                    slot,
                );
                let result = probe(&mut session, &candidate);
                if !fixture.expected.supported {
                    assert!(matches!(result, Err(Error::Unsupported)), "{}", fixture.id);
                    continue;
                }
                let (status, gestures) = result.unwrap_or_else(|e| panic!("{}: {e}", fixture.id));
                let e = &fixture.expected;
                assert_eq!(
                    (
                        status.supports_gesture,
                        status.supports_mode_shift,
                        status.supports_dpi_switch,
                        status.supports_dpi,
                        status.supports_smart_shift
                    ),
                    (e.gesture, e.mode_shift, e.dpi_switch, e.dpi, e.smart_shift),
                    "{}",
                    fixture.id
                );
                assert_eq!(status.receiver_slot, slot);
                assert_eq!(status.controls.len(), fixture.controls.len());
                if fixture.id == "m720" {
                    assert_eq!(gestures[0], 0xd0);
                }
                if fixture.id == "generic-trackball" {
                    assert_eq!(status.model_key, "generic");
                }
                let value = superlight_core::config::defaults();
                let policy = Policy::compile(
                    &value,
                    "default",
                    superlight_core::actions::Platform::MacOs,
                    false,
                )
                .unwrap();
                let desired = Desired::from_config(&value, &policy, &status, true);
                for (enabled, supported) in
                    desired
                        .diverts
                        .into_iter()
                        .zip([e.gesture, e.mode_shift, e.dpi_switch])
                {
                    assert!(!enabled || supported);
                }
                assert_eq!(
                    Desired::from_config(&value, &policy, &status, false).diverts,
                    [false; 3]
                );
            }
        }
    }
}

#[test]
fn gesture_report_fixtures_ignore_foreign_input_and_clear_on_disconnect() {
    use superlight_core::session::Divert;
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!("fixtures/mice.json")).unwrap();
    for fixture in fixtures
        .into_iter()
        .filter(|fixture| fixture.expected.gesture)
    {
        let controls: Vec<_> = fixture
            .controls
            .iter()
            .enumerate()
            .map(|(i, bytes)| hidpp::Control::decode(i as u8, bytes).unwrap())
            .collect();
        let spec = devices::resolve(fixture.pid, &fixture.name).unwrap();
        let cid = hidpp::gesture_candidates(&controls, spec.gesture_cids)[0];
        for slot in [0xff, 1, 6] {
            let mut state = Notifications::new(slot, 9);
            state.configure([Some(Divert { cid, raw_xy: true }), None, None]);
            let mut events = Vec::new();
            let [high, low] = cid.to_be_bytes();
            let packets = [
                [0x11, 2, 9, 0, high, low, 0, 0],
                [0x11, slot, 8, 0, high, low, 0, 0],
                [0x11, slot, 9, hidpp::SOFTWARE, high, low, 0, 0],
                [0x11, slot, 9, 0, high, low, 0, 0],
                [0x11, slot, 9, 0, high, low, 0, 0],
                [0x11, slot, 9, 0x10, 0xff, 0xfe, 0, 7],
            ];
            for packet in packets {
                state.process(
                    Report::from_message(hidpp::parse(&packet).unwrap()),
                    |event| events.push(event),
                );
            }
            assert_eq!(
                events,
                [
                    DeviceEvent::Button {
                        source: 1,
                        down: true
                    },
                    DeviceEvent::Motion { x: -2, y: 7 }
                ],
                "{}",
                fixture.id
            );
            state.clear();
            events.clear();
            for packet in [
                [0x11, slot, 9, 0x10, 0, 1, 0, 1],
                [0x11, slot, 9, 0, 0, 0, 0, 0],
            ] {
                state.process(
                    Report::from_message(hidpp::parse(&packet).unwrap()),
                    |event| events.push(event),
                );
            }
            assert!(events.is_empty());
            state.process(
                Report::from_message(hidpp::parse(&[0x11, slot, 9, 0, high, low, 0, 0]).unwrap()),
                |event| events.push(event),
            );
            assert_eq!(
                events,
                [DeviceEvent::Button {
                    source: 1,
                    down: true
                }]
            );
        }
    }
}
