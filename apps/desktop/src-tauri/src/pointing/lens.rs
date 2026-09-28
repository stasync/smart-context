//! Lens geometry: where the lens is and how big.

use crate::platform::{Point, Rect};

/// Starting values; tune them by use.
const DEFAULT_WIDTH: f64 = 240.0;
const DEFAULT_HEIGHT: f64 = 120.0;
const MIN_WIDTH: f64 = 40.0;
const MAX_WIDTH: f64 = 1600.0;
/// Scrolling by `delta` points scales the lens by e^(delta × RESIZE_RATE).
const RESIZE_RATE: f64 = 0.01;
/// Shift+scroll this far (in points) to step one level.
const STEP_POINTS: f64 = 20.0;

/// The lens rectangle, centered on the cursor. Its size carries over from one
/// gesture to the next.
#[derive(Clone, Debug, PartialEq)]
pub struct Lens {
    center: Point,
    width: f64,
    height: f64,
}

impl Default for Lens {
    fn default() -> Self {
        Self {
            center: Point::default(),
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
        }
    }
}

impl Lens {
    pub fn move_to(&mut self, center: Point) {
        self.center = center;
    }

    /// Scales the lens around its center, keeping its aspect ratio.
    pub fn resize(&mut self, scroll_delta: f64) {
        let factor = (scroll_delta * RESIZE_RATE).exp();
        let width = (self.width * factor).clamp(MIN_WIDTH, MAX_WIDTH);
        self.height *= width / self.width;
        self.width = width;
    }

    pub fn rect(&self) -> Rect {
        Rect::centered_at(self.center, self.width, self.height)
    }
}

/// Shift+scroll stepping through the element under the cursor (level 0)
/// and its ancestors, like a devtools element picker.
#[derive(Debug, Default)]
pub struct Stepper {
    level: Option<usize>,
    /// Scroll not yet used up by a step.
    pending: f64,
}

impl Stepper {
    /// The level the lens is snapped to, if any.
    pub fn level(&self) -> Option<usize> {
        self.level
    }

    /// Scrolling up steps toward ancestors, down toward the element itself.
    /// The first scroll snaps the lens even if it doesn't step.
    pub fn scroll(&mut self, delta: f64, max_level: usize) {
        self.pending += delta;
        let steps = (self.pending / STEP_POINTS).trunc();
        self.pending -= steps * STEP_POINTS;
        let level = self.level.unwrap_or(0) as f64 + steps;
        self.level = Some(level.clamp(0.0, max_level as f64) as usize);
    }

    /// Keeps the level valid when the element chain changes.
    pub fn clamp(&mut self, max_level: usize) {
        if let Some(level) = &mut self.level {
            *level = (*level).min(max_level);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lens_is_centered_on_the_cursor() {
        let mut lens = Lens::default();
        lens.move_to(Point { x: 500.0, y: 300.0 });
        let r = lens.rect();
        assert_eq!((r.x + r.width / 2.0, r.y + r.height / 2.0), (500.0, 300.0));
    }

    #[test]
    fn scrolling_up_grows_and_down_shrinks_keeping_the_aspect_ratio() {
        let mut lens = Lens::default();
        lens.resize(50.0);
        let grown = lens.rect();
        assert!(grown.width > DEFAULT_WIDTH);
        assert!((grown.width / grown.height - DEFAULT_WIDTH / DEFAULT_HEIGHT).abs() < 1e-9);

        lens.resize(-100.0);
        assert!(lens.rect().width < DEFAULT_WIDTH);
    }

    #[test]
    fn the_size_is_clamped() {
        let mut lens = Lens::default();
        lens.resize(10_000.0);
        assert_eq!(lens.rect().width, MAX_WIDTH);
        lens.resize(-10_000.0);
        assert_eq!(lens.rect().width, MIN_WIDTH);
    }

    #[test]
    fn stepping_accumulates_scroll_and_stays_in_range() {
        let mut stepper = Stepper::default();
        assert_eq!(stepper.level(), None);

        stepper.scroll(5.0, 3);
        assert_eq!(
            stepper.level(),
            Some(0),
            "a small scroll snaps without stepping"
        );
        stepper.scroll(15.0, 3);
        assert_eq!(stepper.level(), Some(1));
        stepper.scroll(200.0, 3);
        assert_eq!(stepper.level(), Some(3), "capped at the outermost ancestor");
        stepper.scroll(-200.0, 3);
        assert_eq!(stepper.level(), Some(0));

        stepper.scroll(60.0, 5);
        stepper.clamp(1);
        assert_eq!(stepper.level(), Some(1));
    }
}
