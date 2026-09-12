use serde_json::{Map, Value, json};

pub const VERSION: u64 = 9;
pub const BUTTONS: [&str; 12] = ["middle", "gesture", "xbutton1", "xbutton2", "hscroll_left", "hscroll_right", "mode_shift", "dpi_switch", "gesture_left", "gesture_right", "gesture_up", "gesture_down"];
pub const DIRECTIONS: [&str; 4] = ["gesture_left", "gesture_right", "gesture_up", "gesture_down"];

pub fn defaults() -> Value {
    json!({
        "version": 9,
        "active_profile": "default",
        "profiles": {
            "default": {
                "label": "Default (All Apps)", "apps": [],
                "mappings": {
                    "middle": "none", "gesture": "none",
                    "gesture_left": "none", "gesture_right": "none",
                    "gesture_up": "none", "gesture_down": "none",
                    "xbutton1": "alt_tab", "xbutton2": "alt_tab",
                    "hscroll_left": "browser_back", "hscroll_right": "browser_forward",
                    "mode_shift": "switch_scroll_mode"
                }
            }
        },
        "settings": {
            "start_minimized": true, "start_at_login": false,
            "hscroll_threshold": 1, "invert_hscroll": false, "invert_vscroll": false,
            "dpi": 1000, "smart_shift_mode": "ratchet", "smart_shift_enabled": false,
            "smart_shift_threshold": 25, "gesture_threshold": 50, "gesture_deadzone": 40,
            "gesture_timeout_ms": 3000, "gesture_cooldown_ms": 500,
            "appearance_mode": "system", "debug_mode": false,
            "device_layout_overrides": {}, "language": "en", "ignore_trackpad": true
        }
    })
}

fn object<'a>(parent: &'a mut Map<String, Value>, key: &str) -> Result<&'a mut Map<String, Value>, String> {
    parent.entry(key).or_insert_with(|| json!({})).as_object_mut().ok_or_else(|| format!("{key} must be an object"))
}

fn set_missing(target: &mut Map<String, Value>, key: &str, value: Value) {
    target.entry(key).or_insert(value);
}

fn compatible_type(value: &Value, default: &Value) -> bool {
    match default {
        Value::Null => value.is_null(),
        Value::Bool(_) => value.is_boolean(),
        Value::Number(number) if number.is_i64() || number.is_u64() => value.is_i64() || value.is_u64(),
        Value::Number(_) => value.is_number(),
        Value::String(_) => value.is_string(),
        Value::Array(_) => value.is_array(),
        Value::Object(_) => value.is_object(),
    }
}

fn merge(target: &mut Value, default: &Value) {
    if let (Some(target), Some(default)) = (target.as_object_mut(), default.as_object()) {
        for (key, value) in default {
            let entry = target.entry(key).or_insert_with(|| value.clone());
            if !compatible_type(entry, value) { *entry = value.clone(); }
            else if value.is_object() { merge(entry, value); }
        }
    }
}

pub fn migrate(mut value: Value) -> Result<Value, String> {
    let root = value.as_object_mut().ok_or("Configuration must be an object")?;
    let version = root.get("version").and_then(Value::as_u64).unwrap_or(1);
    let profiles = object(root, "profiles")?;
    if profiles.len() > 64 { return Err("At most 64 application profiles are supported".into()); }
    for profile in profiles.values_mut() {
        let profile = profile.as_object_mut().ok_or("Each profile must be an object")?;
        if version < 2 { set_missing(profile, "apps", json!([])); }
        let mappings = object(profile, "mappings")?;
        if version < 3 {
            set_missing(mappings, "gesture", json!("none"));
            for direction in DIRECTIONS { set_missing(mappings, direction, json!("none")); }
        }
        if version < 6 { set_missing(mappings, "mode_shift", json!("none")); }
        if version < 7 && mappings.get("mode_shift").and_then(Value::as_str) == Some("none") {
            mappings.insert("mode_shift".into(), json!("toggle_smart_shift"));
        }
        if version < 8 && mappings.get("mode_shift").and_then(Value::as_str) == Some("toggle_smart_shift") {
            mappings.insert("mode_shift".into(), json!("switch_scroll_mode"));
        }
        if let Some(apps) = profile.get_mut("apps").and_then(Value::as_array_mut) {
            for app in apps {
                if app.as_str().is_some_and(|name| name.eq_ignore_ascii_case("wmplayer.exe")) { *app = json!("Microsoft.Media.Player.exe"); }
            }
        }
    }
    let settings = object(root, "settings")?;
    if version < 2 {
        set_missing(settings, "invert_hscroll", json!(false));
        set_missing(settings, "invert_vscroll", json!(false));
        set_missing(settings, "dpi", json!(1000));
    }
    if version < 3 {
        for (key, value) in [("gesture_threshold", 50), ("gesture_deadzone", 40), ("gesture_timeout_ms", 3000), ("gesture_cooldown_ms", 500)] {
            set_missing(settings, key, json!(value));
        }
    }
    if version < 5 {
        let old_start = settings.remove("start_with_windows").unwrap_or(json!(false));
        let enabled = match old_start {
            Value::Bool(value) => value,
            Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
            Value::String(value) => !value.is_empty(), Value::Array(value) => !value.is_empty(),
            Value::Object(value) => !value.is_empty(), Value::Null => false,
        };
        set_missing(settings, "start_at_login", json!(enabled));
    }
    for (key, value) in [("appearance_mode", json!("system")), ("debug_mode", json!(false)), ("device_layout_overrides", json!({})), ("language", json!("en")), ("ignore_trackpad", json!(true))] {
        set_missing(settings, key, value);
    }
    if version < VERSION { root.insert("version".into(), json!(VERSION)); }
    merge(&mut value, &defaults());
    validate(&value)?;
    Ok(value)
}

