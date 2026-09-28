//! The pointing session: the hold-to-point gesture, the lens overlay, and the
//! capture when the user releases.
//!
//! Three threads share the work. Input arrives on the platform's input
//! thread, which must never block, so [`Pointing`] only runs the gesture
//! there. The lens worker drives the overlay window. The inspector does the
//! slow accessibility and screenshot work.

mod gesture;
mod inspector;
mod lens;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use crate::context::Classifier;
use crate::platform::{
    Disposition, ElementInfo, InputEvent, InputHandler, Native, Overlay, Point, Rect, Screens,
    WindowInfo,
};
use crate::settings::Settings;
use gesture::{Action, Gesture};
use inspector::{CaptureJob, Inspector, Job};
use lens::{Lens, Stepper};

pub const LENS_WINDOW: &str = "lens";
/// The event the lens window listens to for what to draw.
const LENS_EVENT: &str = "lens:update";

/// Messages for the lens worker.
enum Msg {
    Action(Action),
    /// The element under the cursor and its ancestors, from the inspector.
    Hit(Vec<ElementInfo>),
}

pub struct Pointing {
    enabled: AtomicBool,
    gesture: Mutex<Gesture>,
    worker: Sender<Msg>,
}

impl Pointing {
    /// Starts the lens and inspector threads. Feed input to the result via
    /// [`InputHandler`]. Packs are saved to `packs_dir` when it's set.
    pub fn start(
        app: AppHandle,
        native: Arc<Native>,
        settings: Settings,
        packs_dir: Option<PathBuf>,
    ) -> Arc<Self> {
        let (worker_tx, worker_rx) = mpsc::channel();
        let (inspector_tx, inspector_rx) = mpsc::channel();

        let inspector = Inspector {
            app: app.clone(),
            native: native.clone(),
            classifier: Classifier::builtin(),
            packs_dir,
            replies: worker_tx.clone(),
        };
        thread::Builder::new()
            .name("context-inspector".into())
            .spawn(move || inspector.run(inspector_rx))
            .expect("failed to start the inspector thread");

        let worker = Worker {
            app,
            native,
            settings,
            inspector: inspector_tx,
            lens: Lens::default(),
            stepper: Stepper::default(),
            aiming: false,
            cursor: Point::default(),
            display: None,
            target: None,
            chain: Vec::new(),
        };
        thread::Builder::new()
            .name("context-lens".into())
            .spawn(move || worker.run(worker_rx))
            .expect("failed to start the lens thread");

        Arc::new(Self {
            enabled: AtomicBool::new(true),
            gesture: Mutex::new(Gesture::default()),
            worker: worker_tx,
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
        let _ = self.worker.send(Msg::Action(action));
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
    /// The element under the cursor, while the lens isn't snapped to one.
    highlight: Option<Rect>,
    /// Shift+scroll snapped the lens to an element.
    snapped: bool,
    /// The cursor is over an excluded app: show "Context is off here".
    off_here: bool,
}

struct Worker {
    app: AppHandle,
    native: Arc<Native>,
    settings: Settings,
    inspector: Sender<Job>,
    lens: Lens,
    stepper: Stepper,
    aiming: bool,
    cursor: Point,
    /// Bounds of the display the overlay currently covers.
    display: Option<Rect>,
    /// The window under the cursor.
    target: Option<WindowInfo>,
    /// The element under the cursor, then its ancestors.
    chain: Vec<ElementInfo>,
}

impl Worker {
    fn run(mut self, inbox: Receiver<Msg>) {
        let Some(window) = self.app.get_webview_window(LENS_WINDOW) else {
            log::error!("the lens window is missing; pointing is off");
            return;
        };
        while let Ok(first) = inbox.recv() {
            // Mouse moves arrive faster than the lens redraws: skip any move
            // that another move already replaced.
            let batch: Vec<Msg> = std::iter::once(first).chain(inbox.try_iter()).collect();
            let is_move = |m: Option<&Msg>| matches!(m, Some(Msg::Action(Action::Move(_))));
            for (i, msg) in batch.iter().enumerate() {
                if is_move(Some(msg)) && is_move(batch.get(i + 1)) {
                    continue;
                }
                match msg {
                    Msg::Action(action) => self.apply(&window, *action),
                    Msg::Hit(chain) => self.hit(chain.clone()),
                }
            }
        }
    }

    fn apply(&mut self, window: &WebviewWindow, action: Action) {
        match action {
            Action::Show(p) => {
                self.aiming = true;
                self.stepper = Stepper::default();
                self.chain.clear();
                self.cursor = p;
                self.lens.move_to(p);
                self.retarget(p);
                self.display = self.native.display_at(p);
                self.request_hit();
                self.render(true);
                self.show(window);
            }
            Action::Move(p) => {
                self.cursor = p;
                self.lens.move_to(p);
                if !self.target.as_ref().is_some_and(|w| w.bounds.contains(p)) {
                    self.retarget(p);
                }
                let crossed_display = !self.display.is_some_and(|d| d.contains(p));
                if crossed_display {
                    self.display = self.native.display_at(p);
                }
                self.request_hit();
                self.render(true);
                if crossed_display {
                    self.show(window);
                }
            }
            Action::Resize(delta) => {
                self.stepper = Stepper::default();
                self.lens.resize(delta);
                self.render(true);
            }
            Action::Step(delta) => {
                if !self.chain.is_empty() {
                    self.stepper.scroll(delta, self.chain.len() - 1);
                    self.render(true);
                }
            }
            Action::Cancel | Action::Ask | Action::AskMode => {
                self.aiming = false;
                self.render(false);
                if let Err(e) = self.native.hide_overlay(window) {
                    log::warn!("couldn't hide the lens: {e}");
                }
                if action != Action::Cancel && !self.is_off_here() {
                    // Ask mode's typed question arrives with the popover in M3;
                    // both start from the same capture.
                    let job = CaptureJob {
                        cursor: self.cursor,
                        lens: self.lens_rect(),
                        window: self.target.clone(),
                        focus_level: self.stepper.level().unwrap_or(0),
                    };
                    let _ = self.inspector.send(Job::Capture(job));
                }
            }
        }
    }

    fn hit(&mut self, chain: Vec<ElementInfo>) {
        // A late answer from the last gesture.
        if !self.aiming {
            return;
        }
        self.chain = chain;
        self.stepper.clamp(self.chain.len().saturating_sub(1));
        self.render(true);
    }

    /// Asks the inspector what's under the cursor. Never in excluded apps.
    fn request_hit(&self) {
        if self.is_off_here() {
            return;
        }
        if let Some(window) = &self.target {
            let _ = self.inspector.send(Job::Hit {
                app: window.app.clone(),
                point: self.cursor,
            });
        }
    }

    fn retarget(&mut self, p: Point) {
        self.target = self.native.window_at(p);
        if self.is_off_here() {
            self.chain.clear();
            self.stepper = Stepper::default();
        }
    }

    fn is_off_here(&self) -> bool {
        let bundle_id = self
            .target
            .as_ref()
            .and_then(|w| w.app.bundle_id.as_deref());
        self.settings.is_excluded(bundle_id)
    }

    /// The snapped element's bounds, or the free lens around the cursor.
    fn lens_rect(&self) -> Rect {
        self.snapped_element().unwrap_or_else(|| self.lens.rect())
    }

    fn snapped_element(&self) -> Option<Rect> {
        self.chain.get(self.stepper.level()?)?.bounds
    }

    fn render(&self, visible: bool) {
        let origin = self.display.map(|d| d.origin()).unwrap_or_default();
        let snapped = self.snapped_element().is_some();
        let highlight = if snapped {
            None
        } else {
            self.chain.first().and_then(|e| e.bounds)
        };
        let view = LensView {
            visible,
            rect: self.lens_rect().relative_to(origin),
            highlight: highlight.map(|h| h.relative_to(origin)),
            snapped,
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
