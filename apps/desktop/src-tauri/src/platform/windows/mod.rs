//! Windows implementations. Stubs in the MVP: they compile and return
//! `NotSupported` (docs/PLAN.md section 12, phase 2).

use std::sync::Arc;

use tauri::WebviewWindow;

use super::{
    Accessibility, AppInfo, InputHandler, InputHooks, InspectOptions, Inspection, Overlay,
    OverlayKind, Permission, PermissionStatus, Permissions, PlatformError, Point, Rect, Result,
    ScreenCapture, Screens, Screenshots, ShotLimits, WindowInfo,
};

/// The Windows platform layer.
pub struct Native;

/// Windows has no reopen event; a second launch will need a single-instance
/// check (phase 2).
pub fn is_reopen(_event: &tauri::RunEvent) -> bool {
    false
}

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

impl Accessibility for Native {
    fn inspect(&self, _app: &AppInfo, _p: Point, _options: &InspectOptions) -> Result<Inspection> {
        Err(PlatformError::NotSupported)
    }
}

impl ScreenCapture for Native {
    fn prepare_screenshots(&self) {}

    fn screenshots(
        &self,
        _window: Option<&WindowInfo>,
        _lens: Rect,
        _limits: ShotLimits,
    ) -> Result<Screenshots> {
        Err(PlatformError::NotSupported)
    }
}

impl Overlay for Native {
    fn configure_overlay(&self, _window: &WebviewWindow, _kind: OverlayKind) -> Result<()> {
        Err(PlatformError::NotSupported)
    }

    fn show_overlay(&self, _window: &WebviewWindow, _frame: Rect, _focus: bool) -> Result<()> {
        Err(PlatformError::NotSupported)
    }

    fn hide_overlay(&self, _window: &WebviewWindow) -> Result<()> {
        Err(PlatformError::NotSupported)
    }
}
