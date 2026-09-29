//! Global input: session-level event taps on their own thread and run loop.
//!
//! Two taps: an active one for the keys and scrolling that pointing may
//! swallow (the hotkey, Space, Esc, scroll), and a listen-only one for the
//! mouse, which is never swallowed. Both need the Accessibility permission.
//!
//! An active tap sits in the path of every key press on the Mac. If
//! Accessibility is turned off while it runs, macOS disables it, and turning
//! it back on then leaves a dead tap in that path: all input stops until a
//! restart. So a tap is re-enabled only after checking the permission, a
//! timer checks it every second, and the taps come down the moment it's gone.
//! They go back up once it's granted again.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use objc2_application_services::AXIsProcessTrusted;
use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFMachPort, CFRetained, CFRunLoop, CFRunLoopSource, CFRunLoopTimer,
    CFRunLoopTimerContext, kCFRunLoopCommonModes,
};
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

/// How often to check Accessibility, both while waiting for it and while
/// the taps run.
const PERMISSION_POLL: Duration = Duration::from_secs(1);
/// macOS disables a tap that answers too slowly. Turning it back on now and
/// then is normal; a tap that keeps being disabled is taken down instead and
/// put back after a pause.
const MAX_REENABLES: u32 = 5;
const REENABLE_WINDOW: Duration = Duration::from_secs(30);
const COOL_DOWN: Duration = Duration::from_secs(10);

pub fn start(handler: Arc<dyn InputHandler>) -> Result<()> {
    thread::Builder::new()
        .name("context-input".into())
        .spawn(move || run(handler))
        .map(|_| ())
        .map_err(|e| PlatformError::Failed(format!("starting the input thread: {e}")))
}

/// Whether this process may filter input events right now.
/// `AXIsProcessTrusted` can go on saying yes after Accessibility is turned
/// off, so this also creates a small active tap and removes it at once:
/// macOS refuses to create one without the permission.
pub fn can_filter_events() -> bool {
    if !unsafe { AXIsProcessTrusted() } {
        return false;
    }
    // SAFETY: `pass_through` matches CGEventTapCallBack and ignores its user
    // info; the tap is never added to a run loop and is gone before returning.
    let probe = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::TailAppendEventTap,
            CGEventTapOptions::Default,
            1 << CGEventType::KeyDown.0,
            Some(pass_through),
            ptr::null_mut(),
        )
    };
    match probe {
        Some(port) => {
            CGEvent::tap_enable(&port, false);
            port.invalidate();
            true
        }
        None => false,
    }
}

unsafe extern "C-unwind" fn pass_through(
    _proxy: CGEventTapProxy,
    _event_type: CGEventType,
    event: NonNull<CGEvent>,
    _user_info: *mut c_void,
) -> *mut CGEvent {
    event.as_ptr()
}

