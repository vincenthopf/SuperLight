use superlight_core::hidpp::{self, Message, ResponseMatch};

#[test]
fn every_receiver_slot_rejects_other_slots() {
    for expected in hidpp::DEVICE_INDICES {
        for actual in 0..=u8::MAX {
            let message = Message {
                device: actual,
                feature: 7,
                function: 3,
                software: hidpp::SOFTWARE,
                params: &[0, 0xc3, 0x03],
            };
            let result = hidpp::match_response(message, expected, 7, 3);
            assert_eq!(result == ResponseMatch::Reply, actual == expected);
        }
    }
}

#[test]
fn asynchronous_notifications_never_acknowledge_commands() {
    for slot in hidpp::DEVICE_INDICES {
        for function in 0..16 {
            let message = Message {
                device: slot,
                feature: 7,
                function,
                software: 0,
                params: &[0, 0xc3],
            };
            for expected_function in 0..16 {
                assert_eq!(hidpp::match_response(message, slot, 7, expected_function), ResponseMatch::Unrelated);
            }
        }
    }
}
