//! Accessibility (AX): the element under a point, its ancestors, the text
//! around it, the selection and the page URL (docs/PLAN.md 4.3).

use std::cmp::Ordering;
use std::collections::HashSet;
use std::ptr::{self, NonNull};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::Instant;

use objc2_application_services::{
    AXCopyMultipleAttributeOptions, AXError, AXUIElement, AXValue, AXValueType,
};
use objc2_core_foundation::{
    CFArray, CFBoolean, CFNumber, CFRetained, CFString, CFType, CFURL, CGPoint, CGSize, Type,
};

use crate::platform::{
    AppInfo, ElementInfo, InspectOptions, Inspection, Point, Rect, Result, TextBudget,
};

/// The longest a single AX call may wait on an unresponsive app. Starting value.
const MESSAGING_TIMEOUT_SECS: f32 = 0.25;
/// Per-attribute cap: a text area's value can be a whole document. Starting value.
const MAX_ATTRIBUTE_CHARS: usize = 500;
/// Starting value.
const MAX_SELECTION_CHARS: usize = 2_000;
/// How far up to look for the web area that carries a page's URL.
const MAX_DEPTH_FOR_URL: usize = 40;

/// Chromium browsers build their full tree only for AXEnhancedUserInterface.
/// Electron apps take AXManualAccessibility, which has no side effects, so
/// that's tried first everywhere.
const CHROMIUM_BROWSERS: &[&str] = &[
    "com.google.Chrome",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "company.thebrowser.Browser",
];

/// The attributes `describe` reads in one round trip, in this order.
const DESCRIBE: [&str; 9] = [
    "AXRole",
    "AXSubrole",
    "AXRoleDescription",
    "AXTitle",
    "AXValue",
    "AXDescription",
    "AXHelp",
    "AXPosition",
    "AXSize",
];

/// Containers whose own title or value would repeat their children's text.
const CONTAINER_ROLES: &[&str] = &[
    "AXApplication",
    "AXWindow",
    "AXScrollArea",
    "AXSplitGroup",
    "AXLayoutArea",
    "AXWebArea",
];

/// Processes already asked to build their full accessibility tree.
static PREPARED: LazyLock<Mutex<HashSet<i32>>> = LazyLock::new(Mutex::default);

pub fn inspect(app: &AppInfo, p: Point, options: &InspectOptions) -> Result<Inspection> {
    // SAFETY: creating an application element has no preconditions.
    let root = unsafe { AXUIElement::new_application(app.pid) };
    unsafe { root.set_messaging_timeout(MESSAGING_TIMEOUT_SECS) };
    prepare(&root, app);

    let Some(leaf) = element_at(&root, p) else {
        return Ok(Inspection::default());
    };
    let mut elements = vec![leaf];
    while elements.len() <= options.max_ancestors {
        let Some(parent) = elements
            .last()
            .and_then(|e| element_attribute(e, "AXParent"))
        else {
            break;
        };
        elements.push(parent);
    }
    let chain: Vec<ElementInfo> = elements.iter().map(|e| describe(e)).collect();

    let nearby_text = options
        .text_in
        .map(|(rect, budget)| text_in_rect(&root, rect, p, budget))
        .unwrap_or_default();

    let (selection, url) = if options.page_details {
        (selected_text(&root), page_url(&elements[0]))
    } else {
        (None, None)
    };

    Ok(Inspection {
        chain,
        nearby_text,
        selection,
        url,
    })
}

/// Chromium and Electron build their accessibility tree only when asked.
fn prepare(root: &AXUIElement, app: &AppInfo) {
    let first_contact = PREPARED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(app.pid);
    if !first_contact {
        return;
    }
    let on = CFBoolean::new(true);
    // SAFETY: both attributes take a CFBoolean.
    let manual = unsafe {
        root.set_attribute_value(&CFString::from_static_str("AXManualAccessibility"), on)
    };
    let chromium = app
        .bundle_id
        .as_deref()
        .is_some_and(|id| CHROMIUM_BROWSERS.contains(&id));
    if manual != AXError::Success && chromium {
        // Known to glitch some window animations, so only where it's needed.
        unsafe {
            root.set_attribute_value(&CFString::from_static_str("AXEnhancedUserInterface"), on)
        };
    }
}

