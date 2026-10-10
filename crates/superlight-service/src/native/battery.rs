pub const LOW: u8 = 15;
pub const REARM: u8 = 25;

pub struct Alert {
    armed: bool,
}

impl Default for Alert {
    fn default() -> Self {
        Self { armed: true }
    }
}

impl Alert {
    pub fn update(&mut self, level: u8) -> bool {
        if level > REARM {
            self.armed = true;
        } else if level <= LOW && self.armed {
            self.armed = false;
            return true;
        }
        false
    }
}

pub fn title(level: Option<u8>) -> String {
    level.map(|level| format!("{level}%")).unwrap_or_default()
}

pub fn description(level: Option<u8>) -> String {
    match level {
        Some(level) => format!("SuperLight mouse controls, battery {level}%"),
        None => "SuperLight mouse controls".into(),
    }
}

pub fn message(name: &str, level: u8) -> String {
    let name = if name.is_empty() { "Your mouse" } else { name };
    format!("{name} is at {level}%. Charge it soon.")
}
