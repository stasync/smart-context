//! Everything OS-specific: global input hooks, accessibility queries, screen
//! capture, overlay window behavior and opening System Settings panes.
//!
//! The rest of the app only sees the traits and types in this file
//! (docs/PLAN.md section 5.4). Each OS implements all of them on one type,
//! exported here as [`Native`].

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use tauri::WebviewWindow;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{Native, is_reopen};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{Native, is_reopen};

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
compile_error!("Context supports macOS and Windows only.");

// ---------------------------------------------------------------------------
// Geometry

/// A point in global screen coordinates: points (not pixels), with the origin
/// at the top-left of the primary display and y growing down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// A rectangle in the same coordinates as [`Point`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn centered_at(center: Point, width: f64, height: f64) -> Self {
        Self {
            x: center.x - width / 2.0,
            y: center.y - height / 2.0,
            width,
            height,
        }
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.x + self.width && p.y >= self.y && p.y < self.y + self.height
    }

    /// The overlap of two rectangles, if they overlap at all.
    pub fn intersection(&self, r: &Rect) -> Option<Rect> {
        let x = self.x.max(r.x);
        let y = self.y.max(r.y);
        let right = (self.x + self.width).min(r.x + r.width);
        let bottom = (self.y + self.height).min(r.y + r.height);
        (right > x && bottom > y).then_some(Rect {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }

    pub fn center(&self) -> Point {
        Point {
            x: self.x + self.width / 2.0,
            y: self.y + self.height / 2.0,
        }
    }

    /// This rectangle in coordinates relative to `origin`.
    pub fn relative_to(&self, origin: Point) -> Self {
        Self {
            x: self.x - origin.x,
            y: self.y - origin.y,
            ..*self
        }
    }

    pub fn origin(&self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }
}

// ---------------------------------------------------------------------------
// Apps and windows

/// The app that owns a window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub bundle_id: Option<String>,
    pub pid: i32,
}

/// An on-screen window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub id: u32,
    pub title: Option<String>,
    pub bounds: Rect,
    pub app: AppInfo,
}

// ---------------------------------------------------------------------------
// Accessibility

/// An accessibility element, reduced to what a context pack needs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementInfo {
    pub role: Option<String>,
    pub subrole: Option<String>,
    pub role_description: Option<String>,
    pub title: Option<String>,
    /// Truncated: a text area's value can be a whole document.
    pub value: Option<String>,
    pub description: Option<String>,
    pub help: Option<String>,
    pub bounds: Option<Rect>,
}

/// Limits for collecting the text around the pointer.
#[derive(Clone, Copy, Debug)]
pub struct TextBudget {
    /// How many points inside the lens to hit-test.
    pub max_samples: usize,
    pub max_time: Duration,
    pub max_chars: usize,
}

/// What to read besides the element under the point.
#[derive(Clone, Copy, Debug)]
pub struct InspectOptions {
    /// How many ancestors to return.
    pub max_ancestors: usize,
    /// Collect the text of the elements that intersect this rectangle.
    pub text_in: Option<(Rect, TextBudget)>,
    /// Also read the app's selected text and, in browsers, the page URL.
    pub page_details: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inspection {
    /// The element under the point, then its ancestors, nearest first.
    pub chain: Vec<ElementInfo>,
    pub nearby_text: String,
    pub selection: Option<String>,
    pub url: Option<String>,
}

// ---------------------------------------------------------------------------
// Screen capture

/// The largest image to capture: a long edge and a pixel count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageLimit {
    pub max_long_edge: u32,
    pub max_pixels: u64,
}

impl ImageLimit {
    /// The largest size within the limit with the aspect ratio of
    /// `width` × `height`. Never scales up.
    pub fn fit(&self, width: f64, height: f64) -> (u32, u32) {
        let by_edge = f64::from(self.max_long_edge) / width.max(height);
        let by_area = (self.max_pixels as f64 / (width * height)).sqrt();
        let scale = by_edge.min(by_area).min(1.0);
        (
            ((width * scale).floor() as u32).max(1),
            ((height * scale).floor() as u32).max(1),
        )
    }
}

/// Size limits for the two screenshots.
#[derive(Clone, Copy, Debug)]
pub struct ShotLimits {
    pub window: ImageLimit,
    pub lens: ImageLimit,
}

/// Screenshots for one question, already scaled to their limits.
#[derive(Clone, Debug, Default)]
pub struct Screenshots {
    /// The target window's own pixels, without anything on top of it.
    pub window: Option<RgbaImage>,
    /// What's on screen under the lens, without Context's own windows.
    pub lens: Option<RgbaImage>,
    /// Screen pixels per point where the user pointed (0 if unknown).
    pub scale: f64,
}

// ---------------------------------------------------------------------------
// Permissions

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Permission {
    Accessibility,
    ScreenRecording,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub accessibility: bool,
    pub screen_recording: bool,
}

impl PermissionStatus {
    pub fn all_granted(&self) -> bool {
        self.accessibility && self.screen_recording
    }
}

// ---------------------------------------------------------------------------
// Input

/// A key the pointing gesture cares about. Everything else is `Other`,
/// carrying the OS key code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Space,
    Escape,
    Other(u32),
}

/// A global input event, already reduced to what pointing needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    /// The pointing hotkey went down, with the cursor position.
    HotkeyDown(Point),
    HotkeyUp,
    KeyDown {
        key: Key,
        repeat: bool,
    },
    KeyUp(Key),
    /// A modifier other than the hotkey or Shift changed (Cmd, Ctrl, …).
    OtherModifier,
    /// Any mouse button went down, at this point.
    MouseDown(Point),
    MouseMoved(Point),
    /// Scroll wheel or trackpad, in points. Positive means scrolling up.
    Scroll {
        delta: f64,
        shift: bool,
    },
}

