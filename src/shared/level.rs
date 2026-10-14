//! Brightness bookkeeping: wheel motion in, monitor setting out.

/// One wheel notch, in the units Windows and most toolkits use.
pub const NOTCH: f64 = 120.0;

/// A monitor's brightness, kept fractional so high-resolution wheels and
/// trackpads (which send fractions of a notch) add up instead of rounding away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    value: f64,
    max: f64,
}

impl Level {
    /// `current` and `max` as the monitor reports them (DDC/CI brightness runs 0..=max).
    pub fn new(current: u32, max: u32) -> Self {
        let max = max.max(1) as f64;
        Self { value: (current as f64).min(max), max }
    }

    /// Moves by `percent` of the full range, staying within it.
    pub fn nudge(&mut self, percent: f64) {
        self.value = (self.value + percent / 100.0 * self.max).clamp(0.0, self.max);
    }

    /// The whole number to send to the monitor.
    pub fn device_value(&self) -> u32 {
        self.value.round() as u32
    }

    /// Position within the range, 0..=1.
    pub fn fraction(&self) -> f64 {
        self.device_value() as f64 / self.max
    }
}

/// How far a wheel event moves the brightness, in percent.
pub fn wheel_percent(wheel_delta: i32, step_percent: u32) -> f64 {
    wheel_delta as f64 / NOTCH * step_percent as f64
}

/// Whether a pointer at `y` sits on the top edge of a monitor whose top is
/// `monitor_top`. Only a real screen edge counts: where another monitor
/// continues above, the pointer just passes through.
pub fn on_top_edge(y: i32, monitor_top: i32, edge_px: i32, monitor_above: bool) -> bool {
    !monitor_above && y >= monitor_top && y < monitor_top + edge_px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notch_moves_by_the_step() {
        let mut l = Level::new(36, 100);
        l.nudge(wheel_percent(120, 5));
        assert_eq!(l.device_value(), 41);
        l.nudge(wheel_percent(-240, 5));
        assert_eq!(l.device_value(), 31);
    }

    #[test]
    fn fractional_scrolling_adds_up() {
        let mut l = Level::new(50, 100);
        for _ in 0..4 {
            l.nudge(wheel_percent(30, 4)); // a quarter notch each
        }
        assert_eq!(l.device_value(), 54);
    }

    #[test]
    fn stays_within_the_range() {
        let mut l = Level::new(98, 100);
        l.nudge(25.0);
        assert_eq!(l.device_value(), 100);
        l.nudge(-500.0);
        assert_eq!(l.device_value(), 0);
    }

    #[test]
    fn steps_scale_with_the_monitor_range() {
        let mut l = Level::new(25, 50);
        l.nudge(10.0);
        assert_eq!(l.device_value(), 30);
        assert_eq!(l.fraction(), 0.6);
    }

    #[test]
    fn only_the_outer_top_edge_counts() {
        assert!(on_top_edge(0, 0, 3, false));
        assert!(on_top_edge(2, 0, 3, false));
        assert!(!on_top_edge(3, 0, 3, false));
        assert!(!on_top_edge(0, 0, 3, true));
        assert!(on_top_edge(-1080, -1080, 3, false));
    }
}
