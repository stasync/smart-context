//! Everything OS-specific: global input hooks, accessibility queries, screen
//! capture, overlay window behavior and opening System Settings panes.
//!
//! The rest of the app only sees the traits and types in this file
//! (docs/PLAN.md section 5.4). Each OS implements all of them on one type,
//! exported here as [`Native`].

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::WebviewWindow;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Native;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::Native;

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
    /// Any mouse button went down.
    MouseDown,
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

pub trait Overlay {
    /// One-time setup for the lens window: click-through, on all Spaces and
    /// above full-screen apps.
    fn configure_overlay(&self, window: &WebviewWindow) -> Result<()>;

    /// Moves the overlay to cover `display` and shows it without taking focus.
    fn show_overlay(&self, window: &WebviewWindow, display: Rect) -> Result<()>;

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
