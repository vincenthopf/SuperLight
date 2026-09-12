use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Duration};
use superlight_core::{hidpp::{self, ProtocolError, ScrollMode, SmartShift}, session::{Error, Report, Session, Transport}};

#[derive(Default)]
struct State {
    reads: VecDeque<Vec<u8>>,
    writes: Vec<[u8; 20]>,
    fail_write: bool,
}

struct Fake(Rc<RefCell<State>>);

impl Transport for Fake {
    fn write_report(&mut self, report: &[u8; 20]) -> Result<(), Error> {
        let mut state = self.0.borrow_mut();
        state.writes.push(*report);
        if state.fail_write { Err(Error::Transport("Disconnected".into())) } else { Ok(()) }
    }

    fn read_wait(&mut self, buffer: &mut [u8; 64], timeout: Duration) -> Result<usize, Error> {
        if let Some(bytes) = self.0.borrow_mut().reads.pop_front() {
            let len = bytes.len().min(buffer.len());
            buffer[..len].copy_from_slice(&bytes[..len]);
            Ok(bytes.len())
        } else {
            std::thread::sleep(timeout);
            Ok(0)
        }
    }
}

fn setup(reads: Vec<Vec<u8>>) -> (Session<Fake>, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State { reads: reads.into(), ..State::default() }));
    let mut session = Session::new(Fake(Rc::clone(&state)), 255);
    session.timeout = Duration::from_millis(2);
    (session, state)
}

fn reply(device: u8, feature: u8, function: u8, params: &[u8]) -> Vec<u8> {
    hidpp::encode(device, feature, function, params).unwrap().to_vec()
}

fn error(feature: u8, function: u8, code: u8) -> Vec<u8> {
    vec![17, 255, 255, feature, function << 4 | 10, code, 0]
}

#[test]
fn forwards_button_release_while_waiting_for_a_setting_reply() {
    let mut notification = reply(255, 5, 0, &[0, 0]);
    notification[3] = 0;
    let (mut session, state) = setup(vec![notification, reply(255, 7, 2, &[])]);
    let mut events = Vec::<Report>::new();
    session.request(7, 2, &[2, 25, 0], &mut |report| events.push(report)).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].feature, 5);
    assert_eq!(events[0].parameters[0..2], [0, 0]);
    assert_eq!(state.borrow().writes[0][..7], [17, 255, 7, 42, 2, 25, 0]);
}

#[test]
fn unrelated_receiver_replies_and_notifications_do_not_escape_the_slot() {
    let mut notification = reply(2, 5, 0, &[0, 195]);
    notification[3] = 0;
    let (mut session, _) = setup(vec![reply(2, 7, 2, &[9]), notification, reply(255, 7, 2, &[3])]);
    let result = session.request(7, 2, &[], &mut |_| panic!("Wrong receiver slot")).unwrap();
    assert_eq!(result.parameters[0], 3);
}

#[test]
fn malformed_reports_are_skipped_and_backend_overruns_are_rejected() {
    let (mut session, _) = setup(vec![vec![17], vec![17, 255, 5], reply(255, 7, 2, &[3])]);
    assert!(session.request(7, 2, &[], &mut |_| {}).is_ok());
    let (mut session, _) = setup(vec![vec![0; 65]]);
    assert_eq!(session.request(7, 2, &[], &mut |_| {}), Err(Error::Protocol(ProtocolError::Malformed)));
}

#[test]
fn three_consecutive_timeouts_request_reconnect_and_a_reply_clears_it() {
    let (mut session, state) = setup(vec![]);
    for _ in 0..2 {
        assert_eq!(session.request(7, 2, &[], &mut |_| {}), Err(Error::Timeout));
        assert!(!session.needs_reconnect());
    }
    assert_eq!(session.request(7, 2, &[], &mut |_| {}), Err(Error::Timeout));
    assert!(session.needs_reconnect());
    state.borrow_mut().reads.push_back(reply(255, 7, 2, &[]));
    session.request(7, 2, &[], &mut |_| {}).unwrap();
    assert!(!session.needs_reconnect());
}

