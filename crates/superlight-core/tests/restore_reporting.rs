use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use superlight_core::{
    hidpp::{self, ProtocolError},
    session::{Error, Session, Transport},
};

#[derive(Default)]
struct State {
    pending: VecDeque<Vec<u8>>,
    writes: Vec<[u8; 20]>,
    reject_restore: bool,
}

struct Fake(Arc<Mutex<State>>);

impl Transport for Fake {
    fn write_report(&mut self, report: &[u8; 20]) -> Result<(), Error> {
        let mut state = self.0.lock().unwrap();
        state.writes.push(*report);
        let restoring = matches!(report[6], 0x02 | 0x22);
        if restoring {
            state.pending.push_back(vec![0x11, 2, 7, 0, 0, 0]);
        }
        if restoring && state.reject_restore {
            state.pending.push_back(vec![0x11, 2, 0xff, 7, 0x3a, 8]);
        } else {
            state
                .pending
                .push_back(vec![0x11, 2, 7, 0x3a, report[4], report[5], report[6]]);
        }
        Ok(())
    }

    fn read_wait(&mut self, buffer: &mut [u8; 64], _: Duration) -> Result<usize, Error> {
        let report = self.0.lock().unwrap().pending.pop_front().ok_or_else(|| {
            Error::Transport("The request consumed more replies than the fake device sent".into())
        })?;
        buffer[..report.len()].copy_from_slice(&report);
        Ok(report.len())
    }
}

fn setup() -> (Session<Fake>, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    let mut session = Session::new(Fake(Arc::clone(&state)), 2);
    session.features.reprog = Some(7);
    (session, state)
}

#[test]
fn restoring_raw_motion_consumes_the_acknowledgement_and_delivers_interleaved_input() {
    let (mut session, state) = setup();
    session.divert_gesture(&[0xc3], &mut |_| {}).unwrap();
    let mut notifications = Vec::new();
    session
        .restore_slot(0, &mut |report| notifications.push(report))
        .unwrap();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].software, 0);
    assert!(session.diverts()[0].is_none());
    let state = state.lock().unwrap();
    assert!(state.pending.is_empty());
    assert_eq!(&state.writes[1][4..9], &[0, 0xc3, 0x22, 0, 0]);
}

#[test]
fn a_rejected_restore_keeps_the_diversion_for_shutdown_cleanup() {
    let (mut session, state) = setup();
    session
        .divert_extra(hidpp::MODE_SHIFT_CID, &mut |_| {})
        .unwrap();
    state.lock().unwrap().reject_restore = true;
    assert_eq!(
        session.restore_slot(1, &mut |_| {}),
        Err(Error::Protocol(ProtocolError::Device(8)))
    );
    assert!(session.diverts()[1].is_some());
    drop(session);
    let state = state.lock().unwrap();
    assert_eq!(state.writes.len(), 3);
    assert_eq!(&state.writes[2][4..9], &[0, 0xc4, 0x02, 0, 0]);
}

#[test]
fn restoring_an_unused_slot_does_not_issue_a_hardware_write() {
    let (mut session, state) = setup();
    session.restore_slot(2, &mut |_| {}).unwrap();
    assert!(state.lock().unwrap().writes.is_empty());
}
