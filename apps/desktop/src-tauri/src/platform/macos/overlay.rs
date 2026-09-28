//! Context's floating windows, the lens and the answer popover: on every
//! Space, above full-screen apps, and never activating Context, so the app
//! the user points at keeps its focus.

use std::sync::OnceLock;

use objc2_app_kit::{
    NSPopUpMenuWindowLevel, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_graphics::{CGDisplayBounds, CGMainDisplayID};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use tauri::WebviewWindow;
use tauri_nspanel::Panel;

use crate::platform::{OverlayKind, Rect, Result};
use panel::{LensPanel, PopoverPanel};

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
        panel!(PopoverPanel {
            config: {
                can_become_key_window: true,
                can_become_main_window: false,
                becomes_key_only_if_needed: true,
                is_floating_panel: true
            }
        })
    }
}

/// Both stay panels for the app's whole life.
static LENS_PANEL: OnceLock<LensPanel<tauri::Wry>> = OnceLock::new();
static POPOVER_PANEL: OnceLock<PopoverPanel<tauri::Wry>> = OnceLock::new();

pub fn configure(window: &WebviewWindow, kind: OverlayKind) -> Result<()> {
    if kind == OverlayKind::Lens {
        window.set_ignore_cursor_events(true)?;
    }
    let target = window.clone();
    window.run_on_main_thread(move || {
        // A plain window from a background app never appears on another
        // app's full-screen Space; a non-activating panel does.
        let result = match kind {
            OverlayKind::Lens => LensPanel::from_window(&target).map(|panel| {
                setup(&panel, false);
                let _ = LENS_PANEL.set(panel);
            }),
            OverlayKind::Popover => PopoverPanel::from_window(&target).map(|panel| {
                setup(&panel, true);
                let _ = POPOVER_PANEL.set(panel);
            }),
        };
        if let Err(e) = result {
            log::warn!("couldn't turn the {kind:?} window into a panel: {e}");
        }
    })?;
    Ok(())
}

fn setup(panel: &impl Panel, clickable: bool) {
    if let Err(e) = panel.add_style_mask(NSWindowStyleMask::NonactivatingPanel) {
        log::warn!("couldn't make a panel non-activating: {e}");
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
    panel.set_has_shadow(clickable);
    panel.set_ignores_mouse_events(!clickable);
}

pub fn show(window: &WebviewWindow, frame: Rect, focus: bool) -> Result<()> {
    let primary_height = CGDisplayBounds(CGMainDisplayID()).size.height;
    let frame = to_cocoa(frame, primary_height);
    on_ns_window(window, move |w| {
        w.setFrame_display(frame, true);
        // Unlike makeKeyAndOrderFront, neither of these activates Context.
        w.orderFrontRegardless();
        if focus {
            w.makeKeyWindow();
        }
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
        Err(e) => log::warn!("an overlay window has no NSWindow: {e}"),
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
