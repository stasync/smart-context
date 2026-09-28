//! macOS implementations: CGEventTap input, AX accessibility, ScreenCaptureKit and panels.

mod input;
mod overlay;
mod permissions;
mod screens;

use std::sync::Arc;

use tauri::WebviewWindow;

use super::{
    InputHandler, InputHooks, Overlay, Permission, PermissionStatus, Permissions, Point, Rect,
    Result, Screens, WindowInfo,
};

/// The macOS platform layer.
pub struct Native;

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
