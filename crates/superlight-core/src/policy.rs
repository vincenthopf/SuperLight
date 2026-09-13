use crate::{
    actions::{Action, Platform},
    config, gesture,
    hidpp::{ScrollMode, SmartShift},
};
use serde_json::Value;

#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub mappings: [Action; 12],
    pub paused: bool,
    pub invert_vertical: bool,
    pub invert_horizontal: bool,
    pub ignore_trackpad: bool,
    pub horizontal_threshold: f64,
    pub gestures: gesture::Options,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            mappings: [Action::None; 12],
            paused: true,
            invert_vertical: false,
            invert_horizontal: false,
            ignore_trackpad: true,
            horizontal_threshold: 1.0,
            gestures: gesture::Options::default(),
        }
    }
}

impl Policy {
    pub fn compile(
        value: &Value,
        profile: &str,
        platform: Platform,
        paused: bool,
    ) -> Result<Self, String> {
        let mappings = value["profiles"]
            .get(profile)
            .or_else(|| value["profiles"].get("default"))
            .and_then(|profile| profile["mappings"].as_object())
            .ok_or("Missing profile mappings")?;
        let mut policy = Self {
            paused,
            invert_vertical: boolean(value, "invert_vscroll", false),
            invert_horizontal: boolean(value, "invert_hscroll", false),
            ignore_trackpad: boolean(value, "ignore_trackpad", true),
            horizontal_threshold: number(value, "hscroll_threshold", 1.0).clamp(0.1, 10_000.0),
            ..Self::default()
        };
        for (index, name) in config::BUTTONS.iter().enumerate() {
            if let Some(action) = mappings.get(*name).and_then(Value::as_str) {
                policy.mappings[index] = Action::parse(action, platform)?;
            }
        }
        policy.gestures = gesture::Options {
            enabled: !paused
                && policy.mappings[8..]
                    .iter()
                    .any(|action| *action != Action::None),
            threshold: number(value, "gesture_threshold", 50.0).clamp(5.0, 100_000.0),
            deadzone: number(value, "gesture_deadzone", 40.0).clamp(0.0, 100_000.0),
            timeout_ms: number(value, "gesture_timeout_ms", 3000.0).clamp(250.0, 60_000.0) as u64,
            cooldown_ms: number(value, "gesture_cooldown_ms", 500.0).clamp(0.0, 60_000.0) as u64,
            prefer_hid: platform == Platform::MacOs,
        };
        Ok(policy)
    }

    pub fn action(&self, index: usize) -> Action {
        if self.paused {
            Action::None
        } else {
            self.mappings.get(index).copied().unwrap_or_default()
        }
    }
}

pub fn number(value: &Value, name: &str, default: f64) -> f64 {
    value["settings"][name]
        .as_f64()
        .filter(|value| value.is_finite())
        .unwrap_or(default)
}

pub fn boolean(value: &Value, name: &str, default: bool) -> bool {
    value["settings"][name].as_bool().unwrap_or(default)
}

pub fn smart_shift(value: &Value) -> SmartShift {
    SmartShift {
        mode: if value["settings"]["smart_shift_mode"] == "freespin" {
            ScrollMode::Freespin
        } else {
            ScrollMode::Ratchet
        },
        enabled: boolean(value, "smart_shift_enabled", false),
        threshold: number(value, "smart_shift_threshold", 25.0).clamp(1.0, 50.0) as u8,
    }
}

pub fn needs_divert(value: &Value, button: &str) -> bool {
    value["profiles"].as_object().is_some_and(|profiles| {
        profiles.values().any(|profile| {
            profile["mappings"][button]
                .as_str()
                .is_some_and(|action| action != "none" && !action.is_empty())
        })
    })
}

pub fn validate_actions(value: &Value, platform: Platform) -> Result<(), String> {
    for name in value["profiles"]
        .as_object()
        .ok_or("Missing profiles")?
        .keys()
    {
        Policy::compile(value, name, platform, false)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_profiles_compile_without_hardware() {
        for platform in [Platform::MacOs, Platform::Windows, Platform::Linux] {
            let policy = Policy::compile(&config::defaults(), "default", platform, false).unwrap();
            assert_ne!(policy.action(2), Action::None);
            assert_eq!(policy.action(0), Action::None);
            assert!(!policy.gestures.enabled);
        }
    }

    #[test]
    fn pause_and_invalid_button_are_pass_through() {
        let policy =
            Policy::compile(&config::defaults(), "default", Platform::MacOs, true).unwrap();
        for index in 0..100 {
            assert_eq!(policy.action(index), Action::None);
        }
    }

    #[test]
    fn clamps_unsafe_settings_and_validates_inactive_profiles() {
        let mut value = config::defaults();
        value["settings"]["gesture_threshold"] = json!(-10);
        value["settings"]["gesture_timeout_ms"] = json!(-100);
        value["profiles"]["default"]["mappings"]["gesture_up"] = json!("copy");
        let policy = Policy::compile(&value, "default", Platform::MacOs, false).unwrap();
        assert_eq!(policy.gestures.threshold, 5.0);
        assert_eq!(policy.gestures.timeout_ms, 250);
        assert!(policy.gestures.enabled);
        value["profiles"]["inactive"] = json!({"mappings": {"middle": "custom:badkey"}});
        assert!(validate_actions(&value, Platform::MacOs).is_err());
    }

    #[test]
    fn inactive_profiles_still_request_device_diversion() {
        let mut value = config::defaults();
        assert!(!needs_divert(&value, "dpi_switch"));
        value["profiles"]["work"] = json!({"mappings": {"dpi_switch": "cycle_dpi"}});
        assert!(needs_divert(&value, "dpi_switch"));
    }
}
