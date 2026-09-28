//! Turning a capture into a ContextPack: text caps, image scaling and
//! annotation, source classification (docs/PLAN.md 4.2).

mod classify;
mod images;

use chrono::{DateTime, Utc};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::platform::{
    ElementInfo, ImageLimit, Inspection, Point, Rect, Screenshots, ShotLimits, WindowInfo,
};
pub use classify::{Classifier, SourceHint};

// Starting values; tune them with the eval set (docs/PLAN.md 13).
pub const MAX_ANCESTORS: usize = 5;
pub const MAX_NEARBY_CHARS: usize = 4_000;
const JPEG_QUALITY: u8 = 80;

/// Screenshot sizes (docs/PLAN.md 4.4). The lens stays sharp; the window is
/// kept within the smallest vision limits of the supported models, so they
/// don't downscale it again.
pub const SHOT_LIMITS: ShotLimits = ShotLimits {
    lens: ImageLimit {
        max_long_edge: 1024,
        max_pixels: u64::MAX,
    },
    window: ImageLimit {
        max_long_edge: 1568,
        max_pixels: 1_150_000,
    },
};

/// Everything captured for one question (docs/PLAN.md 4.2). Serializes to
/// pack.json, with the images stored next to it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPack {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub cursor: Point,
    pub lens: Rect,
    /// Screen pixels per point where the user pointed.
    pub display_scale: f64,
    /// The window under the cursor, with its app.
    pub window: Option<WindowInfo>,
    pub url: Option<String>,
    pub selection: Option<String>,
    /// The element the lens was on.
    pub focus: Option<ElementInfo>,
    /// The focus element's ancestors, nearest first.
    pub ancestors: Vec<ElementInfo>,
    pub nearby_text: String,
    pub lens_image: Option<ImageData>,
    pub window_image: Option<ImageData>,
    pub source: SourceHint,
}

/// An encoded image. On disk, the bytes live in `file` next to pack.json.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageData {
    pub file: String,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

/// The raw material for a pack, straight from the platform.
pub struct Capture {
    pub cursor: Point,
    pub lens: Rect,
    pub window: Option<WindowInfo>,
    pub inspection: Inspection,
    /// Which element of `inspection.chain` the lens was on (Shift+scroll).
    pub focus_level: usize,
    pub screenshots: Screenshots,
}

impl ContextPack {
    pub fn build(capture: Capture, classifier: &Classifier) -> Self {
        let Capture {
            cursor,
            lens,
            window,
            inspection,
            focus_level,
            screenshots,
        } = capture;

        let mut chain = inspection.chain.into_iter().skip(focus_level);
        let focus = chain.next();
        let ancestors = chain.take(MAX_ANCESTORS).collect();
        let bundle_id = window.as_ref().and_then(|w| w.app.bundle_id.as_deref());
        let source = classifier.classify(bundle_id, inspection.url.as_deref());
        // Only a browser's URL says where the user is; other apps' web views
        // (VS Code's, for one) have internal ones.
        let url = inspection.url.filter(|_| classifier.is_browser(bundle_id));

        let display_scale = if screenshots.scale > 0.0 {
            screenshots.scale
        } else {
            1.0
        };
        let window_image = window
            .as_ref()
            .zip(screenshots.window)
            .map(|(w, img)| window_image(img, w.bounds, lens, cursor));
        let lens_image = screenshots.lens.map(lens_image);

        Self {
            id: Uuid::new_v4(),
            created_at: Utc::now(),
            cursor,
            lens,
            display_scale,
            window,
            url,
            selection: inspection.selection,
            focus,
            ancestors,
            nearby_text: inspection
                .nearby_text
                .chars()
                .take(MAX_NEARBY_CHARS)
                .collect(),
            lens_image,
            window_image,
            source,
        }
    }
}

/// The lens crop, sharp enough to read: PNG.
fn lens_image(img: RgbaImage) -> ImageData {
    let small = images::fit_within(img, SHOT_LIMITS.lens);
    ImageData {
        file: "lens.png".into(),
        media_type: "image/png".into(),
        width: small.width(),
        height: small.height(),
        bytes: images::encode_png(&small),
    }
}