/// What the input hook does with an event after the handler has seen it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Let the event reach the app underneath.
    Pass,
    /// Drop the event, so the app underneath never sees it.
    Swallow,
}

pub trait InputHandler: Send + Sync + 'static {
    /// Called on the input thread for every event. Must return quickly: the
    /// OS disables slow hooks.
    fn handle(&self, event: InputEvent) -> Disposition;
}

// ---------------------------------------------------------------------------
// Traits

pub trait InputHooks {
    /// Starts the global input hooks on their own thread. If the permission
    /// they need is missing, keeps retrying until it's granted.
    fn start_input(&self, handler: Arc<dyn InputHandler>) -> Result<()>;

    /// How long the mouse has been still: no move, drag or scroll. None if
    /// the OS can't say.
    fn pointer_still_for(&self) -> Option<Duration>;
}

pub trait Permissions {
    fn permission_status(&self) -> PermissionStatus;

    /// Asks the OS for a permission (with its own prompt, where there is
    /// one) and opens the matching System Settings pane.
    fn request_permission(&self, which: Permission);
}

pub trait Screens {
    /// Bounds of the display that contains `p`.
    fn display_at(&self, p: Point) -> Option<Rect>;

    /// The frontmost window under `p`, ignoring this app's own windows.
    fn window_at(&self, p: Point) -> Option<WindowInfo>;
}

pub trait Accessibility {
    /// Reads the element at `p` in `app`, plus whatever `options` asks for.
    /// Apps that build their accessibility tree lazily (Chromium, Electron)
    /// are asked to build it on first contact.
    fn inspect(&self, app: &AppInfo, p: Point, options: &InspectOptions) -> Result<Inspection>;
}

pub trait ScreenCapture {
    /// Starts the slow part of taking screenshots ahead of time (at key
    /// down), so the capture on release is quicker.
    fn prepare_screenshots(&self);

    /// Takes the window and lens screenshots within `limits`, scaling on the
    /// way in. Either can be missing if it fails.
    fn screenshots(
        &self,
        window: Option<&WindowInfo>,
        lens: Rect,
        limits: ShotLimits,
    ) -> Result<Screenshots>;
}

/// The floating windows Context shows over other apps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayKind {
    /// Click-through, never takes focus.
    Lens,
    /// Clickable; takes keyboard focus only when asked to (ask mode) or when
    /// the user clicks its text box.
    Popover,
}

pub trait Overlay {
    /// One-time setup: on all Spaces, above full-screen apps, and never
    /// activating Context.
    fn configure_overlay(&self, window: &WebviewWindow, kind: OverlayKind) -> Result<()>;

    /// Places the overlay at `frame` and shows it. With `focus`, it also
    /// takes the keyboard, still without activating Context.
    fn show_overlay(&self, window: &WebviewWindow, frame: Rect, focus: bool) -> Result<()>;

    fn hide_overlay(&self, window: &WebviewWindow) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Errors

#[derive(Debug)]
pub enum PlatformError {
    /// This OS doesn't implement the feature yet.
    #[cfg_attr(
        not(target_os = "windows"),
        expect(dead_code, reason = "only the Windows stubs return it")
    )]
    NotSupported,
    Failed(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSupported => write!(f, "not supported on this OS yet"),
            Self::Failed(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for PlatformError {}

impl From<tauri::Error> for PlatformError {
    fn from(e: tauri::Error) -> Self {
        Self::Failed(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, PlatformError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains_is_half_open() {
        let r = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        assert!(r.contains(Point { x: 10.0, y: 20.0 }));
        assert!(r.contains(Point { x: 109.9, y: 69.9 }));
        assert!(!r.contains(Point { x: 110.0, y: 30.0 }));
        assert!(!r.contains(Point { x: 50.0, y: 70.0 }));
    }

    #[test]
    fn rect_intersection() {
        let a = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let b = Rect {
            x: 50.0,
            y: 80.0,
            width: 100.0,
            height: 100.0,
        };
        assert_eq!(
            a.intersection(&b),
            Some(Rect {
                x: 50.0,
                y: 80.0,
                width: 50.0,
                height: 20.0
            })
        );
        let far = Rect { x: 200.0, ..b };
        assert_eq!(a.intersection(&far), None);
    }

    #[test]
    fn image_limits_keep_both_bounds_and_never_upscale() {
        let limit = ImageLimit {
            max_long_edge: 1568,
            max_pixels: 1_150_000,
        };
        // A 2× MacBook screen: 4112×2658 px.
        let (w, h) = limit.fit(4112.0, 2658.0);
        assert!(w <= 1568 && u64::from(w) * u64::from(h) <= 1_150_000);
        assert!((f64::from(w) / f64::from(h) - 4112.0 / 2658.0).abs() < 0.01);
        assert_eq!(limit.fit(800.0, 600.0), (800, 600));
        let edge_only = ImageLimit {
            max_long_edge: 1024,
            max_pixels: u64::MAX,
        };
        assert_eq!(edge_only.fit(3000.0, 500.0), (1024, 170));
    }

    #[test]
    fn rect_relative_to_moves_only_the_origin() {
        let r = Rect::centered_at(
            Point {
                x: 1500.0,
                y: 300.0,
            },
            200.0,
            100.0,
        );
        let display_origin = Point { x: 1440.0, y: 0.0 };
        assert_eq!(
            r.relative_to(display_origin),
            Rect {
                x: -40.0,
                y: 250.0,
                width: 200.0,
                height: 100.0
            }
        );
    }
}
