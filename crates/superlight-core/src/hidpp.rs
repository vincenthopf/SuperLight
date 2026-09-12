use serde::{Deserialize, Serialize};
use std::fmt;

pub const VENDOR: u16 = 0x046d;
pub const SOFTWARE: u8 = 0x0a;
pub const SHORT_ID: u8 = 0x10;
pub const LONG_ID: u8 = 0x11;
pub const LONG_LEN: usize = 20;
pub const ROOT: u16 = 0x0000;
pub const REPROG: u16 = 0x1b04;
pub const DPI: u16 = 0x2201;
pub const SMART_SHIFT: u16 = 0x2110;
pub const SMART_SHIFT_ENHANCED: u16 = 0x2111;
pub const UNIFIED_BATTERY: u16 = 0x1004;
pub const BATTERY_STATUS: u16 = 0x1000;
pub const DEVICE_NAME: u16 = 0x0005;
pub const GESTURE_CIDS: [u16; 2] = [0x00c3, 0x00d7];
pub const MODE_SHIFT_CID: u16 = 0x00c4;
pub const DPI_SWITCH_CID: u16 = 0x00fd;
pub const DEVICE_INDICES: [u8; 7] = [0xff, 1, 2, 3, 4, 5, 6];
pub const MAX_CONTROLS: usize = 32;

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
    if params.len() > 16 {
        return Err(ProtocolError::TooManyParameters);
    }
    if function > 15 {
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
    if function > 15 || message.device != device {
        return ResponseMatch::Unrelated;
    }
    if matches!(message.feature, 0xff | 0x8f) {
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
        if controls.iter().any(|c| c.cid == cid) && !result.contains(&cid) {
            result.push(cid);
        }
    }
    for control in controls.iter().take(MAX_CONTROLS) {
        let raw_xy = control.flags & 0x0300 != 0 || control.mapping_flags & 0x0050 != 0;
        let virtual_gesture = control.flags & 0x0080 != 0 || GESTURE_CIDS.contains(&control.cid);
        if raw_xy
            && virtual_gesture
            && control.flags & 0x0020 != 0
            && !result.contains(&control.cid)
        {
            result.push(control.cid);
        }
    }
    if result.is_empty() {
        preferred.to_vec()
    } else {
        result
    }
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
    } else if product_id == 0xc548 {
        "Logi Bolt"
    } else {
        "USB Receiver"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_sends_long_ble_reports() {
        let packet = encode(255, 3, 2, &[1, 2, 3]).unwrap();
        assert_eq!(packet.len(), 20);
        assert_eq!(&packet[..7], &[17, 255, 3, 42, 1, 2, 3]);
        assert!(packet[7..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn rejects_truncating_writes() {
        assert_eq!(
            encode(255, 0, 0, &[1; 17]),
            Err(ProtocolError::TooManyParameters)
        );
        assert_eq!(encode(255, 0, 16, &[]), Err(ProtocolError::InvalidFunction));
    }

    #[test]
    fn supports_id_and_idless_reads() {
        let packet = encode(255, 5, 4, &[1, 2, 3]).unwrap();
        assert_eq!(parse(&packet), parse(&packet[1..]));
        for len in 0..4 {
            assert!(parse(&packet[..len]).is_none());
        }
        assert!(parse(&[0; 65]).is_none());
    }

    #[test]
    fn matches_firmware_response_function_quirk() {
        for function in 0..16 {
            let normal = encode(255, 3, function, &[]).unwrap();
            let adjacent = encode(255, 3, (function + 1) & 15, &[]).unwrap();
            assert_eq!(
                match_response(parse(&normal).unwrap(), 255, 3, function),
                ResponseMatch::Reply
            );
            assert_eq!(
                match_response(parse(&adjacent).unwrap(), 255, 3, function),
                ResponseMatch::Reply
            );
        }
        let packet = encode(255, 3, 0, &[]).unwrap();
        assert_eq!(
            match_response(parse(&packet).unwrap(), 255, 3, 255),
            ResponseMatch::Unrelated
        );
    }

    #[test]
    fn other_slots_and_software_cannot_complete_requests() {
        let packet = encode(2, 3, 1, &[]).unwrap();
        assert_eq!(
            match_response(parse(&packet).unwrap(), 1, 3, 1),
            ResponseMatch::Unrelated
        );
        let notification = [17, 1, 3, 16, 0, 0];
        assert_eq!(
            match_response(parse(&notification).unwrap(), 1, 3, 1),
            ResponseMatch::Unrelated
        );
    }

    #[test]
    fn unrelated_errors_are_not_request_failures() {
        let error = [17, 1, 255, 9, 42, 7];
        assert_eq!(
            match_response(parse(&error).unwrap(), 1, 9, 2),
            ResponseMatch::Error(7)
        );
        assert_eq!(
            match_response(parse(&error).unwrap(), 2, 9, 2),
            ResponseMatch::Unrelated
        );
        assert_eq!(
            match_response(parse(&error).unwrap(), 1, 8, 2),
            ResponseMatch::Unrelated
        );
        assert_eq!(
            match_response(parse(&error).unwrap(), 1, 9, 3),
            ResponseMatch::Unrelated
        );
    }

    #[test]
    fn freespin_never_enables_smart_shift() {
        for threshold in 0..=255 {
            assert!(!SmartShift::decode(1, threshold).enabled);
        }
        assert!(SmartShift::decode(2, 1).enabled);
        assert!(SmartShift::decode(2, 50).enabled);
        assert!(!SmartShift::decode(2, 51).enabled);
        assert_eq!(
            SmartShift::parameters(ScrollMode::Ratchet, false, 25),
            [2, 255, 0]
        );
    }

    #[test]
    fn mode_switch_keeps_threshold_and_disables_auto() {
        let mut state = SmartShift {
            mode: ScrollMode::Ratchet,
            enabled: true,
            threshold: 37,
        };
        state.switch_mode();
        assert_eq!(
            state,
            SmartShift {
                mode: ScrollMode::Freespin,
                enabled: false,
                threshold: 37
            }
        );
    }

    #[test]
    fn cid_decoding_stops_at_terminator() {
        assert!(contains_cid(&[0, 195, 0, 196, 0, 0], 196));
        assert!(!contains_cid(&[0, 195, 0, 0, 0, 196], 196));
        assert!(!contains_cid(&[0], 195));
    }

    #[test]
    fn signed_xy_covers_every_sixteen_bit_value() {
        for value in 0..=u16::MAX {
            let bytes = value.to_be_bytes();
            let xy = signed_xy(&[bytes[0], bytes[1], bytes[0], bytes[1]]).unwrap();
            assert_eq!(xy, (value as i16, value as i16));
        }
    }

    #[test]
    fn control_flags_have_separated_high_byte() {
        let control = Control::decode(2, &[0, 195, 0, 1, 176, 0, 0, 0, 3]).unwrap();
        assert_eq!(control.flags, 944);
        assert_eq!(control.cid, 195);
        assert!(Control::decode(0, &[0; 8]).is_none());
    }

    #[test]
    fn gesture_preference_and_capability_fallback() {
        let controls = [
            Control {
                cid: 215,
                flags: 944,
                ..Control::default()
            },
            Control {
                cid: 195,
                flags: 304,
                ..Control::default()
            },
        ];
        assert_eq!(gesture_candidates(&controls, &[]), [195, 215]);
        let controls = [Control {
            cid: 241,
            flags: 432,
            ..Control::default()
        }];
        assert_eq!(gesture_candidates(&controls, &[]), [241]);
        assert_eq!(gesture_candidates(&[], &[]), [195, 215]);
    }
}
