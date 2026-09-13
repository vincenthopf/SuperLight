use serde::{Deserialize, Serialize};
use serde_json::Value;
use superlight_core::hidpp::{Control, SmartShift};

pub const PROTOCOL_VERSION: u8 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    Get,
    Apply {
        expected_revision: u64,
        config: Value,
    },
    SetPaused {
        value: bool,
    },
    Reconnect,
    RefreshHardware,
    RequestPermissions,
    ShowSettings,
    Quit,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    pub pid: u32,
    pub name: String,
    pub id: String,
    pub aliases: Vec<String>,
    pub input_restricted: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Permissions {
    pub listen: bool,
    pub inject: bool,
    pub description: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceStatus {
    pub name: String,
    pub model_key: String,
    pub layout_key: String,
    pub product_id: u16,
    pub receiver_slot: u8,
    pub transport: String,
    pub backend: String,
    pub dpi: Option<u16>,
    pub dpi_min: u16,
    pub dpi_max: u16,
    pub smart_shift: Option<SmartShift>,
    pub battery: Option<u8>,
    pub supports_dpi: bool,
    pub supports_smart_shift: bool,
    pub supports_gesture: bool,
    pub supports_mode_shift: bool,
    pub supports_dpi_switch: bool,
    pub raw_xy: bool,
    pub controls: Vec<Control>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub instance: String,
    pub revision: u64,
    pub config: Value,
    pub active_profile: String,
    pub foreground: AppInfo,
    pub paused: bool,
    pub suspended: bool,
    pub native_ready: bool,
    pub permissions: Permissions,
    pub device: Option<DeviceStatus>,
    pub hardware_pending: bool,
    pub dropped_events: u64,
    pub errors: Vec<String>,
    pub notice: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub protocol: u8,
    pub ok: bool,
    pub error: Option<String>,
    pub snapshot: Option<Snapshot>,
}

impl Response {
    pub fn success(snapshot: Snapshot) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            ok: true,
            error: None,
            snapshot: Some(snapshot),
        }
    }

    pub fn failure(error: impl ToString) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            ok: false,
            error: Some(error.to_string().chars().take(1024).collect()),
            snapshot: None,
        }
    }
}
