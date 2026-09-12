use std::collections::BTreeMap;
use superlight_core::hidpp::{self, ResponseMatch};

#[derive(Default)]
pub struct Access {
    slot: Option<u8>,
    features: BTreeMap<u8, u16>,
    pending: Option<u16>,
    confirmed_mouse: bool,
    controls: Vec<u16>,
    dpi: (u16, u16),
}

impl Access {
    pub fn confirm_mouse(&mut self, slot: u8, controls: &[u16], dpi_min: u16, dpi_max: u16) -> Result<(), String> {
        if self.slot != Some(slot) { return Err("Cannot authorize an unprobed receiver slot".into()); }
        self.controls = controls.iter().copied().take(hidpp::MAX_CONTROLS).collect();
        self.dpi = (dpi_min.max(200), dpi_max.clamp(dpi_min.max(200), 8000));
        self.confirmed_mouse = true;
        Ok(())
    }

    pub fn validate(&mut self, report: &[u8; 20]) -> Result<(), String> {
        if report[0] != hidpp::LONG_ID || report[3] & 15 != hidpp::SOFTWARE || !hidpp::DEVICE_INDICES.contains(&report[1]) {
            return Err("Only scoped Logitech HID++ configuration requests are permitted".into());
        }
        let function = report[3] >> 4;
        if report[2] == 0 && function == 0 {
            let id = u16::from_be_bytes([report[4], report[5]]);
            if ![hidpp::REPROG, hidpp::DPI, hidpp::SMART_SHIFT, hidpp::SMART_SHIFT_ENHANCED, hidpp::UNIFIED_BATTERY, hidpp::BATTERY_STATUS, hidpp::DEVICE_NAME].contains(&id) {
                return Err("This HID++ feature is outside SuperLight's mouse configuration scope".into());
            }
            if self.slot != Some(report[1]) {
                *self = Self { slot: Some(report[1]), ..Self::default() };
            }
            self.pending = Some(id);
            return Ok(());
        }
        if self.slot != Some(report[1]) { return Err("The receiver slot has not been verified".into()); }
        let feature = self.features.get(&report[2]).copied().ok_or("The HID++ feature index has not been discovered")?;
        let read_only = match feature {
            hidpp::REPROG | hidpp::DPI | hidpp::DEVICE_NAME => function <= 2,
            hidpp::SMART_SHIFT | hidpp::BATTERY_STATUS => function == 0,
            hidpp::SMART_SHIFT_ENHANCED | hidpp::UNIFIED_BATTERY => function <= 1,
            _ => false,
        };
        if read_only { return Ok(()); }
        if !self.confirmed_mouse { return Err("Configuration writes require a verified mouse, not a keyboard or unverified receiver slot".into()); }
        let permitted = match feature {
            hidpp::REPROG if function == 3 => {
                let cid = u16::from_be_bytes([report[4], report[5]]);
                self.controls.contains(&cid) && [0x02, 0x03, 0x22, 0x33].contains(&report[6]) && report[7..].iter().all(|byte| *byte == 0)
            }
            hidpp::DPI if function == 3 => {
                let dpi = u16::from_be_bytes([report[5], report[6]]);
                report[4] == 0 && (self.dpi.0..=self.dpi.1).contains(&dpi) && report[7..].iter().all(|byte| *byte == 0)
            }
            hidpp::SMART_SHIFT if function == 1 => valid_scroll(&report[4..]),
            hidpp::SMART_SHIFT_ENHANCED if function == 2 => valid_scroll(&report[4..]),
            _ => false,
        };
        if permitted { Ok(()) } else { Err("HID++ configuration request exceeds the verified mouse capabilities".into()) }
    }

    pub fn observe(&mut self, bytes: &[u8]) {
        let Some(id) = self.pending else { return; };
        let Some(slot) = self.slot else { return; };
        let Some(message) = hidpp::parse(bytes) else { return; };
        match hidpp::match_response(message, slot, 0, 0) {
            ResponseMatch::Reply => {
                if let Some(index) = message.params.first().copied().filter(|index| *index != 0) {
                    self.features.insert(index, id);
                }
                self.pending = None;
            }
            ResponseMatch::Error(_) => self.pending = None,
            ResponseMatch::Unrelated => {}
        }
    }
}

fn valid_scroll(params: &[u8]) -> bool {
    matches!(params[0], 1 | 2) && (params[1] == 0 || params[1] == 255 || (1..=50).contains(&params[1])) && params[2..].iter().all(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discover(access: &mut Access, slot: u8, id: u16, index: u8) {
        access.validate(&hidpp::encode(slot, 0, 0, &id.to_be_bytes()).unwrap()).unwrap();
        access.observe(&[0x11, slot, 0, 0x0a, index]);
    }

    #[test]
    fn receiver_queries_do_not_authorize_device_writes() {
        let mut access = Access::default();
        discover(&mut access, 2, hidpp::REPROG, 7);
        let write = hidpp::encode(2, 7, 3, &[0, 0xc3, 0x33]).unwrap();
        assert!(access.validate(&write).is_err());
        access.confirm_mouse(2, &[0xc3], 200, 8000).unwrap();
        assert!(access.validate(&write).is_ok());
        assert!(access.validate(&hidpp::encode(2, 7, 3, &[0, 0xc4, 3]).unwrap()).is_err());
    }

    #[test]
    fn receiver_slot_changes_revoke_configuration_authority() {
        let mut access = Access::default();
        discover(&mut access, 2, hidpp::DPI, 8);
        access.confirm_mouse(2, &[], 200, 4000).unwrap();
        assert!(access.validate(&hidpp::encode(2, 8, 3, &[0, 3, 32]).unwrap()).is_ok());
        discover(&mut access, 3, hidpp::DPI, 8);
        assert!(access.validate(&hidpp::encode(3, 8, 3, &[0, 3, 32]).unwrap()).is_err());
        assert!(access.confirm_mouse(2, &[], 200, 4000).is_err());
    }

    #[test]
    fn firmware_updates_unknown_features_and_out_of_range_dpi_are_denied() {
        let mut access = Access::default();
        assert!(access.validate(&hidpp::encode(1, 0, 0, &[0, 0xc2]).unwrap()).is_err());
        discover(&mut access, 1, hidpp::DPI, 8);
        access.confirm_mouse(1, &[], 200, 4000).unwrap();
        assert!(access.validate(&hidpp::encode(1, 8, 3, &[0, 0x1f, 0x40]).unwrap()).is_err());
        assert!(access.validate(&hidpp::encode(1, 9, 3, &[0, 3, 32]).unwrap()).is_err());
    }
}
