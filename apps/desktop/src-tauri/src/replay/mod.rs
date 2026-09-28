//! Saving and loading context packs (docs/PLAN.md 13.2): one folder per
//! pack, holding pack.json and its images.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::context::{ContextPack, ImageData, SourceHint};

const PACK_FILE: &str = "pack.json";

/// A saved pack, for lists.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    /// The pack's folder name.
    pub key: String,
    pub created_at: DateTime<Utc>,
    pub app: Option<String>,
    pub source: SourceHint,
}

/// Writes the pack into a new folder under `root`. Returns its key.
pub fn save(pack: &ContextPack, root: &Path) -> io::Result<String> {
    let key = format!(
        "{}-{}",
        pack.created_at.format("%Y%m%d-%H%M%S"),
        &pack.id.simple().to_string()[..8]
    );
    let dir = root.join(&key);
    fs::create_dir_all(&dir)?;
    for image in [&pack.lens_image, &pack.window_image].into_iter().flatten() {
        fs::write(dir.join(plain_name(&image.file)?), &image.bytes)?;
    }
    fs::write(dir.join(PACK_FILE), serde_json::to_vec_pretty(pack)?)?;
    Ok(key)
}

pub fn load(root: &Path, key: &str) -> io::Result<ContextPack> {
    let dir = root.join(plain_name(key)?);
    let mut pack: ContextPack = serde_json::from_slice(&fs::read(dir.join(PACK_FILE))?)?;
    for image in [&mut pack.lens_image, &mut pack.window_image]
        .into_iter()
        .flatten()
    {
        image.bytes = fs::read(dir.join(plain_name(&image.file)?))?;
    }
    Ok(pack)
}

/// Saved packs under `root`, newest first. Folders without a readable pack
/// are skipped.
pub fn list(root: &Path) -> io::Result<Vec<PackSummary>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut packs: Vec<PackSummary> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let key = entry.file_name().into_string().ok()?;
            let bytes = fs::read(entry.path().join(PACK_FILE)).ok()?;
            let pack: ContextPack = serde_json::from_slice(&bytes).ok()?;
            Some(PackSummary {
                key,
                created_at: pack.created_at,
                app: pack.window.map(|w| w.app.name),
                source: pack.source,
            })
        })
        .collect();
    packs.sort_by_key(|p| std::cmp::Reverse(p.created_at));
    Ok(packs)
}

/// A pack with its images inline, for showing it in a webview.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackView {
    pack: ContextPack,
    lens_image_url: Option<String>,
    window_image_url: Option<String>,
}

pub fn view(pack: &ContextPack) -> PackView {
    PackView {
        lens_image_url: pack.lens_image.as_ref().map(data_url),
        window_image_url: pack.window_image.as_ref().map(data_url),
        pack: pack.clone(),
    }
}

/// An image as a `data:` URL, for showing it in a webview.
pub fn data_url(image: &ImageData) -> String {
    format!(
        "data:{};base64,{}",
        image.media_type,
        STANDARD.encode(&image.bytes)
    )
}

/// Pack keys come from the webview and file names from pack.json: accept
/// only plain names, never paths.
fn plain_name(name: &str) -> io::Result<PathBuf> {
    let plain = !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\']);
    if plain {
        Ok(PathBuf::from(name))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a plain file name: {name:?}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Capture, Classifier};
    use crate::platform::{Inspection, Point, Rect, Screenshots};

    fn pack() -> ContextPack {
        ContextPack::build(
            Capture {
                cursor: Point { x: 10.0, y: 10.0 },
                lens: Rect::centered_at(Point { x: 10.0, y: 10.0 }, 20.0, 10.0),
                window: None,
                inspection: Inspection::default(),
                focus_level: 0,
                screenshots: Screenshots {
                    window: None,
                    lens: Some(image::RgbaImage::new(40, 20)),
                    scale: 2.0,
                },
            },
            &Classifier::builtin(),
        )
    }

    #[test]
    fn a_saved_pack_loads_back_with_its_images() {
        let root = tempfile::tempdir().unwrap();
        let original = pack();
        let key = save(&original, root.path()).unwrap();

        let loaded = load(root.path(), &key).unwrap();
        assert_eq!(loaded.id, original.id);
        let (a, b) = (loaded.lens_image.unwrap(), original.lens_image.unwrap());
        assert_eq!(a.bytes, b.bytes);
        assert!(!a.bytes.is_empty());
        assert!(data_url(&a).starts_with("data:image/png;base64,"));

        let listed = list(root.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, key);
    }

    #[test]
    fn listing_is_newest_first_and_skips_junk() {
        let root = tempfile::tempdir().unwrap();
        let older = save(&pack(), root.path()).unwrap();
        let mut newer = pack();
        newer.created_at += chrono::Duration::seconds(5);
        let newer = save(&newer, root.path()).unwrap();
        fs::create_dir(root.path().join("not-a-pack")).unwrap();

        let keys: Vec<String> = list(root.path())
            .unwrap()
            .into_iter()
            .map(|p| p.key)
            .collect();
        assert_eq!(keys, vec![newer, older]);
    }

    #[test]
    fn a_missing_root_lists_nothing() {
        let root = tempfile::tempdir().unwrap();
        assert!(list(&root.path().join("missing")).unwrap().is_empty());
    }

    #[test]
    fn keys_that_are_paths_are_refused() {
        let root = tempfile::tempdir().unwrap();
        for key in ["../etc", "a/b", "..", "", ".hidden"] {
            let err = load(root.path(), key).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{key:?}");
        }
    }
}
