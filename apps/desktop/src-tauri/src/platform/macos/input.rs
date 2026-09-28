//! Global input: a session-level CGEventTap on its own thread and run loop.
//!
//! The tap is active (not listen-only), so it can swallow the scroll, Space
//! and Esc events pointing uses. It needs the Accessibility permission.

use std::cell::{Cell, OnceCell};
use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use objc2_application_services::AXIsProcessTrusted;
use objc2_core_foundation::{CFMachPort, CFRetained, CFRunLoop, kCFRunLoopCommonModes};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventMask, CGEventSource, CGEventSourceStateID,
    CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy, CGEventType,
};

use crate::platform::{Disposition, InputEvent, InputHandler, Key, PlatformError, Point, Result};

// Virtual key codes, from HIToolbox's Events.h.
const KEY_SPACE: i64 = 0x31;
const KEY_ESCAPE: i64 = 0x35;
const KEY_SHIFT: i64 = 0x38;
const KEY_RIGHT_SHIFT: i64 = 0x3C;
/// The pointing hotkey (docs/PLAN.md section 14). Configurable later.
const KEY_RIGHT_OPTION: i64 = 0x3D;

/// `NX_DEVICERALTKEYMASK` from IOKit's IOLLEvent.h: set in an event's flags
/// while the right Option key specifically is down.
const DEVICE_RIGHT_OPTION: u64 = 0x40;

/// How often to retry creating the tap while Accessibility isn't granted.
const PERMISSION_POLL: Duration = Duration::from_secs(1);

pub fn start(handler: Arc<dyn InputHandler>) -> Result<()> {
    thread::Builder::new()
        .name("context-input".into())
        .spawn(move || run(handler))
        .map(|_| ())
        .map_err(|e| PlatformError::Failed(format!("starting the input thread: {e}")))
}

/// Time since the last mouse move, drag or scroll, from any source.
pub fn pointer_still_for() -> Duration {
    [
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDragged,
        CGEventType::ScrollWheel,
    ]
    .into_iter()
    .map(|kind| {
        CGEventSource::seconds_since_last_event_type(
            CGEventSourceStateID::CombinedSessionState,
            kind,
        )
    })
    .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
    .map(Duration::from_secs_f64)
    .min()
    .unwrap_or(Duration::MAX)
}

/// State the tap callback needs. Lives on the input thread for good.
struct Tap {
    handler: Arc<dyn InputHandler>,
    /// Set once the tap exists, so the callback can re-enable it.
    port: OnceCell<CFRetained<CFMachPort>>,
    hotkey_down: Cell<bool>,
}

fn run(handler: Arc<dyn InputHandler>) {
    let tap = Box::new(Tap {
        handler,
        port: OnceCell::new(),
        hotkey_down: Cell::new(false),
    });
    let user_info = ptr::from_ref::<Tap>(&tap).cast_mut().cast::<c_void>();

    let port = loop {
        // An active tap can only be created once Accessibility is granted.
        if unsafe { AXIsProcessTrusted() } {
            // SAFETY: `callback` matches CGEventTapCallBack, and `user_info`
            // points at `tap`, which outlives the run loop below.
            let port = unsafe {
                CGEvent::tap_create(
                    CGEventTapLocation::SessionEventTap,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    event_mask(),
                    Some(callback),
                    user_info,
                )
            };
            if let Some(port) = port {
                break port;
            }
            log::warn!("couldn't create the input event tap; retrying");
        }
        thread::sleep(PERMISSION_POLL);
    };

    let Some(source) = CFMachPort::new_run_loop_source(None, Some(&port), 0) else {
        log::error!("couldn't create a run loop source for the input event tap");
        return;
    };
    let Some(run_loop) = CFRunLoop::current() else {
        log::error!("the input thread has no run loop");
        return;
    };
    run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    CGEvent::tap_enable(&port, true);
    let _ = tap.port.set(port);
    log::info!("input hooks started");

    // Runs for the rest of the app's life, keeping `tap` alive.
    CFRunLoop::run();
}

fn event_mask() -> CGEventMask {
    [
        CGEventType::FlagsChanged,
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDragged,
        CGEventType::LeftMouseDown,
        CGEventType::RightMouseDown,
        CGEventType::OtherMouseDown,
        CGEventType::ScrollWheel,
    ]
    .iter()
    .fold(0, |mask, t| mask | 1 << t.0)
}

