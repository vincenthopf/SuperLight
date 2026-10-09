const VOLUME_WHEEL_COOLDOWN_MS: u64 = 60;
const WHEEL_ACTION_COOLDOWN_MS: u64 = 350;
const MAX_WHEEL_STEP: f64 = 1.0;

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
        let cooldown_ms = if volume {
            VOLUME_WHEEL_COOLDOWN_MS
        } else {
            WHEEL_ACTION_COOLDOWN_MS
        };
        if self
            .last_fire_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < cooldown_ms)
        {
            self.accumulated = 0.0;
            return false;
        }
        self.accumulated += delta.abs().min(MAX_WHEEL_STEP);
        if self.accumulated < threshold.max(0.1) {
            return false;
        }
        self.accumulated = 0.0;
        self.last_fire_ms = Some(now_ms);
        true
    }
}
