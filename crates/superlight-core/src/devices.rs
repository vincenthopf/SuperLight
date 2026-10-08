use crate::hidpp::{GESTURE_CIDS, MULTIPLATFORM_GESTURE_CID, VIRTUAL_GESTURE_CID};

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
    pub product_ids: &'static [u16],
    pub aliases: &'static [&'static str],
    pub family: Family,
    pub dpi_min: u16,
    pub dpi_max: u16,
    pub image: &'static str,
    pub gesture_cids: &'static [u16],
}

pub static DEVICES: [DeviceSpec; 13] = [
    DeviceSpec {
        key: "mx_master_4",
        name: "MX Master 4",
        product_ids: &[0xb042, 0xb048],
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
        product_ids: &[0xb034, 0xb043],
        aliases: &[
            "Logitech MX Master 3S",
            "MX Master 3S for Mac",
            "MX Master 3S for Business",
        ],
        family: Family::Master,
        dpi_min: 200,
        dpi_max: 8000,
        image: "mouse.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_master_3",
        name: "MX Master 3",
        product_ids: &[0xb023, 0xb028],
        aliases: &[
            "Wireless Mouse MX Master 3",
            "MX Master 3 for Mac",
            "MX Master 3 Mac",
            "MX Master 3 for Business",
        ],
        family: Family::Master,
        dpi_min: 200,
        dpi_max: 4000,
        image: "mouse.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_master_2s",
        name: "MX Master 2S",
        product_ids: &[0xb019],
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
        product_ids: &[0xb012],
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
        product_ids: &[0xb020],
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
        product_ids: &[0xb037],
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
        product_ids: &[0xb025, 0xb02d],
        aliases: &["MX Anywhere 3 for Mac", "MX Anywhere 3 for Business"],
        family: Family::Anywhere,
        dpi_min: 200,
        dpi_max: 4000,
        image: "mouse_mx_anywhere_3s.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_anywhere_2s",
        name: "MX Anywhere 2S",
        product_ids: &[0xb01a],
        aliases: &["Wireless Mobile Mouse MX Anywhere 2S"],
        family: Family::Anywhere,
        dpi_min: 200,
        dpi_max: 4000,
        image: "mouse_mx_anywhere_3s.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "mx_anywhere_2",
        name: "MX Anywhere 2",
        product_ids: &[0xb013, 0xb01f],
        aliases: &[
            "Wireless Mouse MX Anywhere 2",
            "Wireless Mobile Mouse MX Anywhere 2",
            "MX Anywhere 2 for Mac",
        ],
        family: Family::Anywhere,
        dpi_min: 400,
        dpi_max: 1600,
        image: "mouse_mx_anywhere_3s.png",
        gesture_cids: &GESTURE_CIDS,
    },
    DeviceSpec {
        key: "m650",
        name: "M650 Signature",
        product_ids: &[0xb02a],
        aliases: &[
            "Signature M650",
            "Logi M650",
            "Logitech Signature M650",
            "M650",
            "M650 L",
            "Signature M650 L",
            "M650 Signature for Business",
        ],
        family: Family::Generic,
        dpi_min: 200,
        dpi_max: 4000,
        image: "",
        gesture_cids: &[],
    },
    DeviceSpec {
        key: "m585_m590",
        name: "M585/M590 Multi-Device Mouse",
        product_ids: &[],
        aliases: &[
            "M585/M590",
            "M590 Multi-Device Mouse",
            "M585 Multi-Device Mouse",
            "Logitech M590",
            "Logitech M585",
            "M590",
            "M585",
        ],
        family: Family::Generic,
        dpi_min: 200,
        dpi_max: 8000,
        image: "",
        gesture_cids: &[],
    },
    DeviceSpec {
        key: "m720_triathlon",
        name: "M720 Triathlon Multi-Device Mouse",
        product_ids: &[0xb015],
        aliases: &[
            "M720 Triathlon Mouse",
            "M720 Triathlon",
            "Logitech M720 Triathlon",
            "M720",
        ],
        family: Family::Generic,
        dpi_min: 1000,
        dpi_max: 1000,
        image: "",
        gesture_cids: &[MULTIPLATFORM_GESTURE_CID, VIRTUAL_GESTURE_CID],
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
    if let Some(device) = DEVICES
        .iter()
        .find(|device| device.product_ids.contains(&pid))
    {
        return Some(device);
    }
    let normalized = normalize_name(name);
    DEVICES.iter().find(|device| {
        !normalized.is_empty()
            && std::iter::once(device.name)
                .chain(std::iter::once(device.key))
                .chain(device.aliases.iter().copied())
                .any(|candidate| normalize_name(candidate) == normalized)
    })
}

pub fn candidate_allowed(vendor: u16, pid: u16, name: &str, usage_page: u16) -> bool {
    vendor == crate::hidpp::VENDOR
        && (usage_page >= crate::hidpp::VENDOR_USAGE_PAGE || resolve(pid, name).is_some())
}
