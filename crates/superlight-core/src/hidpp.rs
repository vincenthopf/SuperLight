use serde::{Deserialize, Serialize};
use std::{fmt, ops::RangeInclusive};

pub const VENDOR: u16 = 0x046d;
pub const SOFTWARE: u8 = 0x0a;
pub const SHORT_ID: u8 = 0x10;
pub const LONG_ID: u8 = 0x11;
pub const LONG_LEN: usize = 20;
pub const MAX_PARAMS: usize = LONG_LEN - 4;
pub const MAX_FUNCTION: u8 = 15;
pub const NOTIFICATION_SOFTWARE: u8 = 0;
pub const HIDPP20_ERROR: u8 = 0xff;
pub const HIDPP10_ERROR: u8 = 0x8f;
pub const ERROR_INVALID_FEATURE_INDEX: u8 = 6;
pub const ERROR_INVALID_FUNCTION: u8 = 7;
pub const ERROR_UNSUPPORTED: u8 = 9;
pub const ROOT: u16 = 0x0000;
pub const REPROG: u16 = 0x1b04;
pub const DPI: u16 = 0x2201;
pub const SMART_SHIFT: u16 = 0x2110;
pub const SMART_SHIFT_ENHANCED: u16 = 0x2111;
pub const UNIFIED_BATTERY: u16 = 0x1004;
pub const BATTERY_STATUS: u16 = 0x1000;
pub const DEVICE_NAME: u16 = 0x0005;
pub const MOUSE_GESTURE_CID: u16 = 0x00c3;
pub const VIRTUAL_GESTURE_CID: u16 = 0x00d7;
pub const MULTIPLATFORM_GESTURE_CID: u16 = 0x00d0;
pub const GESTURE_CIDS: [u16; 2] = [MOUSE_GESTURE_CID, VIRTUAL_GESTURE_CID];
pub const MODE_SHIFT_CID: u16 = 0x00c4;
pub const DPI_SWITCH_CID: u16 = 0x00fd;
pub const DEVICE_INDICES: [u8; 7] = [0xff, 1, 2, 3, 4, 5, 6];
pub const MAX_CONTROLS: usize = 32;
pub const DIVERTED_BUTTONS_EVENT: u8 = 0;
pub const RAW_XY_EVENT: u8 = 1;
pub const KEY_DIVERTABLE: u16 = 0x0020;
pub const KEY_VIRTUAL: u16 = 0x0080;
pub const KEY_RAW_XY: u16 = 0x0100;
pub const KEY_FORCE_RAW_XY: u16 = 0x0200;
pub const MAPPING_RAW_XY_DIVERTED: u16 = 0x0010;
pub const MAPPING_FORCE_RAW_XY_DIVERTED: u16 = 0x0040;
pub const DIVERT: u8 = 0x03;
pub const DIVERT_RAW_XY: u8 = 0x33;
pub const UNDIVERT: u8 = 0x02;
pub const UNDIVERT_RAW_XY: u8 = 0x22;
pub const BLUETOOTH_PRODUCT_IDS: RangeInclusive<u16> = 0xb000..=0xbfff;
pub const BOLT_RECEIVER_PID: u16 = 0xc548;
pub const VENDOR_USAGE_PAGE: u16 = 0xff00;
pub const HIDPP_USAGE: u16 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    TooManyParameters,
    InvalidFunction,
    Device(u8),
    Malformed,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyParameters => f.write_str("HID++ accepts at most 16 parameter bytes"),
            Self::InvalidFunction => f.write_str("HID++ function must be in 0..=15"),
            Self::Device(code) => write!(f, "HID++ device error 0x{code:02x}"),
            Self::Malformed => f.write_str("Malformed HID++ report"),
        }
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Message<'a> {
    pub device: u8,
    pub feature: u8,
    pub function: u8,
    pub software: u8,
    pub params: &'a [u8],
}

pub fn parse(raw: &[u8]) -> Option<Message<'_>> {
    if raw.len() < 4 || raw.len() > 64 {
        return None;
    }
    let offset = usize::from(matches!(raw[0], SHORT_ID | LONG_ID));
    Some(Message {
        device: raw[offset],
        feature: raw[offset + 1],
        function: raw[offset + 2] >> 4,
        software: raw[offset + 2] & 15,
        params: &raw[offset + 3..],
    })
}

