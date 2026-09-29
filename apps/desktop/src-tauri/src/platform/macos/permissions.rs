//! Accessibility and Screen Recording permissions.

use objc2_app_kit::NSWorkspace;
use objc2_application_services::{AXIsProcessTrustedWithOptions, kAXTrustedCheckOptionPrompt};
use objc2_core_foundation::{CFBoolean, CFDictionary};
use objc2_core_graphics::{CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess};
use objc2_foundation::{NSString, NSURL};

use crate::platform::{Permission, PermissionStatus};

const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const SCREEN_RECORDING_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";

pub fn status() -> PermissionStatus {
    PermissionStatus {
        // Not AXIsProcessTrusted alone: it can stay true after the switch
        // is turned off.
        accessibility: super::input::can_filter_events(),
        screen_recording: CGPreflightScreenCaptureAccess(),
    }
}

pub fn request(which: Permission) {
    // Asking first puts the app in System Settings' list, so the user only
    // has to flip its switch.
    match which {
        Permission::Accessibility => {
            let key = unsafe { kAXTrustedCheckOptionPrompt };
            let options = CFDictionary::from_slices(&[key], &[CFBoolean::new(true)]);
            unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) };
            open_url(ACCESSIBILITY_PANE);
        }
        Permission::ScreenRecording => {
            CGRequestScreenCaptureAccess();
            open_url(SCREEN_RECORDING_PANE);
        }
    }
}

fn open_url(url: &str) {
    match NSURL::URLWithString(&NSString::from_str(url)) {
        Some(url) => {
            NSWorkspace::sharedWorkspace().openURL(&url);
        }
        None => log::warn!("invalid System Settings URL: {url}"),
    }
}
