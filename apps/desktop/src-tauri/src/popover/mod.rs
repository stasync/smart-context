//! The answer popover (docs/PLAN.md 3.2): placing it beside where the user
//! pointed, showing and hiding it, and streaming the conversation into it.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Emitter, Manager};

use crate::orchestrator::{AnswerEvent, AnswerSink};
use crate::platform::{Native, Overlay, Point, Rect};

pub const POPOVER_WINDOW: &str = "popover";
/// The event the popover listens to for the conversation.
const ANSWER_EVENT: &str = "answer:event";
/// Starting values.
const WIDTH: f64 = 440.0;
const HEIGHT: f64 = 380.0;
/// Space between the lens and the popover.
const GAP: f64 = 12.0;
/// The popover stays at least this far inside the display's edges.
const MARGIN: f64 = 8.0;

pub struct Popover {
    app: AppHandle,
    native: Arc<Native>,
    /// Where it is while open.
    frame: Mutex<Option<Rect>>,
}

impl Popover {
    pub fn new(app: AppHandle, native: Arc<Native>) -> Self {
        Self {
            app,
            native,
            frame: Mutex::new(None),
        }
    }

    /// Opens beside `anchor` (the lens), inside `display`. Takes the keyboard
    /// only in ask mode, so otherwise typing keeps going to the user's app.
    pub fn open(&self, anchor: Rect, display: Rect, focus: bool) {
        let frame = place(anchor, display, WIDTH, HEIGHT);
        *self.frame() = Some(frame);
        if let Some(window) = self.app.get_webview_window(POPOVER_WINDOW)
            && let Err(e) = self.native.show_overlay(&window, frame, focus)
        {
            log::warn!("couldn't show the popover: {e}");
        }
    }

    pub fn close(&self) {
        if self.frame().take().is_none() {
            return;
        }
        if let Some(window) = self.app.get_webview_window(POPOVER_WINDOW)
            && let Err(e) = self.native.hide_overlay(&window)
        {
            log::warn!("couldn't hide the popover: {e}");
        }
    }

    pub fn is_open(&self) -> bool {
        self.frame().is_some()
    }

    /// Whether `p` is inside the open popover; None when it's closed.
    pub fn contains(&self, p: Point) -> Option<bool> {
        self.frame().map(|f| f.contains(p))
    }

    fn frame(&self) -> MutexGuard<'_, Option<Rect>> {
        self.frame.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl AnswerSink for Popover {
    fn send(&self, event: AnswerEvent) {
        if let Err(e) = self.app.emit_to(POPOVER_WINDOW, ANSWER_EVENT, event) {
            log::warn!("couldn't update the popover: {e}");
        }
    }
}

/// To the right of the anchor if it fits, else to the left, else on top of
/// it; top-aligned with it and always inside the display.
fn place(anchor: Rect, display: Rect, width: f64, height: f64) -> Rect {
    let left_edge = display.x + MARGIN;
    let right_edge = display.x + display.width - MARGIN;
    let right_side = anchor.x + anchor.width + GAP;
    let left_side = anchor.x - GAP - width;
    let x = if right_side + width <= right_edge {
        right_side
    } else if left_side >= left_edge {
        left_side
    } else {
        clamp(anchor.x, left_edge, right_edge - width)
    };
    let y = clamp(
        anchor.y,
        display.y + MARGIN,
        display.y + display.height - MARGIN - height,
    );
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Like `f64::clamp`, but when the range is empty (a display smaller than
/// the popover) it keeps the lower bound instead of panicking.
fn clamp(value: f64, low: f64, high: f64) -> f64 {
    value.min(high).max(low)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1440.0,
        height: 900.0,
    };

    fn lens(x: f64, y: f64) -> Rect {
        Rect {
            x,
            y,
            width: 240.0,
            height: 120.0,
        }
    }

    #[test]
    fn it_opens_right_of_the_lens_when_there_is_room() {
        let frame = place(lens(100.0, 200.0), DISPLAY, WIDTH, HEIGHT);
        assert_eq!((frame.x, frame.y), (100.0 + 240.0 + GAP, 200.0));
    }

    #[test]
    fn near_the_right_edge_it_opens_on_the_left() {
        let frame = place(lens(1100.0, 200.0), DISPLAY, WIDTH, HEIGHT);
        assert_eq!(frame.x, 1100.0 - GAP - WIDTH);
    }

    #[test]
    fn it_never_leaves_the_display() {
        for (x, y) in [(-50.0, -50.0), (1300.0, 850.0), (600.0, 880.0), (0.0, 0.0)] {
            let frame = place(lens(x, y), DISPLAY, WIDTH, HEIGHT);
            assert!(
                frame.x >= MARGIN && frame.x + frame.width <= DISPLAY.width - MARGIN + 0.01,
                "{x},{y}"
            );
            assert!(
                frame.y >= MARGIN && frame.y + frame.height <= DISPLAY.height - MARGIN + 0.01,
                "{x},{y}"
            );
        }
    }

    #[test]
    fn a_huge_lens_gets_the_popover_on_top_of_it() {
        let wide = Rect {
            x: 20.0,
            y: 100.0,
            width: 1400.0,
            height: 700.0,
        };
        let frame = place(wide, DISPLAY, WIDTH, HEIGHT);
        assert_eq!(frame.x, 20.0);
    }

    #[test]
    fn a_tiny_display_does_not_panic() {
        let tiny = Rect {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 200.0,
        };
        let frame = place(lens(10.0, 10.0), tiny, WIDTH, HEIGHT);
        assert_eq!((frame.x, frame.y), (MARGIN, MARGIN));
    }
}
