//! Screenshots through ScreenCaptureKit (docs/PLAN.md 4.4). Needs the Screen
//! Recording permission.

use std::process;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use block2::RcBlock;
use image::RgbaImage;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{
    SCCaptureResolutionType, SCContentFilter, SCRunningApplication, SCScreenshotManager,
    SCShareableContent, SCStreamConfiguration, SCWindow,
};

use crate::platform::{
    ImageLimit, PlatformError, Rect, Result, Screenshots, ShotLimits, WindowInfo,
};

/// ScreenCaptureKit normally answers well within this.
const TIMEOUT: Duration = Duration::from_secs(2);
/// A window listing fetched ahead of time is used if it's this fresh.
const PREPARED_FOR: Duration = Duration::from_secs(3);

/// The window listing fetched at key down, and when.
static PREPARED: Mutex<Option<(Instant, Delivered<Retained<SCShareableContent>>)>> =
    Mutex::new(None);

/// Lists the windows in the background (the slow part of a capture), so the
/// capture on release can skip it.
pub fn prepare() {
    thread::spawn(|| match fetch_shareable_content() {
        Ok(content) => {
            *PREPARED.lock().unwrap_or_else(PoisonError::into_inner) =
                Some((Instant::now(), Delivered(content)));
        }
        Err(e) => log::debug!("preparing screenshots failed: {e}"),
    });
}

type Pending = Receiver<std::result::Result<RgbaImage, String>>;

pub fn screenshots(
    window: Option<&WindowInfo>,
    lens: Rect,
    limits: ShotLimits,
) -> Result<Screenshots> {
    let content = shareable_content()?;
    // Start both captures before waiting for either. ScreenCaptureKit scales
    // them to size on the way, far faster than resizing them afterwards.
    let window_shot = window
        .and_then(|w| find_window(&content, w.id))
        .map(|w| capture_window(&w, limits.window));
    let lens_shot = capture_region(&content, lens, limits.lens);
    let scale = lens_shot.as_ref().map_or(0.0, |(_, scale)| *scale);
    Ok(Screenshots {
        window: window_shot.and_then(|p| wait("window", p)),
        lens: lens_shot.and_then(|(p, _)| wait("lens", p)),
        scale,
    })
}

/// ScreenCaptureKit objects are immutable once delivered, so passing one to
/// the waiting thread is safe.
struct Delivered<T>(T);
// SAFETY: see above.
unsafe impl<T> Send for Delivered<T> {}

fn shareable_content() -> Result<Retained<SCShareableContent>> {
    let prepared = PREPARED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    match prepared {
        Some((at, Delivered(content))) if at.elapsed() < PREPARED_FOR => Ok(content),
        _ => fetch_shareable_content(),
    }
}

fn fetch_shareable_content() -> Result<Retained<SCShareableContent>> {
    let (tx, rx) = mpsc::sync_channel(1);
    let handler = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            // SAFETY: ScreenCaptureKit passes a valid object or null.
            let result = unsafe { Retained::retain(content) }
                .map(Delivered)
                .ok_or_else(|| error_text(error));
            let _ = tx.send(result);
        },
    );
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, true, &handler,
        );
    }
    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(Delivered(content))) => Ok(content),
        Ok(Err(e)) => Err(PlatformError::Failed(format!(
            "listing windows to capture: {e}"
        ))),
        Err(_) => Err(PlatformError::Failed(
            "listing windows to capture timed out".into(),
        )),
    }
}

fn find_window(content: &SCShareableContent, id: u32) -> Option<Retained<SCWindow>> {
    unsafe { content.windows() }
        .iter()
        .find(|w| unsafe { w.windowID() } == id)
}

