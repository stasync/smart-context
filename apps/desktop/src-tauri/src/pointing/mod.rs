//! The pointing session: the hold-to-point gesture, the lens overlay, and
//! (from M2) the capture when the user releases.
//!
//! Input arrives on the platform's input thread, which must never block, so
//! [`Pointing`] only runs the gesture there and hands the resulting actions
//! to a worker thread that talks to the overlay window.

mod gesture;
mod lens;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use crate::platform::{
    Disposition, InputEvent, InputHandler, Native, Overlay, Point, Rect, Screens, WindowInfo,
};
use crate::settings::Settings;
use gesture::{Action, Gesture};
use lens::Lens;

pub const LENS_WINDOW: &str = "lens";
/// The event the lens window listens to for what to draw.
const LENS_EVENT: &str = "lens:update";

pub struct Pointing {
    enabled: AtomicBool,
    gesture: Mutex<Gesture>,
    actions: Sender<Action>,
}

impl Pointing {
    /// Starts the lens worker. Feed input to the result via [`InputHandler`].
    pub fn start(app: AppHandle, native: Arc<Native>, settings: Settings) -> Arc<Self> {
        let (actions, inbox) = mpsc::channel();
        let worker = Worker {
            app,
            native,
            settings,
            lens: Lens::default(),
            display: None,
            target: None,
        };
        thread::Builder::new()
            .name("context-lens".into())
            .spawn(move || worker.run(inbox))
            .expect("failed to start the lens thread");
        Arc::new(Self {
            enabled: AtomicBool::new(true),
            gesture: Mutex::new(Gesture::default()),
            actions,
        })
    }

    /// The tray's Enabled switch. Turning pointing off ends a gesture in progress.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
        if !on && let Some(action) = self.gesture().reset() {
            self.send(action);
        }
    }

    fn gesture(&self) -> MutexGuard<'_, Gesture> {
        self.gesture.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn send(&self, action: Action) {
        // The worker lives as long as the app, so this only fails at shutdown.
        let _ = self.actions.send(action);
    }
}

impl InputHandler for Pointing {
    fn handle(&self, event: InputEvent) -> Disposition {
        if !self.enabled.load(Ordering::Relaxed) {
            return Disposition::Pass;
        }
        let (disposition, action) = self.gesture().handle(event, Instant::now());
        if let Some(action) = action {
            self.send(action);
        }
        disposition
    }
}

/// What the lens window draws, in coordinates relative to the overlay.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LensView {
    visible: bool,
    rect: Rect,
    /// The cursor is over an excluded app: show "Context is off here".
    off_here: bool,
}

struct Worker {
    app: AppHandle,
    native: Arc<Native>,
    settings: Settings,
    lens: Lens,
    /// Bounds of the display the overlay currently covers.
    display: Option<Rect>,
    /// The window under the cursor.
    target: Option<WindowInfo>,
}

impl Worker {
    fn run(mut self, inbox: Receiver<Action>) {
        let Some(window) = self.app.get_webview_window(LENS_WINDOW) else {
            log::error!("the lens window is missing; pointing is off");
            return;
        };
        while let Ok(first) = inbox.recv() {
            // Mouse moves arrive faster than the lens redraws: skip any move
            // that another move already replaced.
            let batch: Vec<Action> = std::iter::once(first).chain(inbox.try_iter()).collect();
            for (i, action) in batch.iter().enumerate() {
                let superseded = matches!(action, Action::Move(_))
                    && matches!(batch.get(i + 1), Some(Action::Move(_)));
                if !superseded {
                    self.apply(&window, *action);
                }
            }
        }
    }

    fn apply(&mut self, window: &WebviewWindow, action: Action) {
        match action {
            Action::Show(p) => {
                self.lens.move_to(p);
                self.retarget(p);
                self.display = self.native.display_at(p);
                self.render(true);
                self.show(window);
            }
            Action::Move(p) => {
                self.lens.move_to(p);
                if !self.target.as_ref().is_some_and(|w| w.bounds.contains(p)) {
                    self.retarget(p);
                }
                let crossed_display = !self.display.is_some_and(|d| d.contains(p));
                if crossed_display {
                    self.display = self.native.display_at(p);
                }
                self.render(true);
                if crossed_display {
                    self.show(window);
                }
            }
            Action::Resize(delta) => {
                self.lens.resize(delta);
                self.render(true);
            }
            // Stepping between an element and its parent or child needs the
            // accessibility tree: M2.
            Action::Step(_) => {}
            Action::Cancel | Action::Ask | Action::AskMode => {
                self.render(false);
                if let Err(e) = self.native.hide_overlay(window) {
                    log::warn!("couldn't hide the lens: {e}");
                }
                if action != Action::Cancel && !self.is_off_here() {
                    // Capturing the context pack arrives in M2, answering in M3.
                    let app = self.target.as_ref().map(|w| &w.app);
                    log::info!("{action:?} at {:?} over {app:?}", self.lens.rect());
                }
            }
        }
    }

    fn retarget(&mut self, p: Point) {
        self.target = self.native.window_at(p);
    }

    fn is_off_here(&self) -> bool {
        let bundle_id = self
            .target
            .as_ref()
            .and_then(|w| w.app.bundle_id.as_deref());
        self.settings.is_excluded(bundle_id)
    }

    fn render(&self, visible: bool) {
        let origin = self.display.map(|d| d.origin()).unwrap_or_default();
        let view = LensView {
            visible,
            rect: self.lens.rect().relative_to(origin),
            off_here: self.is_off_here(),
        };
        if let Err(e) = self.app.emit_to(LENS_WINDOW, LENS_EVENT, view) {
            log::warn!("couldn't update the lens: {e}");
        }
    }

    fn show(&self, window: &WebviewWindow) {
        let Some(display) = self.display else {
            log::warn!("no display under the cursor");
            return;
        };
        if let Err(e) = self.native.show_overlay(window, display) {
            log::warn!("couldn't show the lens: {e}");
        }
    }
}
