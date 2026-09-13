#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    MacOs,
    Windows,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Windows
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Chord {
    pub codes: [u16; 8],
    pub len: u8,
    pub phased: bool,
}

impl Chord {
    pub fn parse(combo: &str, platform: Platform) -> Result<Self, String> {
        let mut chord = Self {
            codes: [0; 8],
            len: 0,
            phased: false,
        };
        for name in combo
            .split('+')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if usize::from(chord.len) == chord.codes.len() {
                return Err("Shortcuts accept at most eight keys".into());
            }
            chord.codes[usize::from(chord.len)] =
                key_code(name, platform).ok_or_else(|| format!("Unknown shortcut key: {name}"))?;
            chord.len += 1;
        }
        if chord.len == 0 {
            return Err("Shortcut is empty".into());
        }
        Ok(chord)
    }

    pub fn keys(&self) -> &[u16] {
        &self.codes[..usize::from(self.len)]
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Action {
    #[default]
    None,
    Chord(Chord),
    Mouse(u8),
    Media(u8),
    System(u8),
    ToggleSmartShift,
    SwitchScrollMode,
    CycleDpi,
}

pub const ACTIONS: &[(&str, &str)] = &[
    ("none", "Pass through"),
    ("alt_tab", "Switch applications"),
    ("alt_shift_tab", "Previous application"),
    ("browser_back", "Browser back"),
    ("browser_forward", "Browser forward"),
    ("copy", "Copy"),
    ("paste", "Paste"),
    ("cut", "Cut"),
    ("undo", "Undo"),
    ("select_all", "Select all"),
    ("save", "Save"),
    ("next_tab", "Next tab"),
    ("prev_tab", "Previous tab"),
    ("close_tab", "Close tab"),
    ("new_tab", "New tab"),
    ("find", "Find"),
    ("win_d", "Desktop overview"),
    ("task_view", "Task view"),
    ("mission_control", "Mission Control"),
    ("app_expose", "Application overview"),
    ("space_left", "Previous desktop"),
    ("space_right", "Next desktop"),
    ("show_desktop", "Show desktop"),
    ("launchpad", "Applications"),
    ("volume_up", "Volume up"),
    ("volume_down", "Volume down"),
    ("volume_mute", "Mute"),
    ("play_pause", "Play / pause"),
    ("next_track", "Next track"),
    ("prev_track", "Previous track"),
    ("page_up", "Page up"),
    ("page_down", "Page down"),
    ("home", "Home"),
    ("end", "End"),
    ("switch_scroll_mode", "Ratchet / free spin"),
    ("toggle_smart_shift", "Toggle SmartShift"),
    ("cycle_dpi", "Cycle DPI presets"),
    ("mouse_left_click", "Left mouse button"),
    ("mouse_right_click", "Right mouse button"),
    ("mouse_middle_click", "Middle mouse button"),
    ("mouse_back_click", "Back mouse button"),
    ("mouse_forward_click", "Forward mouse button"),
];

impl Action {
    pub fn parse(id: &str, platform: Platform) -> Result<Self, String> {
        if id == "none" || id.is_empty() {
            return Ok(Self::None);
        }
        if let Some(combo) = id.strip_prefix("custom:") {
            return Chord::parse(combo, platform).map(Self::Chord);
        }
        let mac = platform == Platform::MacOs;
        let chord = match id {
            "mouse_left_click" => return Ok(Self::Mouse(0)),
            "mouse_right_click" => return Ok(Self::Mouse(1)),
            "mouse_middle_click" => return Ok(Self::Mouse(2)),
            "mouse_back_click" => return Ok(Self::Mouse(3)),
            "mouse_forward_click" => return Ok(Self::Mouse(4)),
            "volume_up" => return Ok(Self::Media(0)),
            "volume_down" => return Ok(Self::Media(1)),
            "volume_mute" => return Ok(Self::Media(2)),
            "play_pause" => return Ok(Self::Media(3)),
            "next_track" => return Ok(Self::Media(4)),
            "prev_track" => return Ok(Self::Media(5)),
            "toggle_smart_shift" => return Ok(Self::ToggleSmartShift),
            "switch_scroll_mode" => return Ok(Self::SwitchScrollMode),
            "cycle_dpi" => return Ok(Self::CycleDpi),
            "mission_control" => return Ok(Self::System(0)),
            "app_expose" => return Ok(Self::System(1)),
            "show_desktop" => return Ok(Self::System(2)),
            "launchpad" => return Ok(Self::System(3)),
            "space_left" => return Ok(Self::System(4)),
            "space_right" => return Ok(Self::System(5)),
            "alt_tab" => {
                if mac {
                    "super+tab"
                } else {
                    "alt+tab"
                }
            }
            "alt_shift_tab" => {
                if mac {
                    "super+shift+tab"
                } else {
                    "alt+shift+tab"
                }
            }
            "browser_back" => {
                if mac {
                    "super+["
                } else {
                    "alt+left"
                }
            }
            "browser_forward" => {
                if mac {
                    "super+]"
                } else {
                    "alt+right"
                }
            }
            "copy" => {
                if mac {
                    "super+c"
                } else {
                    "ctrl+c"
                }
            }
            "paste" => {
                if mac {
                    "super+v"
                } else {
                    "ctrl+v"
                }
            }
            "cut" => {
                if mac {
                    "super+x"
                } else {
                    "ctrl+x"
                }
            }
            "undo" => {
                if mac {
                    "super+z"
                } else {
                    "ctrl+z"
                }
            }
            "select_all" => {
                if mac {
                    "super+a"
                } else {
                    "ctrl+a"
                }
            }
            "save" => {
                if mac {
                    "super+s"
                } else {
                    "ctrl+s"
                }
            }
            "next_tab" => {
                if mac {
                    "super+shift+]"
                } else {
                    "ctrl+tab"
                }
            }
            "prev_tab" => {
                if mac {
                    "super+shift+["
                } else {
                    "ctrl+shift+tab"
                }
            }
            "close_tab" => {
                if mac {
                    "super+w"
                } else {
                    "ctrl+w"
                }
            }
            "new_tab" => {
                if mac {
                    "super+t"
                } else {
                    "ctrl+t"
                }
            }
            "find" => {
                if mac {
                    "super+f"
                } else {
                    "ctrl+f"
                }
            }
            "win_d" => {
                if mac {
                    "ctrl+up"
                } else {
                    "super+d"
                }
            }
            "task_view" => {
                if mac {
                    "ctrl+up"
                } else {
                    "super+tab"
                }
            }
            "page_up" => "pageup",
            "page_down" => "pagedown",
            "home" => "home",
            "end" => "end",
            _ => return Err(format!("Unknown action: {id}")),
        };
        let mut chord = Chord::parse(chord, platform)?;
        if platform == Platform::Windows && matches!(id, "browser_back" | "browser_forward") {
            chord.phased = true;
            chord.codes[0] = 0xa4;
        }
        Ok(Self::Chord(chord))
    }

    pub fn is_volume(self) -> bool {
        matches!(self, Self::Media(0..=2))
    }

    pub fn is_device(self) -> bool {
        matches!(
            self,
            Self::ToggleSmartShift | Self::SwitchScrollMode | Self::CycleDpi
        )
    }
}

pub fn canonical_key(name: &str) -> &str {
    match name {
        "control" => "ctrl",
        "option" | "opt" => "alt",
        "cmd" | "command" | "meta" | "win" | "windows" => "super",
        "return" => "enter",
        "escape" => "esc",
        _ => name,
    }
}

pub fn key_code(name: &str, platform: Platform) -> Option<u16> {
    let lowered = name.to_ascii_lowercase();
    let name = canonical_key(lowered.trim());
    let column = match platform {
        Platform::MacOs => 0,
        Platform::Windows => 1,
    };
    let codes = match name {
        "ctrl" => [0x3b, 0x11],
        "shift" => [0x38, 0x10],
        "alt" => [0x3a, 0x12],
        "super" => [0x37, 0x5b],
        "tab" => [0x30, 9],
        "space" => [0x31, 32],
        "enter" => [0x24, 13],
        "esc" => [0x35, 27],
        "backspace" => [0x33, 8],
        "delete" => [0x75, 46],
        "left" => [0x7b, 37],
        "right" => [0x7c, 39],
        "up" => [0x7e, 38],
        "down" => [0x7d, 40],
        "pageup" => [0x74, 33],
        "pagedown" => [0x79, 34],
        "home" => [0x73, 36],
        "end" => [0x77, 35],
        "[" => [0x21, 0xdb],
        "]" => [0x1e, 0xdd],
        "volumeup" if platform != Platform::MacOs => [0, 0xaf],
        "volumedown" if platform != Platform::MacOs => [0, 0xae],
        "mute" if platform != Platform::MacOs => [0, 0xad],
        "playpause" if platform != Platform::MacOs => [0, 0xb3],
        "nexttrack" if platform != Platform::MacOs => [0, 0xb0],
        "prevtrack" if platform != Platform::MacOs => [0, 0xb1],
        _ => {
            if name.len() == 1 {
                let byte = name.as_bytes()[0];
                if byte.is_ascii_lowercase() {
                    let index = usize::from(byte - b'a');
                    let mac = [
                        0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17,
                        32, 9, 13, 7, 16, 6,
                    ];
                    return Some([mac[index], u16::from(byte.to_ascii_uppercase())][column]);
                }
                if byte.is_ascii_digit() {
                    let index = usize::from(byte - b'0');
                    return Some(
                        [
                            [29, 18, 19, 20, 21, 23, 22, 26, 28, 25][index],
                            u16::from(byte),
                        ][column],
                    );
                }
            }
            if let Some(function) = name
                .strip_prefix('f')
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| (1..=12).contains(value))
            {
                let index = function - 1;
                return Some(
                    [
                        [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111][index],
                        (0x70 + index) as u16,
                    ][column],
                );
            }
            return None;
        }
    };
    Some(codes[column])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_compiles_on_all_platforms() {
        for platform in [Platform::MacOs, Platform::Windows] {
            for (id, _) in ACTIONS {
                assert!(Action::parse(id, platform).is_ok(), "{id}");
            }
        }
    }

    #[test]
    fn aliases_match_canonical_keys_on_all_platforms() {
        for platform in [Platform::MacOs, Platform::Windows] {
            for (alias, canonical) in [
                ("command", "super"),
                ("win", "super"),
                ("opt", "alt"),
                ("control", "ctrl"),
                ("return", "enter"),
                ("escape", "esc"),
            ] {
                assert_eq!(key_code(alias, platform), key_code(canonical, platform));
            }
        }
    }

    #[test]
    fn mac_digits_are_hardware_codes_not_ascii() {
        assert_eq!(
            Chord::parse("cmd+shift+3", Platform::MacOs).unwrap().keys(),
            [55, 56, 20]
        );
        assert_eq!(
            Chord::parse("ctrl+0", Platform::Windows).unwrap().keys(),
            [17, 48]
        );
    }

    #[test]
    fn shortcuts_are_bounded_and_invalid_names_are_reported() {
        assert!(Chord::parse("", Platform::MacOs).is_err());
        assert!(Chord::parse("ctrl+notakey", Platform::Windows).is_err());
        assert!(Chord::parse("a+b+c+d+e+f+g+h+i", Platform::Windows).is_err());
        assert!(Action::parse("unknown", Platform::MacOs).is_err());
    }

    #[test]
    fn chromium_navigation_keeps_phased_left_alt() {
        let Action::Chord(chord) = Action::parse("browser_back", Platform::Windows).unwrap() else {
            panic!()
        };
        assert!(chord.phased);
        assert_eq!(chord.keys(), [164, 37]);
        let Action::Chord(chord) = Action::parse("browser_back", Platform::MacOs).unwrap() else {
            panic!()
        };
        assert!(!chord.phased);
        assert_eq!(chord.keys(), [55, 33]);
    }
}