/// The whole window, downscaled, with the lens outlined and the cursor
/// marked, so the model sees where the user pointed.
fn window_image(img: RgbaImage, window: Rect, lens: Rect, cursor: Point) -> ImageData {
    let mut small = images::fit_within(img, SHOT_LIMITS.window);
    let sx = f64::from(small.width()) / window.width;
    let sy = f64::from(small.height()) / window.height;
    let x = |v: f64| ((v - window.x) * sx).round() as i64;
    let y = |v: f64| ((v - window.y) * sy).round() as i64;
    images::outline(
        &mut small,
        (
            x(lens.x),
            y(lens.y),
            (lens.width * sx).round() as i64,
            (lens.height * sy).round() as i64,
        ),
    );
    images::cursor_marker(&mut small, x(cursor.x), y(cursor.y));
    ImageData {
        file: "window.jpg".into(),
        media_type: "image/jpeg".into(),
        width: small.width(),
        height: small.height(),
        bytes: images::encode_jpeg(&small, JPEG_QUALITY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::AppInfo;

    fn element(role: &str) -> ElementInfo {
        ElementInfo {
            role: Some(role.into()),
            ..Default::default()
        }
    }

    fn capture(focus_level: usize) -> Capture {
        let window = WindowInfo {
            id: 1,
            title: Some("Product page".into()),
            bounds: Rect {
                x: 100.0,
                y: 100.0,
                width: 1000.0,
                height: 800.0,
            },
            app: AppInfo {
                name: "Google Chrome".into(),
                bundle_id: Some("com.google.Chrome".into()),
                pid: 42,
            },
        };
        Capture {
            cursor: Point { x: 600.0, y: 500.0 },
            lens: Rect::centered_at(Point { x: 600.0, y: 500.0 }, 240.0, 120.0),
            window: Some(window),
            inspection: Inspection {
                chain: [
                    "AXStaticText",
                    "AXLink",
                    "AXGroup",
                    "AXGroup",
                    "AXGroup",
                    "AXGroup",
                    "AXGroup",
                    "AXWebArea",
                ]
                .map(element)
                .to_vec(),
                nearby_text: "x".repeat(MAX_NEARBY_CHARS + 10),
                selection: None,
                url: Some("https://www.amazon.com/dp/B0".into()),
            },
            focus_level,
            screenshots: Screenshots {
                // As captured for a 1000×800 pt window on a 2× screen.
                window: Some(RgbaImage::new(1197, 958)),
                lens: Some(RgbaImage::new(480, 240)),
                scale: 2.0,
            },
        }
    }

    #[test]
    fn build_fills_the_pack() {
        let pack = ContextPack::build(capture(0), &Classifier::builtin());
        assert_eq!(pack.source, SourceHint::Shopping);
        assert_eq!(pack.focus.unwrap().role.as_deref(), Some("AXStaticText"));
        assert_eq!(pack.ancestors.len(), MAX_ANCESTORS);
        assert_eq!(pack.display_scale, 2.0);
        assert_eq!(pack.nearby_text.chars().count(), MAX_NEARBY_CHARS);

        let lens = pack.lens_image.unwrap();
        assert_eq!(
            (lens.width, lens.height, lens.media_type.as_str()),
            (480, 240, "image/png")
        );
        let window = pack.window_image.unwrap();
        assert_eq!((window.width, window.height), (1197, 958));
        assert_eq!(window.media_type, "image/jpeg");
    }

    #[test]
    fn the_focus_follows_the_stepped_level() {
        let pack = ContextPack::build(capture(1), &Classifier::builtin());
        assert_eq!(pack.focus.unwrap().role.as_deref(), Some("AXLink"));
        assert_eq!(pack.ancestors[0].role.as_deref(), Some("AXGroup"));
    }

    #[test]
    fn the_pack_survives_missing_screenshots() {
        let mut c = capture(0);
        c.screenshots = Screenshots::default();
        let pack = ContextPack::build(c, &Classifier::builtin());
        assert!(pack.lens_image.is_none() && pack.window_image.is_none());
        assert_eq!(pack.display_scale, 1.0);
    }
}