unsafe extern "C-unwind" fn callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: `user_info` is the `Tap` owned by `run`, which never returns.
    let tap = unsafe { &*user_info.cast::<Tap>() };
    let passthrough = event.as_ptr();

    // macOS disables a tap that's too slow, or on some secure input. Turn it back on.
    if event_type == CGEventType::TapDisabledByTimeout
        || event_type == CGEventType::TapDisabledByUserInput
    {
        if let Some(port) = tap.port.get() {
            CGEvent::tap_enable(port, true);
        }
        return passthrough;
    }

    // SAFETY: the event is valid for the duration of the callback.
    let event = unsafe { event.as_ref() };
    // Never unwind into CoreGraphics: a panic lets the event through.
    let disposition = panic::catch_unwind(AssertUnwindSafe(|| tap.dispatch(event_type, event)))
        .unwrap_or(Disposition::Pass);
    match disposition {
        Disposition::Pass => passthrough,
        Disposition::Swallow => ptr::null_mut(),
    }
}

impl Tap {
    fn dispatch(&self, event_type: CGEventType, event: &CGEvent) -> Disposition {
        let flags = CGEvent::flags(Some(event)).bits();

        // If the hotkey's release was missed (say, while the tap was
        // disabled), every later event's flags show it's up. Catch up.
        if self.hotkey_down.get()
            && flags & DEVICE_RIGHT_OPTION == 0
            && event_type != CGEventType::FlagsChanged
        {
            self.hotkey_down.set(false);
            self.handler.handle(InputEvent::HotkeyUp);
        }

        match self.translate(event_type, event, flags) {
            Some(input) => self.handler.handle(input),
            None => Disposition::Pass,
        }
    }

    fn translate(
        &self,
        event_type: CGEventType,
        event: &CGEvent,
        flags: u64,
    ) -> Option<InputEvent> {
        let field = |f: CGEventField| CGEvent::integer_value_field(Some(event), f);
        match event_type {
            CGEventType::FlagsChanged => match field(CGEventField::KeyboardEventKeycode) {
                KEY_RIGHT_OPTION => {
                    let down = flags & DEVICE_RIGHT_OPTION != 0;
                    if down == self.hotkey_down.get() {
                        return None;
                    }
                    self.hotkey_down.set(down);
                    Some(if down {
                        InputEvent::HotkeyDown(location(event))
                    } else {
                        InputEvent::HotkeyUp
                    })
                }
                // Shift is part of the gesture (Shift+scroll).
                KEY_SHIFT | KEY_RIGHT_SHIFT => None,
                _ => Some(InputEvent::OtherModifier),
            },
            CGEventType::KeyDown => Some(InputEvent::KeyDown {
                key: key(field(CGEventField::KeyboardEventKeycode)),
                repeat: field(CGEventField::KeyboardEventAutorepeat) != 0,
            }),
            CGEventType::KeyUp => Some(InputEvent::KeyUp(key(field(
                CGEventField::KeyboardEventKeycode,
            )))),
            CGEventType::MouseMoved
            | CGEventType::LeftMouseDragged
            | CGEventType::RightMouseDragged
            | CGEventType::OtherMouseDragged => Some(InputEvent::MouseMoved(location(event))),
            CGEventType::LeftMouseDown
            | CGEventType::RightMouseDown
            | CGEventType::OtherMouseDown => Some(InputEvent::MouseDown(location(event))),
            CGEventType::ScrollWheel => {
                let shift = flags & CGEventFlags::MaskShift.bits() != 0;
                let mut delta = field(CGEventField::ScrollWheelEventPointDeltaAxis1);
                // macOS turns Shift+wheel into horizontal scrolling on mice.
                if shift && delta == 0 {
                    delta = field(CGEventField::ScrollWheelEventPointDeltaAxis2);
                }
                Some(InputEvent::Scroll {
                    delta: delta as f64,
                    shift,
                })
            }
            _ => None,
        }
    }
}

fn key(code: i64) -> Key {
    match code {
        KEY_SPACE => Key::Space,
        KEY_ESCAPE => Key::Escape,
        other => Key::Other(other as u32),
    }
}

fn location(event: &CGEvent) -> Point {
    let p = CGEvent::location(Some(event));
    Point { x: p.x, y: p.y }
}
