//! Displays and on-screen windows, from the window server.

use objc2_app_kit::NSRunningApplication;
use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFString, CFType, CGPoint, CGRect};
use objc2_core_graphics::{
    CGDisplayBounds, CGError, CGGetDisplaysWithPoint, CGRectMakeWithDictionaryRepresentation,
    CGWindowListCopyWindowInfo, CGWindowListOption, kCGNullWindowID, kCGWindowAlpha,
    kCGWindowBounds, kCGWindowLayer, kCGWindowName, kCGWindowNumber, kCGWindowOwnerName,
    kCGWindowOwnerPID,
};

use crate::platform::{AppInfo, Point, Rect, WindowInfo};

/// `kCGDockWindowLevel`. From here up are the Dock, the menu bar, status
/// items and other system UI. On macOS 26 the Dock keeps a full-screen window
/// at this level, so without this cut-off it would be "under" every point.
const DOCK_LAYER: i64 = 20;

pub fn display_at(p: Point) -> Option<Rect> {
    let mut display = 0;
    let mut count = 0;
    // SAFETY: both out-pointers are valid for one display.
    let err =
        unsafe { CGGetDisplaysWithPoint(CGPoint { x: p.x, y: p.y }, 1, &mut display, &mut count) };
    (err == CGError::Success && count > 0).then(|| rect(CGDisplayBounds(display)))
}

pub fn window_at(p: Point) -> Option<WindowInfo> {
    let list = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        kCGNullWindowID,
    )?;
    // SAFETY: the window list is an array of dictionaries with string keys.
    let list: &CFArray<CFDictionary<CFString, CFType>> = unsafe { list.cast_unchecked() };
    let own_pid = std::process::id() as i32;

    // The list runs front to back.
    list.iter()
        .filter_map(|window| parse_window(&window))
        .find(|w| w.app.pid != own_pid && w.bounds.contains(p))
}

fn parse_window(window: &CFDictionary<CFString, CFType>) -> Option<WindowInfo> {
    let number = |key: &CFString| window.get(key)?.downcast::<CFNumber>().ok();
    let string = |key: &CFString| {
        let value = window.get(key)?.downcast::<CFString>().ok()?;
        Some(value.to_string()).filter(|s| !s.is_empty())
    };

    // SAFETY: the kCGWindow* keys are constant CFStrings.
    let (id_key, pid_key, layer_key, alpha_key, bounds_key, owner_key, title_key) = unsafe {
        (
            kCGWindowNumber,
            kCGWindowOwnerPID,
            kCGWindowLayer,
            kCGWindowAlpha,
            kCGWindowBounds,
            kCGWindowOwnerName,
            kCGWindowName,
        )
    };

    // Only app windows are targets: not system UI, and not fully transparent
    // windows (some apps keep invisible ones on screen).
    if number(layer_key)?.as_i64()? >= DOCK_LAYER || number(alpha_key)?.as_f64()? <= 0.0 {
        return None;
    }
    let pid = number(pid_key)?.as_i32()?;
    let bounds_dict = window.get(bounds_key)?.downcast::<CFDictionary>().ok()?;
    let mut bounds = CGRect::default();
    // SAFETY: `bounds_dict` is a CGRect dictionary and `bounds` is a valid out-pointer.
    if !unsafe { CGRectMakeWithDictionaryRepresentation(Some(&bounds_dict), &mut bounds) } {
        return None;
    }

    Some(WindowInfo {
        id: number(id_key)?.as_i64()? as u32,
        // Titles are only visible with the Screen Recording permission.
        title: string(title_key),
        bounds: rect(bounds),
        app: AppInfo {
            name: string(owner_key).unwrap_or_default(),
            bundle_id: bundle_id(pid),
            pid,
        },
    })
}

fn bundle_id(pid: i32) -> Option<String> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?
        .bundleIdentifier()
        .map(|id| id.to_string())
}

fn rect(r: CGRect) -> Rect {
    Rect {
        x: r.origin.x,
        y: r.origin.y,
        width: r.size.width,
        height: r.size.height,
    }
}