pub fn parse(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > crate::CONFIG_LIMIT { return Err("Configuration exceeds 1 MiB".into()); }
    migrate(serde_json::from_slice(bytes).map_err(|error| error.to_string())?)
}

pub fn validate(value: &Value) -> Result<(), String> {
    let profiles = value.get("profiles").and_then(Value::as_object).ok_or("profiles must be an object")?;
    if !profiles.contains_key("default") || profiles.len() > 64 { return Err("Configuration needs a default profile and at most 64 profiles".into()); }
    for (name, profile) in profiles {
        if name.is_empty() || name.len() > 256 { return Err("Profile names must have 1..=256 bytes".into()); }
        if let Some(apps) = profile.get("apps") {
            let apps = apps.as_array().ok_or("Profile apps must be an array")?;
            if apps.len() > 128 || apps.iter().any(|app| app.as_str().is_none_or(|app| app.len() > 4096)) { return Err("Invalid application list".into()); }
        }
        let mappings = profile.get("mappings").and_then(Value::as_object).ok_or("Profile mappings must be an object")?;
        if mappings.len() > 64 || mappings.values().any(|action| action.as_str().is_none_or(|action| action.len() > 512)) { return Err("Invalid button mappings".into()); }
    }
    Ok(())
}

pub fn profile_for_aliases<'a>(value: &'a Value, aliases: &[String]) -> &'a str {
    if let Some(profiles) = value.get("profiles").and_then(Value::as_object) {
        for (name, profile) in profiles {
            if profile.get("apps").and_then(Value::as_array).is_some_and(|apps| {
                apps.iter().filter_map(Value::as_str).any(|app| aliases.iter().any(|alias| app.to_lowercase() == alias.to_lowercase()))
            }) { return name; }
        }
    }
    "default"
}

pub fn active_mappings(value: &Value) -> Option<&Map<String, Value>> {
    let name = value.get("active_profile").and_then(Value::as_str).unwrap_or("default");
    value.get("profiles")?.get(name).or_else(|| value.get("profiles")?.get("default"))?.get("mappings")?.as_object()
}

pub fn delete_profile(value: &mut Value, name: &str) -> Result<(), String> {
    if name == "default" { return Err("The default profile cannot be removed".into()); }
    value.get_mut("profiles").and_then(Value::as_object_mut).ok_or("Missing profiles")?.remove(name);
    if value.get("active_profile").and_then(Value::as_str) == Some(name) { value["active_profile"] = json!("default"); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_migrates_to_current_defaults() { assert_eq!(migrate(json!({})).unwrap(), defaults()); }

    #[test]
    fn mode_shift_promotes_without_overwriting_user_actions() {
        for (version, action, expected) in [(6, "none", "switch_scroll_mode"), (7, "toggle_smart_shift", "switch_scroll_mode"), (8, "none", "none"), (1, "copy", "copy")] {
            let value = migrate(json!({"version": version, "profiles": {"default": {"mappings": {"mode_shift": action}}}})).unwrap();
            assert_eq!(value["profiles"]["default"]["mappings"]["mode_shift"], expected);
        }
    }

    #[test]
    fn keeps_smart_shift_fallback_and_unknown_fields() {
        let value = migrate(json!({"version": 8, "settings": {"smart_shift_mode": "freespin", "smart_shift_enabled": true, "future": 7}, "future": {"enabled": true}})).unwrap();
        assert_eq!(value["settings"]["smart_shift_mode"], "freespin");
        assert_eq!(value["future"]["enabled"], true);
        assert_eq!(value["settings"]["future"], 7);
    }

    #[test]
    fn login_migration_preserves_explicit_new_setting() {
        let value = migrate(json!({"version": 4, "settings": {"start_with_windows": true, "start_at_login": false}})).unwrap();
        assert_eq!(value["settings"]["start_at_login"], false);
        assert!(value["settings"].get("start_with_windows").is_none());
    }

    #[test]
    fn malformed_and_oversized_are_errors_not_data_loss() {
        assert!(parse(b"{broken").is_err());
        assert!(parse(b"[]").is_err());
        assert!(parse(&vec![b' '; crate::CONFIG_LIMIT + 1]).is_err());
        assert!(migrate(json!({"profiles": []})).is_err());
    }

    #[test]
    fn default_cannot_be_deleted() {
        let mut value = defaults();
        assert!(delete_profile(&mut value, "default").is_err());
        value["profiles"]["work"] = json!({"mappings": {}});
        value["active_profile"] = json!("work");
        delete_profile(&mut value, "work").unwrap();
        assert_eq!(value["active_profile"], "default");
    }

    #[test]
    fn first_application_profile_wins_case_insensitively() {
        let mut value = defaults();
        value["profiles"]["first"] = json!({"apps": ["Code.exe"], "mappings": {}});
        value["profiles"]["second"] = json!({"apps": ["code.exe"], "mappings": {}});
        assert_eq!(profile_for_aliases(&value, &["CODE.EXE".into()]), "first");
        assert_eq!(profile_for_aliases(&value, &[]), "default");
    }
}