/// Time since the mouse last moved or dragged, from any source. Scrolling
/// doesn't count: it also resizes the lens, and the editor reports its own
/// scrolling.
pub fn pointer_still_for() -> Duration {
    [
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDragged,
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

/// Why the taps came down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    PermissionLost,
    KeptDisabled,
}

/// State the callbacks need. Lives on the input thread for good.
struct Tap {
    handler: Arc<dyn InputHandler>,
    run_loop: CFRetained<CFRunLoop>,
    /// The taps and the permission timer, while they're up.
    hooks: RefCell<Option<Hooks>>,
    hotkey_down: Cell<bool>,
    /// Re-enables in the current window: when it started, and how many.
    reenables: Cell<(Instant, u32)>,
    stopped: Cell<Option<Stop>>,
}

fn run(handler: Arc<dyn InputHandler>) {
    let Some(run_loop) = CFRunLoop::current() else {
        log::error!("the input thread has no run loop");
        return;
    };
    let tap = Box::new(Tap {
        handler,
        run_loop,
        hooks: RefCell::default(),
        hotkey_down: Cell::new(false),
        reenables: Cell::new((Instant::now(), 0)),
        stopped: Cell::new(None),
    });
    let info = ptr::from_ref::<Tap>(&tap).cast_mut().cast::<c_void>();

    loop {
        while !can_filter_events() {
            thread::sleep(PERMISSION_POLL);
        }
        // SAFETY: `info` points at `tap`, which outlives every hook (this
        // function never returns).
        let Some(hooks) = (unsafe { Hooks::install(&tap.run_loop, info) }) else {
            log::warn!("couldn't create the input event taps; retrying");
            thread::sleep(PERMISSION_POLL);
            continue;
        };
        *tap.hooks.borrow_mut() = Some(hooks);
        tap.stopped.set(None);
        log::info!("input hooks started");

        // Until a callback finds the permission gone, or the taps unusable.
        CFRunLoop::run();

        if let Some(hooks) = tap.hooks.borrow_mut().take() {
            hooks.uninstall(&tap.run_loop);
        }
        tap.hotkey_down.set(false);
        let _ = panic::catch_unwind(AssertUnwindSafe(|| {
            tap.handler.handle(InputEvent::HooksStopped)
        }));
        if tap.stopped.get() == Some(Stop::KeptDisabled) {
            log::warn!(
                "input hooks stopped: macOS kept disabling them; retrying in {} s",
                COOL_DOWN.as_secs()
            );
            thread::sleep(COOL_DOWN);
        } else {
            log::warn!("input hooks stopped: Accessibility is off");
        }
    }
}

/// The installed taps with their run loop sources, and the permission timer.
struct Hooks {
    taps: Vec<(CFRetained<CFMachPort>, CFRetained<CFRunLoopSource>)>,
    timer: CFRetained<CFRunLoopTimer>,
}

impl Hooks {
    /// # Safety
    ///
    /// `info` must point at a `Tap` that outlives the hooks.
    unsafe fn install(run_loop: &CFRunLoop, info: *mut c_void) -> Option<Self> {
        let mut taps = Vec::new();
        let kinds = [
            (CGEventTapOptions::Default, filter_mask()),
            (CGEventTapOptions::ListenOnly, listen_mask()),
        ];
        for (options, mask) in kinds {
            // SAFETY: `callback` matches CGEventTapCallBack, and the caller
            // guarantees `info`.
            let port = unsafe {
                CGEvent::tap_create(
                    CGEventTapLocation::SessionEventTap,
                    CGEventTapPlacement::HeadInsertEventTap,
                    options,
                    mask,
                    Some(callback),
                    info,
                )
            };
            let source = port
                .as_ref()
                .and_then(|port| CFMachPort::new_run_loop_source(None, Some(port), 0));
            let (Some(port), Some(source)) = (port, source) else {
                remove_taps(&taps, run_loop);
                return None;
            };
            run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
            taps.push((port, source));
        }

        let seconds = PERMISSION_POLL.as_secs_f64();
        let mut context = CFRunLoopTimerContext {
            version: 0,
            info,
            retain: None,
            release: None,
            copyDescription: None,
        };
        // SAFETY: `check_permission` matches CFRunLoopTimerCallBack, and the
        // context (copied by CoreFoundation) carries `info`.
        let timer = unsafe {
            CFRunLoopTimer::new(
                None,
                CFAbsoluteTimeGetCurrent() + seconds,
                seconds,
                0,
                0,
                Some(check_permission),
                &mut context,
            )
        };
        let Some(timer) = timer else {
            remove_taps(&taps, run_loop);
            return None;
        };
        run_loop.add_timer(Some(&timer), unsafe { kCFRunLoopCommonModes });
        Some(Self { taps, timer })
    }

    fn set_enabled(&self, on: bool) {
        for (port, _) in &self.taps {
            CGEvent::tap_enable(port, on);
        }
    }

    fn uninstall(self, run_loop: &CFRunLoop) {
        self.timer.invalidate();
        remove_taps(&self.taps, run_loop);
    }
}

fn remove_taps(
    taps: &[(CFRetained<CFMachPort>, CFRetained<CFRunLoopSource>)],
    run_loop: &CFRunLoop,
) {
    for (port, source) in taps {
        CGEvent::tap_enable(port, false);
        run_loop.remove_source(Some(source), unsafe { kCFRunLoopCommonModes });
        port.invalidate();
    }
}

/// What the active tap may swallow.
fn filter_mask() -> CGEventMask {
    mask(&[
        CGEventType::FlagsChanged,
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::ScrollWheel,
    ])
}

/// What pointing only watches.
fn listen_mask() -> CGEventMask {
    mask(&[
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDragged,
        CGEventType::LeftMouseDown,
        CGEventType::RightMouseDown,
        CGEventType::OtherMouseDown,
    ])
}

fn mask(types: &[CGEventType]) -> CGEventMask {
    types.iter().fold(0, |mask, t| mask | 1 << t.0)
}

unsafe extern "C-unwind" fn check_permission(_timer: *mut CFRunLoopTimer, info: *mut c_void) {
    // SAFETY: `info` is the `Tap` owned by `run`, which never returns.
    let tap = unsafe { &*info.cast::<Tap>() };
    if !can_filter_events() {
        tap.stop(Stop::PermissionLost);
    }
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

    // macOS disabled a tap: it answered too slowly, or the permission is
    // gone. Only in the first case may it be turned back on (module docs).
    if event_type == CGEventType::TapDisabledByTimeout
        || event_type == CGEventType::TapDisabledByUserInput
    {
        if !can_filter_events() {
            tap.stop(Stop::PermissionLost);
        } else if tap.may_reenable() {
            log::info!("macOS disabled an input tap ({event_type:?}); turning it back on");
            if let Some(hooks) = &*tap.hooks.borrow() {
                hooks.set_enabled(true);
            }
        } else {
            tap.stop(Stop::KeptDisabled);
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
    /// Turns the taps off at once, so they never hold up input, and ends the
    /// run loop; `run` then removes them.
    fn stop(&self, why: Stop) {
        if let Some(hooks) = &*self.hooks.borrow() {
            hooks.set_enabled(false);
        }
        self.stopped.set(Some(why));
        self.run_loop.stop();
    }

    /// Counts a re-enable; false once there have been too many lately.
    fn may_reenable(&self) -> bool {
        let (since, count) = self.reenables.get();
        let (since, count) = if since.elapsed() > REENABLE_WINDOW {
            (Instant::now(), 0)
        } else {
            (since, count)
        };
        self.reenables.set((since, count + 1));
        count < MAX_REENABLES
    }

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
