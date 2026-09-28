//! Windows implementations. Stubs in the MVP: they compile and return
//! `NotSupported` (docs/PLAN.md section 12, phase 2).

use std::sync::Arc;

use tauri::WebviewWindow;

use super::{
    InputHandler, InputHooks, Overlay, Permission, PermissionStatus, Permissions, PlatformError,
    Point, Rect, Result, Screens, WindowInfo,
};

/// The Windows platform layer.
pub struct Native;

impl InputHooks for Native {
    fn start_input(&self, _handler: Arc<dyn InputHandler>) -> Result<()> {
        Err(PlatformError::NotSupported)
    }
}

impl Permissions for Native {
    /// Windows has no equivalent permissions to ask for.
    fn permission_status(&self) -> PermissionStatus {
        PermissionStatus {
            accessibility: true,
            screen_recording: true,
        }
    }

    fn request_permission(&self, _which: Permission) {}
}

impl Screens for Native {
    fn display_at(&self, _p: Point) -> Option<Rect> {
        None
    }

    fn window_at(&self, _p: Point) -> Option<WindowInfo> {
        None
    }
}

impl Overlay for Native {
    fn configure_overlay(&self, _window: &WebviewWindow) -> Result<()> {
        Err(PlatformError::NotSupported)
    }

    fn show_overlay(&self, _window: &WebviewWindow, _display: Rect) -> Result<()> {
        Err(PlatformError::NotSupported)
    }

    fn hide_overlay(&self, _window: &WebviewWindow) -> Result<()> {
        Err(PlatformError::NotSupported)
    }
}
