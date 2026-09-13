use std::collections::HashSet;
use superlight_core::devices::{DEVICES, candidate_allowed, normalize_name, resolve};

#[test]
fn matrix_tracks_every_catalog_entry_and_all_product_ids() {
    let matrix: serde_json::Value =
        serde_json::from_str(include_str!("../../../compatibility/mice.json")).unwrap();
    let rows = matrix["models"].as_array().unwrap();
    assert_eq!(rows.len(), DEVICES.len());
    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    for device in &DEVICES {
        assert!(keys.insert(device.key));
        let row = rows.iter().find(|row| row["model"] == device.key).unwrap();
        let expected: Vec<_> = row["product_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| u16::from_str_radix(v.as_str().unwrap().trim_start_matches("0x"), 16).unwrap())
            .collect();
        assert_eq!(device.product_ids, expected);
        for &pid in device.product_ids {
            assert!(ids.insert(pid));
            assert!(![0, 0xc52b, 0xc532, 0xc548].contains(&pid));
            assert_eq!(resolve(pid, "").unwrap().key, device.key);
        }
        assert!(device.dpi_min <= device.dpi_max);
        for alias in std::iter::once(device.name).chain(device.aliases.iter().copied()) {
            assert_eq!(resolve(0, &normalize_name(alias)).unwrap().key, device.key);
        }
        for evidence in row["physical_verification"].as_array().unwrap() {
            assert!(!evidence["checked"].as_array().unwrap().is_empty());
            assert!(!evidence["not_checked"].as_array().unwrap().is_empty());
            assert_eq!(evidence["build"].as_str().unwrap().len(), 64);
        }
    }
}

#[test]
fn receiver_ids_and_conflicting_names_cannot_select_the_wrong_model() {
    assert_eq!(resolve(0xb034, "MX Master 4").unwrap().key, "mx_master_3s");
    for pid in [0xc52b, 0xc532, 0xc548] {
        assert!(resolve(pid, "USB Receiver").is_none());
        assert_eq!(resolve(pid, "M590").unwrap().key, "m585_m590");
    }
    assert!(resolve(0, "").is_none());
    assert!(!candidate_allowed(0x1234, 0xb034, "MX Master 3S", 0xff00));
    assert!(!candidate_allowed(0x046d, 0xffff, "Unknown keyboard", 1));
}
