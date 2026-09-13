use serde_json::json;
use superlight_core::{actions::Platform, config, policy::Policy};
use superlight_ipc::DeviceStatus;
use superlight_service::hardware::Desired;

fn mouse(max: u16) -> DeviceStatus {
    DeviceStatus {
        dpi_min: 200,
        dpi_max: max,
        supports_dpi: true,
        supports_smart_shift: true,
        supports_gesture: true,
        supports_mode_shift: true,
        supports_dpi_switch: true,
        ..DeviceStatus::default()
    }
}

#[test]
fn unavailable_native_input_never_diverts_mouse_controls() {
    let mut value = config::defaults();
    value["profiles"]["default"]["mappings"]["gesture_up"] = json!("copy");
    value["profiles"]["default"]["mappings"]["dpi_switch"] = json!("cycle_dpi");
    let policy = Policy::compile(&value, "default", Platform::MacOs, false).unwrap();
    let desired = Desired::from_config(&value, &policy, &mouse(4000), false);
    assert_eq!(desired.diverts, [false; 3]);
    let desired = Desired::from_config(&value, &policy, &mouse(4000), true);
    assert_eq!(desired.diverts, [true; 3]);
}

#[test]
fn paused_profiles_release_all_diversions() {
    let value = config::defaults();
    let policy = Policy::compile(&value, "default", Platform::Windows, true).unwrap();
    assert_eq!(
        Desired::from_config(&value, &policy, &mouse(8000), true).diverts,
        [false; 3]
    );
}

#[test]
fn settings_are_bounded_by_the_connected_model() {
    let mut value = config::defaults();
    let policy = Policy::default();
    value["settings"]["dpi"] = json!(16000);
    assert_eq!(
        Desired::from_config(&value, &policy, &mouse(4000), false).dpi,
        Some(4000)
    );
    value["settings"]["dpi"] = json!(-10);
    assert_eq!(
        Desired::from_config(&value, &policy, &mouse(8000), false).dpi,
        Some(200)
    );
}

#[test]
fn smart_shift_retains_the_configured_fallback_mode() {
    let mut value = config::defaults();
    value["settings"]["smart_shift_mode"] = json!("freespin");
    value["settings"]["smart_shift_enabled"] = json!(true);
    let desired = Desired::from_config(&value, &Policy::default(), &mouse(8000), true);
    let state = desired.smart_shift.unwrap();
    assert_eq!(state.wire_parameters(), [2, 25, 0]);
    assert_eq!(state.mode, superlight_core::hidpp::ScrollMode::Freespin);
}
