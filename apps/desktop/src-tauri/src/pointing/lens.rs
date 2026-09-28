//! Lens geometry: where the lens is and how big.

use crate::platform::{Point, Rect};

/// Starting values; tune them by use.
const DEFAULT_WIDTH: f64 = 240.0;
const DEFAULT_HEIGHT: f64 = 120.0;
const MIN_WIDTH: f64 = 40.0;
const MAX_WIDTH: f64 = 1600.0;
/// Scrolling by `delta` points scales the lens by e^(delta × RESIZE_RATE).
const RESIZE_RATE: f64 = 0.01;

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
}