#[test]
fn transport_failure_is_not_misreported_as_a_device_timeout() {
    let (mut session, state) = setup(vec![]);
    state.borrow_mut().fail_write = true;
    assert_eq!(session.request(7, 2, &[], &mut |_| {}), Err(Error::Transport("Disconnected".into())));
    assert!(!session.needs_reconnect());
}

#[test]
fn feature_lookup_preserves_root_big_endian_selector() {
    let (mut session, state) = setup(vec![reply(255, 0, 0, &[9])]);
    assert_eq!(session.feature(0x1b04, &mut |_| {}).unwrap(), Some(9));
    assert_eq!(&state.borrow().writes[0][..7], &[17, 255, 0, 10, 27, 4, 0]);
}

#[test]
fn missing_feature_is_not_a_transport_failure() {
    let (mut session, _) = setup(vec![reply(255, 0, 0, &[0]), error(0, 0, 7)]);
    assert_eq!(session.feature(0x2111, &mut |_| {}).unwrap(), None);
    assert_eq!(session.feature(0x2110, &mut |_| {}).unwrap(), None);
}

#[test]
fn rawxy_rejection_falls_back_to_button_only_diversion() {
    let (mut session, state) = setup(vec![error(5, 3, 7), reply(255, 5, 3, &[])]);
    session.features.reprog = Some(5);
    let diverted = session.divert_gesture(&[195, 215], &mut |_| {}).unwrap().unwrap();
    assert_eq!(diverted.cid, 195);
    assert!(!diverted.raw_xy);
    session.shutdown();
    let flags: Vec<_> = state.borrow().writes.iter().map(|packet| packet[6]).collect();
    assert_eq!(flags, [0x33, 0x03, 0x02]);
    assert!(session.diverts().iter().all(Option::is_none));
}

#[test]
fn next_gesture_candidate_is_tried_without_changing_preference_order() {
    let (mut session, state) = setup(vec![error(5, 3, 7), error(5, 3, 7), reply(255, 5, 3, &[])]);
    session.features.reprog = Some(5);
    let diverted = session.divert_gesture(&[195, 215], &mut |_| {}).unwrap().unwrap();
    assert_eq!(diverted.cid, 215);
    assert!(diverted.raw_xy);
    drop(session);
    let state = state.borrow();
    assert_eq!(state.writes.len(), 4);
    assert_eq!(&state.writes[3][4..9], &[0, 215, 0x22, 0, 0]);
}

#[test]
fn destructor_restores_extra_buttons_before_gesture() {
    let (mut session, state) = setup(vec![reply(255, 5, 3, &[]), reply(255, 5, 3, &[]), reply(255, 5, 3, &[])]);
    session.features.reprog = Some(5);
    session.divert_gesture(&[195], &mut |_| {}).unwrap();
    session.divert_extra(hidpp::MODE_SHIFT_CID, &mut |_| {}).unwrap();
    session.divert_extra(hidpp::DPI_SWITCH_CID, &mut |_| {}).unwrap();
    drop(session);
    let state = state.borrow();
    assert_eq!(state.writes.len(), 6);
    assert_eq!(&state.writes[3][4..9], &[0, 196, 2, 0, 0]);
    assert_eq!(&state.writes[4][4..9], &[0, 253, 2, 0, 0]);
    assert_eq!(&state.writes[5][4..9], &[0, 195, 34, 0, 0]);
}

#[test]
fn retargeting_is_rejected_until_active_diverts_are_restored() {
    let (mut session, _) = setup(vec![reply(255, 5, 3, &[])]);
    session.features.reprog = Some(5);
    session.divert_gesture(&[195], &mut |_| {}).unwrap();
    assert!(session.retarget(2).is_err());
    session.shutdown();
    session.retarget(2).unwrap();
    assert_eq!(session.device_index, 2);
    assert_eq!(session.features.reprog, None);
}

#[test]
fn repeated_divert_requests_do_not_generate_redundant_device_writes() {
    let (mut session, state) = setup(vec![reply(255, 5, 3, &[])]);
    session.features.reprog = Some(5);
    for _ in 0..100 { session.divert_gesture(&[195], &mut |_| {}).unwrap(); }
    assert_eq!(state.borrow().writes.len(), 1);
}