fn element_at(root: &AXUIElement, p: Point) -> Option<CFRetained<AXUIElement>> {
    let mut element: *const AXUIElement = ptr::null();
    // SAFETY: `element` is a valid out-pointer.
    let err = unsafe {
        root.copy_element_at_position(p.x as f32, p.y as f32, NonNull::from(&mut element))
    };
    take(err, element)
}

/// Takes ownership of a value an AX "Copy" function returned.
fn take<T: Type>(err: AXError, value: *const T) -> Option<CFRetained<T>> {
    if err != AXError::Success {
        return None;
    }
    // SAFETY: on success, AX hands over a +1 reference.
    NonNull::new(value.cast_mut()).map(|v| unsafe { CFRetained::from_raw(v) })
}

fn attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<CFType>> {
    let mut value: *const CFType = ptr::null();
    // SAFETY: `value` is a valid out-pointer.
    let err = unsafe {
        element.copy_attribute_value(&CFString::from_static_str(name), NonNull::from(&mut value))
    };
    take(err, value)
}

fn element_attribute(element: &AXUIElement, name: &'static str) -> Option<CFRetained<AXUIElement>> {
    attribute(element, name)?.downcast::<AXUIElement>().ok()
}

fn describe(element: &AXUIElement) -> ElementInfo {
    let names: Vec<CFRetained<CFString>> = DESCRIBE
        .iter()
        .map(|n| CFString::from_static_str(n))
        .collect();
    let names: Vec<&CFString> = names.iter().map(|n| &**n).collect();
    let names = CFArray::from_objects(&names);
    let mut values: *const CFArray = ptr::null();
    // SAFETY: `names` holds attribute-name strings and `values` is a valid out-pointer.
    let err = unsafe {
        element.copy_multiple_attribute_values(
            names.as_opaque(),
            AXCopyMultipleAttributeOptions::empty(),
            NonNull::from(&mut values),
        )
    };
    let Some(values) = take(err, values) else {
        return ElementInfo::default();
    };
    // SAFETY: the result holds one value (or an AXValue error) per attribute.
    let values: &CFArray<CFType> = unsafe { values.cast_unchecked() };
    let text = |i: usize| values.get(i).and_then(|v| to_text(&v, MAX_ATTRIBUTE_CHARS));
    let position = values
        .get(7)
        .and_then(|v| ax_value::<CGPoint>(&v, AXValueType::CGPoint));
    let size = values
        .get(8)
        .and_then(|v| ax_value::<CGSize>(&v, AXValueType::CGSize));

    ElementInfo {
        role: text(0),
        subrole: text(1),
        role_description: text(2),
        title: text(3),
        value: text(4),
        description: text(5),
        help: text(6),
        bounds: position.zip(size).map(|(p, s)| Rect {
            x: p.x,
            y: p.y,
            width: s.width,
            height: s.height,
        }),
    }
}

/// Strings, numbers and URLs as text; anything else (and empty text) as None.
fn to_text(value: &CFType, max_chars: usize) -> Option<String> {
    let text = if let Some(s) = value.downcast_ref::<CFString>() {
        s.to_string()
    } else if let Some(url) = value.downcast_ref::<CFURL>() {
        url.string().to_string()
    } else {
        value.downcast_ref::<CFNumber>()?.as_f64()?.to_string()
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.chars().take(max_chars).collect())
}

fn ax_value<T: Default>(value: &CFType, kind: AXValueType) -> Option<T> {
    let value = value.downcast_ref::<AXValue>()?;
    let mut out = T::default();
    // SAFETY: `out` has the layout `kind` describes.
    unsafe { value.value(kind, NonNull::from(&mut out).cast()) }.then_some(out)
}

