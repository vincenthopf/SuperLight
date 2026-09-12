use serde_json::json;
use superlight_core::{actions::Platform, config, policy::Policy};
use superlight_service::hardware::Desired;

#[test]
fn unavailable_native_input_never_diverts_mouse_controls() {
    let mut value = config::defaults();
    value["profiles"]["default"]["mappings"]["gesture_up"] = json!("copy");
    value["profiles"]["default"]["mappings"]["dpi_switch"] = json!("cycle_dpi");
    let policy = Policy::compile(&value, "default", Platform::MacOs, false).unwrap();
    let desired = Desired::new(&value, &policy, false, 200, 4000);
    assert_eq!(desired.divert, [false; 3]);
    let desired = Desired::new(&value, &policy, true, 200, 4000);
    assert_eq!(desired.divert, [true; 3]);
}

#[test]
fn paused_profiles_release_all_diversions() {
    let value = config::defaults();
    let policy = Policy::compile(&value, "default", Platform::Windows, true).unwrap();
    assert_eq!(Desired::new(&value, &policy, true, 200, 8000).divert, [false; 3]);
}

#[test]
fn settings_are_bounded_by_the_connected_model() {
    let mut value = config::defaults();
    let policy = Policy::default();
    value["settings"]["dpi"] = json!(16000);
    assert_eq!(Desired::new(&value, &policy, false, 200, 4000).dpi, 4000);
    value["settings"]["dpi"] = json!(-10);
    assert_eq!(Desired::new(&value, &policy, false, 200, 8000).dpi, 200);
}

#[test]
fn smart_shift_retains_the_configured_fallback_mode() {
    let mut value = config::defaults();
    value["settings"]["smart_shift_mode"] = json!("freespin");
    value["settings"]["smart_shift_enabled"] = json!(true);
    let desired = Desired::new(&value, &Policy::default(), true, 200, 8000);
    assert_eq!(desired.smart_shift.wire_parameters(), [2, 25, 0]);
    assert_eq!(desired.smart_shift.mode, superlight_core::hidpp::ScrollMode::Freespin);
}
