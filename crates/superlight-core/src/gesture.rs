#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    pub const fn mapping_index(self) -> usize {
        match self {
            Self::Left => 8,
            Self::Right => 9,
            Self::Up => 10,
            Self::Down => 11,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Hid,
    Native,
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub enabled: bool,
    pub threshold: f64,
    pub deadzone: f64,
    pub timeout_ms: u64,
    pub cooldown_ms: u64,
    pub prefer_hid: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: 50.0,
            deadzone: 40.0,
            timeout_ms: 3000,
            cooldown_ms: 500,
            prefer_hid: cfg!(target_os = "macos"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Gesture {
    pub options: Options,
    held: bool,
    triggered: bool,
    tracking: bool,
    source: Option<Source>,
    last_move_ms: u64,
    cooldown_until_ms: u64,
    x: f64,
    y: f64,
}

impl Gesture {
    pub fn new(options: Options) -> Self {
        Self {
            options,
            ..Self::default()
        }
    }

    pub fn held(&self) -> bool {
        self.held
    }
    pub fn source(&self) -> Option<Source> {
        self.source
    }

    fn reset_segment(&mut self, now_ms: u64) {
        self.tracking = true;
        self.source = None;
        self.last_move_ms = now_ms;
        self.x = 0.0;
        self.y = 0.0;
    }

    pub fn press(&mut self, now_ms: u64) {
        if self.held {
            return;
        }
        self.held = true;
        self.triggered = false;
        self.tracking = false;
        if self.options.enabled && now_ms >= self.cooldown_until_ms {
            self.reset_segment(now_ms);
        }
    }

    pub fn release(&mut self) -> bool {
        let click = self.held && !self.triggered;
        self.cancel();
        click
    }

    pub fn cancel(&mut self) {
        self.held = false;
        self.tracking = false;
        self.triggered = false;
        self.source = None;
        self.x = 0.0;
        self.y = 0.0;
    }

    pub fn movement(&mut self, x: f64, y: f64, source: Source, now_ms: u64) -> Option<Direction> {
        if !self.held
            || !self.options.enabled
            || !x.is_finite()
            || !y.is_finite()
            || now_ms < self.cooldown_until_ms
        {
            return None;
        }
        let promote =
            self.options.prefer_hid && source == Source::Hid && self.source == Some(Source::Native);
        if !self.tracking
            || now_ms.saturating_sub(self.last_move_ms) > self.options.timeout_ms.max(250)
            || promote
        {
            self.reset_segment(now_ms);
        }
        if self.source.is_some_and(|existing| existing != source) {
            return None;
        }
        self.source = Some(source);
        self.x = (self.x + x).clamp(-1_000_000.0, 1_000_000.0);
        self.y = (self.y + y).clamp(-1_000_000.0, 1_000_000.0);
        self.last_move_ms = now_ms;
        let direction = detect(
            self.x,
            self.y,
            self.options.threshold,
            self.options.deadzone,
        )?;
        self.triggered = true;
        self.cooldown_until_ms = now_ms.saturating_add(self.options.cooldown_ms);
        self.tracking = false;
        self.source = None;
        self.x = 0.0;
        self.y = 0.0;
        Some(direction)
    }
}

pub fn detect(x: f64, y: f64, threshold: f64, deadzone: f64) -> Option<Direction> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let abs_x = x.abs();
    let abs_y = y.abs();
    let dominant = abs_x.max(abs_y);
    if dominant < threshold.max(5.0) {
        return None;
    }
    let cross_limit = deadzone.max(0.0).max(dominant * 0.35);
    if abs_x > abs_y {
        if abs_y > cross_limit {
            return None;
        }
        Some(if x > 0.0 {
            Direction::Right
        } else {
            Direction::Left
        })
    } else {
        if abs_x > cross_limit {
            return None;
        }
        Some(if y > 0.0 {
            Direction::Down
        } else {
            Direction::Up
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WheelAccumulator {
    accumulated: f64,
    last_fire_ms: Option<u64>,
}

impl WheelAccumulator {
    pub fn step(&mut self, delta: f64, threshold: f64, volume: bool, now_ms: u64) -> bool {
        if !delta.is_finite() || !threshold.is_finite() {
            return false;
        }
        let cooldown_ms = if volume { 60 } else { 350 };
        if self
            .last_fire_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < cooldown_ms)
        {
            self.accumulated = 0.0;
            return false;
        }
        self.accumulated += delta.abs().min(1.0);
        if self.accumulated < threshold.max(0.1) {
            return false;
        }
        self.accumulated = 0.0;
        self.last_fire_ms = Some(now_ms);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_diagonals_and_axis_signs() {
        assert_eq!(detect(49.0, 0.0, 50.0, 40.0), None);
        assert_eq!(detect(50.0, 40.0, 50.0, 40.0), Some(Direction::Right));
        assert_eq!(detect(-50.0, 0.0, 50.0, 40.0), Some(Direction::Left));
        assert_eq!(detect(0.0, -50.0, 50.0, 40.0), Some(Direction::Up));
        assert_eq!(detect(0.0, 50.0, 50.0, 40.0), Some(Direction::Down));
        assert_eq!(detect(60.0, 60.0, 50.0, 40.0), None);
        assert_eq!(detect(f64::NAN, 0.0, 50.0, 40.0), None);
    }

    #[test]
    fn click_is_only_emitted_on_an_untriggered_release() {
        let mut gesture = Gesture::new(Options {
            enabled: true,
            ..Options::default()
        });
        assert!(!gesture.release());
        gesture.press(0);
        assert!(gesture.release());
        gesture.press(10);
        assert_eq!(
            gesture.movement(50.0, 0.0, Source::Hid, 11),
            Some(Direction::Right)
        );
        assert!(!gesture.release());
    }

    #[test]
    fn mac_promotes_hid_after_tiny_native_starter() {
        let mut gesture = Gesture::new(Options {
            enabled: true,
            prefer_hid: true,
            ..Options::default()
        });
        gesture.press(0);
        assert_eq!(gesture.movement(2.0, 0.0, Source::Native, 1), None);
        assert_eq!(
            gesture.movement(-50.0, 0.0, Source::Hid, 2),
            Some(Direction::Left)
        );
        assert!(!gesture.release());
    }

    #[test]
    fn no_double_counting_and_timeout_starts_new_segment() {
        let mut gesture = Gesture::new(Options {
            enabled: true,
            ..Options::default()
        });
        gesture.press(0);
        assert_eq!(gesture.movement(30.0, 0.0, Source::Hid, 1), None);
        assert_eq!(gesture.movement(30.0, 0.0, Source::Native, 2), None);
        assert_eq!(gesture.movement(30.0, 0.0, Source::Hid, 4000), None);
        assert_eq!(
            gesture.movement(20.0, 0.0, Source::Hid, 4001),
            Some(Direction::Right)
        );
    }

    #[test]
    fn cooldown_allows_repeated_swipes_while_held_without_click() {
        let mut gesture = Gesture::new(Options {
            enabled: true,
            ..Options::default()
        });
        gesture.press(0);
        assert!(gesture.movement(50.0, 0.0, Source::Hid, 1).is_some());
        assert!(gesture.movement(50.0, 0.0, Source::Hid, 500).is_none());
        assert!(gesture.movement(50.0, 0.0, Source::Hid, 501).is_some());
        assert!(!gesture.release());
    }

    #[test]
    fn disconnect_cancels_without_false_click() {
        let mut gesture = Gesture::default();
        gesture.press(0);
        gesture.cancel();
        assert!(!gesture.release());
    }

    #[test]
    fn fractional_wheel_values_and_distinct_volume_cooldown() {
        let mut wheel = WheelAccumulator::default();
        assert!(!wheel.step(0.5, 1.0, false, 0));
        assert!(wheel.step(0.5, 1.0, false, 1));
        assert!(!wheel.step(120.0, 1.0, false, 100));
        assert!(wheel.step(120.0, 1.0, false, 351));
        let mut wheel = WheelAccumulator::default();
        assert!(wheel.step(120.0, 1.0, true, 0));
        assert!(!wheel.step(120.0, 1.0, true, 59));
        assert!(wheel.step(120.0, 1.0, true, 60));
    }
}