fn capture_window(window: &SCWindow, limit: ImageLimit) -> Pending {
    // SAFETY: plain ScreenCaptureKit object setup.
    unsafe {
        let filter =
            SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), window);
        let scale = filter.pointPixelScale() as f64;
        let size = filter.contentRect().size;
        let (width, height) = limit.fit(size.width * scale, size.height * scale);
        let config = SCStreamConfiguration::new();
        config.setWidth(width as usize);
        config.setHeight(height as usize);
        config.setShowsCursor(false);
        config.setIgnoreShadowsSingleWindow(true);
        config.setCaptureResolution(SCCaptureResolutionType::Best);
        start(&filter, &config)
    }
}

/// What's on screen in `region` (on the display under its center), without
/// this app's own windows. Also returns that display's pixels per point.
fn capture_region(
    content: &SCShareableContent,
    region: Rect,
    limit: ImageLimit,
) -> Option<(Pending, f64)> {
    let center = region.center();
    let display = unsafe { content.displays() }
        .iter()
        .find(|d| rect(unsafe { d.frame() }).contains(center))?;
    let bounds = rect(unsafe { display.frame() });
    let area = region.intersection(&bounds)?;
    let own_pid = process::id() as i32;
    let own_apps: Vec<Retained<SCRunningApplication>> = unsafe { content.applications() }
        .iter()
        .filter(|a| unsafe { a.processID() } == own_pid)
        .collect();

    // SAFETY: plain ScreenCaptureKit object setup.
    unsafe {
        let filter = SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            &display,
            &NSArray::from_retained_slice(&own_apps),
            &NSArray::new(),
        );
        let scale = filter.pointPixelScale() as f64;
        let config = SCStreamConfiguration::new();
        // The source rectangle is relative to the display.
        config.setSourceRect(CGRect::new(
            CGPoint::new(area.x - bounds.x, area.y - bounds.y),
            CGSize::new(area.width, area.height),
        ));
        let (width, height) = limit.fit(area.width * scale, area.height * scale);
        config.setWidth(width as usize);
        config.setHeight(height as usize);
        config.setShowsCursor(false);
        config.setCaptureResolution(SCCaptureResolutionType::Best);
        Some((start(&filter, &config), scale))
    }
}

fn start(filter: &SCContentFilter, config: &SCStreamConfiguration) -> Pending {
    let (tx, rx) = mpsc::sync_channel(1);
    let handler = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
        // SAFETY: ScreenCaptureKit passes a valid image or null.
        let result = match unsafe { image.as_ref() } {
            Some(image) => to_rgba(image).ok_or_else(|| "couldn't read the pixels".to_string()),
            None => Err(error_text(error)),
        };
        let _ = tx.send(result);
    });
    unsafe {
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
            filter,
            config,
            Some(&handler),
        );
    }
    rx
}

fn wait(what: &str, pending: Pending) -> Option<RgbaImage> {
    match pending.recv_timeout(TIMEOUT) {
        Ok(Ok(image)) => Some(image),
        Ok(Err(e)) => {
            log::warn!("{what} screenshot failed: {e}");
            None
        }
        Err(_) => {
            log::warn!("{what} screenshot timed out");
            None
        }
    }
}

/// Redraws the screenshot into a plain sRGB RGBA buffer.
fn to_rgba(image: &CGImage) -> Option<RgbaImage> {
    let (width, height) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    let mut rgba = vec![0u8; width * height * 4];
    // SAFETY: the buffer holds width × height RGBA pixels and outlives the context.
    let context = unsafe {
        CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            width,
            height,
            8,
            width * 4,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }?;
    let full = CGRect::new(CGPoint::ZERO, CGSize::new(width as f64, height as f64));
    CGContext::draw_image(Some(&context), full, Some(image));
    drop(context);
    RgbaImage::from_raw(width as u32, height as u32, rgba)
}

fn error_text(error: *mut NSError) -> String {
    // SAFETY: ScreenCaptureKit passes a valid error or null.
    unsafe { error.as_ref() }.map_or_else(
        || "unknown error".into(),
        |e| e.localizedDescription().to_string(),
    )
}

fn rect(r: CGRect) -> Rect {
    Rect {
        x: r.origin.x,
        y: r.origin.y,
        width: r.size.width,
        height: r.size.height,
    }
}
