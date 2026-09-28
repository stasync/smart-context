//! The inspector thread: all accessibility and screen-capture work, one job
//! at a time (docs/PLAN.md 5.4, "Threading"), so a slow app never stalls
//! the lens.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use super::Msg;
use crate::context::{
    Capture, Classifier, ContextPack, MAX_ANCESTORS, MAX_NEARBY_CHARS, SHOT_LIMITS,
};
use crate::orchestrator::Orchestrator;
use crate::platform::{
    Accessibility, AppInfo, InspectOptions, Native, Point, Rect, ScreenCapture, TextBudget,
    WindowInfo,
};
use crate::replay;

/// Ancestors fetched while aiming, for Shift+scroll to step through.
const AIM_ANCESTORS: usize = 8;
/// Starting values (docs/PLAN.md 4.3).
const TEXT_BUDGET: TextBudget = TextBudget {
    max_samples: 60,
    max_time: Duration::from_millis(150),
    max_chars: MAX_NEARBY_CHARS,
};
/// Tells open windows (the dev viewer) that a pack was saved.
pub const PACK_SAVED_EVENT: &str = "pack:saved";

pub enum Job {
    /// What's under the cursor while aiming.
    Hit { app: AppInfo, point: Point },
    /// Everything about where the user pointed, on release.
    Capture(CaptureJob),
}

pub struct CaptureJob {
    pub cursor: Point,
    pub lens: Rect,
    pub window: Option<WindowInfo>,
    pub focus_level: usize,
    /// Space was pressed: the user will type the question.
    pub ask_mode: bool,
}

pub struct Inspector {
    pub app: AppHandle,
    pub native: Arc<Native>,
    pub classifier: Classifier,
    pub orchestrator: Arc<Orchestrator>,
    /// Where packs are saved in dev mode.
    pub packs_dir: Option<PathBuf>,
    pub replies: Sender<Msg>,
}

impl Inspector {
    pub fn run(self, jobs: Receiver<Job>) {
        while let Ok(first) = jobs.recv() {
            let batch: Vec<Job> = std::iter::once(first).chain(jobs.try_iter()).collect();
            // Only the latest hit test matters; captures all run, in order.
            let last_hit = batch.iter().rposition(|j| matches!(j, Job::Hit { .. }));
            for (i, job) in batch.into_iter().enumerate() {
                match job {
                    Job::Hit { app, point } if Some(i) == last_hit => self.hit(&app, point),
                    Job::Hit { .. } => {}
                    Job::Capture(job) => self.capture(job),
                }
            }
        }
    }

    fn hit(&self, app: &AppInfo, point: Point) {
        let options = InspectOptions {
            max_ancestors: AIM_ANCESTORS,
            text_in: None,
            page_details: false,
        };
        let chain = match self.native.inspect(app, point, &options) {
            Ok(inspection) => inspection.chain,
            Err(e) => {
                log::debug!("hit test failed: {e}");
                Vec::new()
            }
        };
        // The worker lives as long as the app, so this only fails at shutdown.
        let _ = self.replies.send(Msg::Hit(chain));
    }

    fn capture(&self, job: CaptureJob) {
        let started = Instant::now();
        let inspection = job
            .window
            .as_ref()
            .map(|w| {
                let options = InspectOptions {
                    max_ancestors: job.focus_level + MAX_ANCESTORS,
                    text_in: Some((job.lens, TEXT_BUDGET)),
                    page_details: true,
                };
                self.native
                    .inspect(&w.app, job.cursor, &options)
                    .unwrap_or_else(|e| {
                        log::warn!("reading accessibility failed: {e}");
                        Default::default()
                    })
            })
            .unwrap_or_default();
        let inspected = started.elapsed();
        let screenshots = self
            .native
            .screenshots(job.window.as_ref(), job.lens, SHOT_LIMITS)
            .unwrap_or_else(|e| {
                log::warn!("taking screenshots failed: {e}");
                Default::default()
            });
        let shot = started.elapsed();

        let pack = ContextPack::build(
            Capture {
                cursor: job.cursor,
                lens: job.lens,
                window: job.window,
                inspection,
                focus_level: job.focus_level,
                screenshots,
            },
            &self.classifier,
        );
        log::info!(
            "captured a {:?} pack in {} ms (accessibility {} ms, screenshots {} ms, images {} ms)",
            pack.source,
            started.elapsed().as_millis(),
            inspected.as_millis(),
            (shot - inspected).as_millis(),
            (started.elapsed() - shot).as_millis(),
        );

        // In dev mode, every pack is kept for replay.
        if let Some(dir) = &self.packs_dir {
            match replay::save(&pack, dir) {
                Ok(key) => {
                    let _ = self.app.emit(PACK_SAVED_EVENT, key);
                }
                Err(e) => log::warn!("couldn't save the pack: {e}"),
            }
        }
        self.orchestrator.begin(pack, job.ask_mode);
    }
}