pub fn encode(
    device: u8,
    feature: u8,
    function: u8,
    params: &[u8],
) -> Result<[u8; LONG_LEN], ProtocolError> {
    if params.len() > MAX_PARAMS {
        return Err(ProtocolError::TooManyParameters);
    }
    if function > MAX_FUNCTION {
        return Err(ProtocolError::InvalidFunction);
    }
    let mut report = [0; LONG_LEN];
    report[..4].copy_from_slice(&[LONG_ID, device, feature, function << 4 | SOFTWARE]);
    report[4..4 + params.len()].copy_from_slice(params);
    Ok(report)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseMatch {
    Reply,
    Error(u8),
    Unrelated,
}

pub fn match_response(
    message: Message<'_>,
    device: u8,
    feature: u8,
    function: u8,
) -> ResponseMatch {
    if function > MAX_FUNCTION || message.device != device {
        return ResponseMatch::Unrelated;
    }
    if matches!(message.feature, HIDPP20_ERROR | HIDPP10_ERROR) {
        let echoed_feature = message.function << 4 | message.software;
        if echoed_feature == feature
            && message.params.len() >= 2
            && message.params[0] == (function << 4 | SOFTWARE)
        {
            return ResponseMatch::Error(message.params[1]);
        }
        return ResponseMatch::Unrelated;
    }
    if message.feature == feature
        && message.software == SOFTWARE
        && (message.function == function || message.function == ((function + 1) & 15))
    {
        ResponseMatch::Reply
    } else {
        ResponseMatch::Unrelated
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollMode {
    Ratchet,
    Freespin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SmartShift {
    pub mode: ScrollMode,
    pub enabled: bool,
    pub threshold: u8,
}

impl Default for SmartShift {
    fn default() -> Self {
        Self {
            mode: ScrollMode::Ratchet,
            enabled: false,
            threshold: 25,
        }
    }
}

impl SmartShift {
    pub fn decode(mode: u8, auto_disengage: u8) -> Self {
        let valid = (1..=50).contains(&auto_disengage);
        Self {
            mode: if mode == 1 {
                ScrollMode::Freespin
            } else {
                ScrollMode::Ratchet
            },
            enabled: mode != 1 && valid,
            threshold: if valid { auto_disengage } else { 25 },
        }
    }

    pub fn parameters(mode: ScrollMode, enabled: bool, threshold: i64) -> [u8; 3] {
        if enabled {
            [2, threshold.clamp(1, 50) as u8, 0]
        } else if mode == ScrollMode::Freespin {
            [1, 0, 0]
        } else {
            [2, 255, 0]
        }
    }

    pub fn wire_parameters(self) -> [u8; 3] {
        Self::parameters(self.mode, self.enabled, i64::from(self.threshold))
    }

    pub fn switch_mode(&mut self) {
        self.mode = match self.mode {
            ScrollMode::Ratchet => ScrollMode::Freespin,
            ScrollMode::Freespin => ScrollMode::Ratchet,
        };
        self.enabled = false;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Control {
    pub index: u8,
    pub cid: u16,
    pub task: u16,
    pub flags: u16,
    pub mapping_flags: u16,
    pub mapped_to: u16,
}

impl Control {
    pub fn decode(index: u8, params: &[u8]) -> Option<Self> {
        if params.len() < 9 {
            return None;
        }
        let cid = u16::from_be_bytes([params[0], params[1]]);
        Some(Self {
            index,
            cid,
            task: u16::from_be_bytes([params[2], params[3]]),
            flags: u16::from_le_bytes([params[4], params[8]]),
            mapping_flags: 0,
            mapped_to: cid,
        })
    }

    pub fn apply_reporting(&mut self, params: &[u8]) {
        if params.len() < 5 {
            return;
        }
        self.mapping_flags = u16::from_le_bytes([params[2], params.get(5).copied().unwrap_or(0)]);
        let mapped = u16::from_be_bytes([params[3], params[4]]);
        let original = u16::from_be_bytes([params[0], params[1]]);
        self.mapped_to = if mapped != 0 {
            mapped
        } else if original != 0 {
            original
        } else {
            self.cid
        };
    }
}

pub fn gesture_candidates(controls: &[Control], preferred: &[u16]) -> Vec<u16> {
    let preferred = if preferred.is_empty() {
        &GESTURE_CIDS
    } else {
        preferred
    };
    let mut result = Vec::with_capacity(MAX_CONTROLS);
    for &cid in preferred {
        if controls
            .iter()
            .any(|c| c.cid == cid && c.flags & KEY_DIVERTABLE != 0)
            && !result.contains(&cid)
        {
            result.push(cid);
        }
    }
    for control in controls.iter().take(MAX_CONTROLS) {
        let raw_xy = control.flags & (KEY_RAW_XY | KEY_FORCE_RAW_XY) != 0
            || control.mapping_flags & (MAPPING_RAW_XY_DIVERTED | MAPPING_FORCE_RAW_XY_DIVERTED)
                != 0;
        let virtual_gesture =
            control.flags & KEY_VIRTUAL != 0 || GESTURE_CIDS.contains(&control.cid);
        if raw_xy
            && virtual_gesture
            && control.flags & KEY_DIVERTABLE != 0
            && !result.contains(&control.cid)
        {
            result.push(control.cid);
        }
    }
    result
}

pub fn signed_xy(params: &[u8]) -> Option<(i16, i16)> {
    if params.len() < 4 {
        return None;
    }
    Some((
        i16::from_be_bytes([params[0], params[1]]),
        i16::from_be_bytes([params[2], params[3]]),
    ))
}

pub fn contains_cid(params: &[u8], cid: u16) -> bool {
    params
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes(*pair))
        .take_while(|value| *value != 0)
        .any(|value| value == cid)
}

pub fn transport_label(device_index: u8, product_id: u16) -> &'static str {
    if device_index == 255 {
        "Bluetooth"
    } else if product_id == BOLT_RECEIVER_PID {
        "Logi Bolt"
    } else {
        "USB Receiver"
    }
}