/// The text under `rect`. Hit-tests a grid of points inside it, a row per
/// line of text and nearest `cursor` first, then joins the distinct
/// elements' text in reading order. (Walking down from an ancestor instead
/// spends the whole budget on off-screen content in long documents.)
fn text_in_rect(root: &AXUIElement, rect: Rect, cursor: Point, budget: TextBudget) -> String {
    let deadline = Instant::now() + budget.max_time;
    let mut found: Vec<(CFRetained<AXUIElement>, ElementInfo)> = Vec::new();
    for p in sample_points(rect, cursor, budget.max_samples) {
        if Instant::now() >= deadline {
            break;
        }
        let Some(element) = element_at(root, p) else {
            continue;
        };
        if found.iter().any(|(e, _)| **e == *element) {
            continue;
        }
        let info = describe(&element);
        found.push((element, info));
    }

    // Reading order: top to bottom, then left to right.
    let position = |info: &ElementInfo| info.bounds.map_or((f64::MAX, f64::MAX), |b| (b.y, b.x));
    found.sort_by(|(_, a), (_, b)| {
        position(a)
            .partial_cmp(&position(b))
            .unwrap_or(Ordering::Equal)
    });

    let mut collected = TextCollector::new(budget.max_chars);
    for (_, info) in found {
        let container = info
            .role
            .as_deref()
            .is_some_and(|r| CONTAINER_ROLES.contains(&r));
        if !container && let Some(text) = info.value.or(info.title).or(info.description) {
            collected.push(text);
        }
    }
    collected.finish()
}

/// Points inside `rect`: a row about every line of text, a few columns
/// across, nearest `cursor` first.
fn sample_points(rect: Rect, cursor: Point, max: usize) -> Vec<Point> {
    const ROW_SPACING: f64 = 12.0;
    const COLUMNS: usize = 4;
    let rows = ((rect.height / ROW_SPACING).ceil() as usize).max(1);
    let mut points: Vec<Point> = (0..rows)
        .flat_map(|row| {
            (0..COLUMNS).map(move |col| Point {
                x: rect.x + rect.width * (col as f64 + 0.5) / COLUMNS as f64,
                y: rect.y + rect.height * (row as f64 + 0.5) / rows as f64,
            })
        })
        .collect();
    let distance = |p: &Point| (p.x - cursor.x).hypot(p.y - cursor.y);
    points.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    points.truncate(max);
    points
}

fn selected_text(root: &AXUIElement) -> Option<String> {
    let focused = element_attribute(root, "AXFocusedUIElement")?;
    let selection = attribute(&focused, "AXSelectedText")?;
    to_text(&selection, MAX_SELECTION_CHARS)
}

/// A browser page's URL, from the web area that contains the element.
fn page_url(leaf: &AXUIElement) -> Option<String> {
    let mut element = leaf.retain();
    for _ in 0..MAX_DEPTH_FOR_URL {
        let role = attribute(&element, "AXRole").and_then(|r| to_text(&r, 64));
        if role.as_deref() == Some("AXWebArea") {
            return attribute(&element, "AXURL").and_then(|u| to_text(&u, 2_000));
        }
        element = element_attribute(&element, "AXParent")?;
    }
    None
}

/// Joins distinct snippets with newlines, up to a character budget.
struct TextCollector {
    text: String,
    seen: HashSet<String>,
    max_chars: usize,
}

impl TextCollector {
    fn new(max_chars: usize) -> Self {
        Self {
            text: String::new(),
            seen: HashSet::new(),
            max_chars,
        }
    }

    fn is_full(&self) -> bool {
        self.text.chars().count() >= self.max_chars
    }

    fn push(&mut self, snippet: String) {
        if self.is_full() || !self.seen.insert(snippet.clone()) {
            return;
        }
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        let room = self.max_chars.saturating_sub(self.text.chars().count());
        self.text.extend(snippet.chars().take(room));
    }

    fn finish(self) -> String {
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_cover_the_rect_nearest_the_cursor_first() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 120.0,
        };
        let cursor = Point { x: 30.0, y: 5.0 };
        let points = sample_points(rect, cursor, 100);
        assert_eq!(points.len(), 40, "10 rows of 4");
        assert!(points.iter().all(|p| rect.contains(*p)));
        assert_eq!(points[0], Point { x: 30.0, y: 6.0 });
        assert_eq!(sample_points(rect, cursor, 7).len(), 7);
    }

    #[test]
    fn text_collector_skips_repeats_and_stops_at_the_budget() {
        let mut c = TextCollector::new(12);
        c.push("hello".into());
        c.push("hello".into());
        c.push("world wide web".into());
        assert!(c.is_full());
        c.push("more".into());
        assert_eq!(c.finish(), "hello\nworld ");
    }
}
