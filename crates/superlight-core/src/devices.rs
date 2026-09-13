use crate::hidpp::GESTURE_CIDS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Family {
    Master,
    Anywhere,
    Vertical,
    Generic,
}

#[derive(Clone, Copy, Debug)]
pub struct DeviceSpec {
    pub key: &'static str,
    pub name: &'static str,
    pub pid: u16,
    pub aliases: &'static [&'static str],
    pub family: Family,
    pub dpi_min: u16,
    pub dpi_max: u16,
    pub image: &'static str,
    pub gesture_cids: &'static [u16],
}

pub static DEVICES: [DeviceSpec; 9] = [
    DeviceSpec {
        key: "mx_master_4",
        name: "MX Master 4",
        pid: 0xb042,
        aliases: &[
            "Logitech MX Master 4",
            "MX Master 4 for Mac",
            "MX_Master_4",
            "MX Master 4 for Business",
        ],
        family: Family::Master,
        dpi_min: 200,
        dpi_max: 8000,
        image: "mouse.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_master_3s",
        name: "MX Master 3S",
        pid: 0xb034,
        aliases: &["Logitech MX Master 3S", "MX Master 3S for Mac"],
        family: Family::Master,
        dpi_min: 200,
        dpi_max: 8000,
        image: "mouse.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_master_3",
        name: "MX Master 3",
        pid: 0xb023,
        aliases: &[
            "Wireless Mouse MX Master 3",
            "MX Master 3 for Mac",
            "MX Master 3 Mac",
        ],
        family: Family::Master,
        dpi_min: 200,
        dpi_max: 8000,
        image: "mouse.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_master_2s",
        name: "MX Master 2S",
        pid: 0xb019,
        aliases: &["Wireless Mouse MX Master 2S"],
        family: Family::Master,
        dpi_min: 200,
        dpi_max: 4000,
        image: "mouse.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_master",
        name: "MX Master",
        pid: 0xb012,
        aliases: &["Wireless Mouse MX Master"],
        family: Family::Master,
        dpi_min: 200,
        dpi_max: 4000,
        image: "mouse.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_vertical",
        name: "MX Vertical",
        pid: 0xb020,
        aliases: &[
            "MX Vertical Wireless Mouse",
            "MX Vertical Advanced Ergonomic Mouse",
        ],
        family: Family::Vertical,
        dpi_min: 200,
        dpi_max: 4000,
        image: "mx_vertical.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_anywhere_3s",
        name: "MX Anywhere 3S",
        pid: 0xb037,
        aliases: &["MX Anywhere 3S for Mac"],
        family: Family::Anywhere,
        dpi_min: 200,
        dpi_max: 8000,
        image: "mouse_mx_anywhere_3s.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_anywhere_3",
        name: "MX Anywhere 3",
        pid: 0xb025,
        aliases: &["MX Anywhere 3 for Mac"],
        family: Family::Anywhere,
        dpi_min: 200,
        dpi_max: 4000,
        image: "mouse_mx_anywhere_3s.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_anywhere_2s",
        name: "MX Anywhere 2S",
        pid: 0xb01a,
        aliases: &["Wireless Mobile Mouse MX Anywhere 2S"],
        family: Family::Anywhere,
        dpi_min: 200,
        dpi_max: 4000,
        image: "mouse_mx_anywhere_3s.png",
        gesture_cids: &GESTURE_CIDS,
    },
];

pub fn normalize_name(name: &str) -> String {
    name.replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn resolve(pid: u16, name: &str) -> Option<&'static DeviceSpec> {
    let normalized = normalize_name(name);
    DEVICES.iter().find(|device| {
        device.pid == pid
            || (!normalized.is_empty()
                && std::iter::once(device.name)
                    .chain(std::iter::once(device.key))
                    .chain(device.aliases.iter().copied())
                    .any(|candidate| normalize_name(candidate) == normalized))
    })
}

pub fn clamp_dpi(value: i64, device: Option<&DeviceSpec>) -> u16 {
    let (min, max) = device.map_or((200, 8000), |spec| (spec.dpi_min, spec.dpi_max));
    value.clamp(i64::from(min), i64::from(max)) as u16
}

pub fn candidate_allowed(vendor: u16, pid: u16, name: &str, usage_page: u16) -> bool {
    vendor == crate::hidpp::VENDOR && (usage_page >= 0xff00 || resolve(pid, name).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_all_aliases_without_substring_matching() {
        for spec in &DEVICES {
            assert_eq!(resolve(spec.pid, "").unwrap().key, spec.key);
            for name in std::iter::once(spec.key)
                .chain(std::iter::once(spec.name))
                .chain(spec.aliases.iter().copied())
            {
                assert_eq!(resolve(0, name).unwrap().key, spec.key);
            }
        }
        assert!(resolve(0, "MX Master 3S keyboard imitation").is_none());
    }

    #[test]
    fn accepts_known_ble_without_usage_but_never_other_vendor() {
        assert!(candidate_allowed(0x046d, 0xb034, "", 0));
        assert!(!candidate_allowed(0x1234, 0xb034, "MX Master 3S", 0xff00));
        assert!(!candidate_allowed(0x046d, 0x1234, "Unknown", 0));
        assert!(candidate_allowed(
            0x046d,
            0xc548,
            "Logi Bolt Receiver",
            0xff00
        ));
    }

    #[test]
    fn model_specific_dpi_limits_are_preserved() {
        assert_eq!(clamp_dpi(16000, resolve(0xb020, "")), 4000);
        assert_eq!(clamp_dpi(-1, None), 200);
        assert_eq!(clamp_dpi(16000, None), 8000);
    }
}
