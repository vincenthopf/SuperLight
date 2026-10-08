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