#[test]
fn control_count_is_capped_and_mapping_flags_use_both_bytes() {
    let mut reads = vec![reply(255, 5, 0, &[255])];
    for index in 0..32 {
        reads.push(reply(255, 5, 1, &[0, index, 0, 1, 0x30, 0, 0, 0, 3]));
        reads.push(reply(255, 5, 2, &[0, index, 0x51, 0, index, 1]));
    }
    let (mut session, state) = setup(reads);
    session.features.reprog = Some(5);
    let controls = session.discover_controls(&mut |_| {}).unwrap();
    assert_eq!(controls.len(), 32);
    assert_eq!(controls[4].flags, 0x0330);
    assert_eq!(controls[4].mapping_flags, 0x0151);
    assert_eq!(state.borrow().writes.len(), 65);
}

#[test]
fn control_discovery_stops_after_three_failures() {
    let (mut session, state) = setup(vec![reply(255, 5, 0, &[32]), error(5, 1, 7), error(5, 1, 7), error(5, 1, 7)]);
    session.features.reprog = Some(5);
    assert!(session.discover_controls(&mut |_| {}).unwrap().is_empty());
    assert_eq!(state.borrow().writes.len(), 4);
}

#[test]
fn dpi_uses_sensor_zero_and_big_endian_values() {
    let (mut session, state) = setup(vec![reply(255, 6, 3, &[]), reply(255, 6, 2, &[0, 3, 232])]);
    session.features.dpi = Some(6);
    session.set_dpi(1000, &mut |_| {}).unwrap();
    assert_eq!(session.read_dpi(&mut |_| {}).unwrap(), 1000);
    assert_eq!(&state.borrow().writes[0][4..7], &[0, 3, 232]);
    assert_eq!(state.borrow().writes[1][3], 42);
}

#[test]
fn enhanced_smart_shift_uses_distinct_function_ids() {
    let (mut session, state) = setup(vec![reply(255, 7, 2, &[]), reply(255, 7, 1, &[1, 25])]);
    session.features.smart_shift = Some(7);
    session.features.enhanced_smart_shift = true;
    session.set_smart_shift(SmartShift::default(), &mut |_| {}).unwrap();
    assert_eq!(session.read_smart_shift(&mut |_| {}).unwrap(), SmartShift { mode: ScrollMode::Freespin, enabled: false, threshold: 25 });
    assert_eq!(&state.borrow().writes[0][3..7], &[42, 2, 255, 0]);
    assert_eq!(state.borrow().writes[1][3], 26);
}

#[test]
fn unified_and_legacy_battery_queries_use_the_original_functions() {
    let (mut session, state) = setup(vec![reply(255, 8, 1, &[78]), reply(255, 8, 0, &[55]), reply(255, 8, 0, &[255])]);
    session.features.battery = Some(8);
    session.features.unified_battery = true;
    assert_eq!(session.read_battery(&mut |_| {}).unwrap(), 78);
    session.features.unified_battery = false;
    assert_eq!(session.read_battery(&mut |_| {}).unwrap(), 55);
    assert!(session.read_battery(&mut |_| {}).is_err());
    assert_eq!(state.borrow().writes[0][3], 26);
    assert_eq!(state.borrow().writes[1][3], 10);
}

#[test]
fn device_names_are_length_bounded_and_reassembled_from_chunks() {
    let name = b"MX Master 3S for Mac";
    let (mut session, state) = setup(vec![reply(255, 9, 0, &[name.len() as u8]), reply(255, 9, 1, &name[..16]), reply(255, 9, 1, &name[16..])]);
    session.features.name = Some(9);
    assert_eq!(session.read_name(&mut |_| {}).unwrap(), "MX Master 3S for Mac");
    assert_eq!(state.borrow().writes[2][4], 16);
}

#[test]
fn missing_features_never_send_writes_to_root() {
    let (mut session, state) = setup(vec![]);
    assert_eq!(session.set_dpi(1000, &mut |_| {}), Err(Error::Unsupported));
    assert_eq!(session.set_smart_shift(SmartShift::default(), &mut |_| {}), Err(Error::Unsupported));
    assert_eq!(session.read_battery(&mut |_| {}), Err(Error::Unsupported));
    assert!(state.borrow().writes.is_empty());
}
