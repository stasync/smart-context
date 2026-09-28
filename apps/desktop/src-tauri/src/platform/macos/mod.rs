//! macOS implementations: CGEventTap input, AX accessibility, ScreenCaptureKit and panels.

mod ax;
mod capture;
mod input;
mod overlay;
mod permissions;
mod screens;

use std::sync::Arc;

use tauri::WebviewWindow;

use super::{
    Accessibility, AppInfo, InputHandler, InputHooks, InspectOptions, Inspection, Overlay,
    Permission, PermissionStatus, Permissions, Point, Rect, Result, ScreenCapture, Screens,
    Screenshots, ShotLimits, WindowInfo,
};

/// The macOS platform layer.
pub struct Native;

/// The user opened the app again while it runs (Finder, Spotlight, `open`).
/// With the menu-bar icon hidden behind the notch, that's their way in.
pub fn is_reopen(event: &tauri::RunEvent) -> bool {
    matches!(event, tauri::RunEvent::Reopen { .. })
}

impl InputHooks for Native {
    fn start_input(&self, handler: Arc<dyn InputHandler>) -> Result<()> {
        input::start(handler)
    }
}

impl Permissions for Native {
    fn permission_status(&self) -> PermissionStatus {
        permissions::status()
    }

    fn request_permission(&self, which: Permission) {
        permissions::request(which)
    }
}

impl Screens for Native {
    fn display_at(&self, p: Point) -> Option<Rect> {
        screens::display_at(p)
    }

    fn window_at(&self, p: Point) -> Option<WindowInfo> {
        screens::window_at(p)
    }
}

impl Accessibility for Native {
    fn inspect(&self, app: &AppInfo, p: Point, options: &InspectOptions) -> Result<Inspection> {
        ax::inspect(app, p, options)
    }
}

impl ScreenCapture for Native {
    fn screenshots(
        &self,
        window: Option<&WindowInfo>,
        lens: Rect,
        limits: ShotLimits,
    ) -> Result<Screenshots> {
        capture::screenshots(window, lens, limits)
    }
}

impl Overlay for Native {
    fn configure_overlay(&self, window: &WebviewWindow) -> Result<()> {
        overlay::configure(window)
    }

    fn show_overlay(&self, window: &WebviewWindow, display: Rect) -> Result<()> {
        overlay::show(window, display)
    }

    fn hide_overlay(&self, window: &WebviewWindow) -> Result<()> {
        overlay::hide(window)
    }
}
