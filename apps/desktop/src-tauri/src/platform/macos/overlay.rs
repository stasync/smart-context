//! The lens overlay window: click-through, on every Space, above full-screen
//! apps, and shown without taking focus from the app the user points at.

use std::sync::OnceLock;

use objc2_app_kit::{
    NSPopUpMenuWindowLevel, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_graphics::{CGDisplayBounds, CGMainDisplayID};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use tauri::WebviewWindow;
use tauri_nspanel::Panel;

use crate::platform::{Rect, Result};
use panel::LensPanel;

/// `tauri_panel!` brings its own imports, so it gets a module to itself.
mod panel {
    tauri_nspanel::tauri_panel! {
        panel!(LensPanel {
            config: {
                can_become_key_window: false,
                can_become_main_window: false,
                is_floating_panel: true
            }
        })
    }
}

/// The lens stays a panel for the app's whole life.
static LENS_PANEL: OnceLock<LensPanel<tauri::Wry>> = OnceLock::new();

pub fn configure(window: &WebviewWindow) -> Result<()> {
    window.set_ignore_cursor_events(true)?;
    let target = window.clone();
    window.run_on_main_thread(move || {
        // A plain window from a background app never appears on another
        // app's full-screen Space; a non-activating panel does.
        let panel = match LensPanel::from_window(&target) {
            Ok(panel) => panel,
            Err(e) => {
                log::warn!("couldn't turn the lens into a panel: {e}");
                return;
            }
        };
        if let Err(e) = panel.add_style_mask(NSWindowStyleMask::NonactivatingPanel) {
            log::warn!("couldn't make the lens non-activating: {e}");
        }
        // Above the menu bar, popup menus and full-screen windows.
        panel.set_level(NSPopUpMenuWindowLevel as i64);
        panel.set_collection_behavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        panel.set_hides_on_deactivate(false);
        panel.set_has_shadow(false);
        panel.set_ignores_mouse_events(true);
        let _ = LENS_PANEL.set(panel);
    })?;
    Ok(())
}

pub fn show(window: &WebviewWindow, display: Rect) -> Result<()> {
    let primary_height = CGDisplayBounds(CGMainDisplayID()).size.height;
    let frame = to_cocoa(display, primary_height);
    on_ns_window(window, move |w| {
        w.setFrame_display(frame, true);
        // Unlike makeKeyAndOrderFront, this doesn't activate Context.
        w.orderFrontRegardless();
    })
}

pub fn hide(window: &WebviewWindow) -> Result<()> {
    on_ns_window(window, |w| w.orderOut(None))
}

/// Runs `f` with the window's NSWindow on the main thread, as AppKit requires.
fn on_ns_window(window: &WebviewWindow, f: impl FnOnce(&NSWindow) + Send + 'static) -> Result<()> {
    let target = window.clone();
    window.run_on_main_thread(move || match target.ns_window() {
        // SAFETY: Tauri returns the live NSWindow, and this runs on the main thread.
        Ok(ns_window) => f(unsafe { &*ns_window.cast::<NSWindow>() }),
        Err(e) => log::warn!("the lens window has no NSWindow: {e}"),
    })?;
    Ok(())
}

/// Converts global top-left coordinates to Cocoa's, whose origin is the
/// bottom-left of the primary display with y growing up.
fn to_cocoa(r: Rect, primary_height: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(r.x, primary_height - r.y - r.height),
        NSSize::new(r.width, r.height),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_cocoa_flips_y_against_the_primary_display() {
        // A 1440×900 display stacked above a 1920×1080 primary display.
        let upper = Rect {
            x: 0.0,
            y: -900.0,
            width: 1440.0,
            height: 900.0,
        };
        let frame = to_cocoa(upper, 1080.0);
        assert_eq!((frame.origin.x, frame.origin.y), (0.0, 1080.0));
        assert_eq!((frame.size.width, frame.size.height), (1440.0, 900.0));

        let primary = Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        assert_eq!(to_cocoa(primary, 1080.0).origin.y, 0.0);
    }
}
