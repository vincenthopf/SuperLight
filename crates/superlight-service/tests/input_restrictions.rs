use std::sync::atomic::Ordering;
use superlight_core::config;
use superlight_service::shared::Shared;

#[test]
fn entering_secure_input_invalidates_queued_input_and_updates_hardware_policy() {
    let (shared, _inputs, _commands) =
        Shared::new(config::defaults(), "test".into(), true).unwrap();
    shared.native_ready.store(true, Ordering::Release);
    shared.device_connected.store(true, Ordering::Release);
    let generation = shared.generation.load(Ordering::Acquire);
    let epoch = shared.input_epoch.load(Ordering::Acquire);
    assert!(shared.allowed());
    shared.set_input_restricted(true);
    assert!(!shared.allowed());
    assert!(shared.restricted.load(Ordering::Acquire));
    assert_eq!(shared.generation.load(Ordering::Acquire), generation + 1);
    assert_eq!(shared.input_epoch.load(Ordering::Acquire), epoch + 1);
}

#[test]
fn leaving_secure_input_reconfigures_hardware_even_when_the_profile_is_unchanged() {
    let (shared, _inputs, _commands) =
        Shared::new(config::defaults(), "test".into(), true).unwrap();
    shared.set_input_restricted(true);
    let generation = shared.generation.load(Ordering::Acquire);
    let epoch = shared.input_epoch.load(Ordering::Acquire);
    shared.set_input_restricted(false);
    assert!(!shared.restricted.load(Ordering::Acquire));
    assert_eq!(shared.generation.load(Ordering::Acquire), generation + 1);
    assert_eq!(shared.input_epoch.load(Ordering::Acquire), epoch);
}

#[test]
fn repeated_foreground_checks_do_not_issue_redundant_hardware_changes() {
    let (shared, _inputs, _commands) =
        Shared::new(config::defaults(), "test".into(), true).unwrap();
    let generation = shared.generation.load(Ordering::Acquire);
    for _ in 0..1000 {
        shared.set_input_restricted(false);
    }
    assert_eq!(shared.generation.load(Ordering::Acquire), generation);
    shared.set_input_restricted(true);
    for _ in 0..1000 {
        shared.set_input_restricted(true);
    }
    assert_eq!(shared.generation.load(Ordering::Acquire), generation + 1);
}
