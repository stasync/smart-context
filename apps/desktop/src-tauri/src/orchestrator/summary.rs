//! The project summary cache (docs/PLAN.md 4.7): a short description of each
//! code project, made once at Low effort from its README and manifests, kept
//! in app data, and made again when a manifest changes. Later questions start
//! from it instead of re-reading the project.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

const MANIFESTS: &[&str] = &[
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Gemfile",
    "composer.json",
    "Package.swift",
];
const READMES: &[&str] = &[
    "README.md",
    "readme.md",
    "README",
    "README.rst",
    "README.txt",
];
// Starting values.
const MAX_README_CHARS: usize = 6_000;
const MAX_MANIFEST_CHARS: usize = 3_000;

#[derive(Serialize, Deserialize)]
struct Entry {
    root: PathBuf,
    summary: String,
    /// Each manifest's modified time when the summary was made.
    stamps: Vec<(String, u64)>,
}

pub struct ProjectSummaries {
    dir: PathBuf,
    /// Projects whose summary is being made right now.
    in_flight: Mutex<HashSet<PathBuf>>,
}

impl ProjectSummaries {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            in_flight: Mutex::default(),
        }
    }

    /// The saved summary, unless a manifest changed since it was made.
    pub fn get(&self, root: &Path) -> Option<String> {
        let entry: Entry = serde_json::from_slice(&fs::read(self.file(root)).ok()?).ok()?;
        (entry.root == root && entry.stamps == stamps(root)).then_some(entry.summary)
    }

    pub fn store(&self, root: &Path, summary: &str) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let entry = Entry {
            root: root.to_path_buf(),
            summary: summary.trim().to_string(),
            stamps: stamps(root),
        };
        fs::write(self.file(root), serde_json::to_vec_pretty(&entry)?)
    }

    /// Claims the job of making this project's summary. False if it's
    /// already being made.
    pub fn claim(&self, root: &Path) -> bool {
        self.in_flight().insert(root.to_path_buf())
    }

    pub fn release(&self, root: &Path) {
        self.in_flight().remove(root);
    }

    fn in_flight(&self) -> std::sync::MutexGuard<'_, HashSet<PathBuf>> {
        self.in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn file(&self, root: &Path) -> PathBuf {
        self.dir.join(format!("{:016x}.json", fnv1a(root)))
    }
}

/// What a summary is made from: the README and manifests, capped. Empty if
/// the project has none of them.
pub fn material(root: &Path) -> String {
    let mut parts = Vec::new();
    let read = |name: &str, cap: usize| -> Option<String> {
        let text = fs::read_to_string(root.join(name)).ok()?;
        Some(format!(
            "<file path=\"{name}\">\n{}\n</file>",
            text.chars().take(cap).collect::<String>()
        ))
    };
    if let Some(readme) = READMES.iter().find_map(|name| read(name, MAX_README_CHARS)) {
        parts.push(readme);
    }
    parts.extend(
        MANIFESTS
            .iter()
            .filter_map(|name| read(name, MAX_MANIFEST_CHARS)),
    );
    parts.join("\n\n")
}

fn stamps(root: &Path) -> Vec<(String, u64)> {
    MANIFESTS
        .iter()
        .filter_map(|name| {
            let modified = fs::metadata(root.join(name)).ok()?.modified().ok()?;
            let seconds = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
            Some((name.to_string(), seconds))
        })
        .collect()
}

/// A stable 64-bit hash (FNV-1a) of the project path, for the file name.
fn fnv1a(path: &Path) -> u64 {
    path.to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    #[test]
    fn a_summary_lasts_until_a_manifest_changes() {
        let cache_dir = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        fs::write(root.join("package.json"), "{}").unwrap();
        let cache = ProjectSummaries::new(cache_dir.path().to_path_buf());

        assert_eq!(cache.get(root), None);
        cache.store(root, " A shop API. \n").unwrap();
        assert_eq!(cache.get(root).as_deref(), Some("A shop API."));

        let file = fs::File::options()
            .write(true)
            .open(root.join("package.json"))
            .unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(60))
            .unwrap();
        assert_eq!(cache.get(root), None, "stale after package.json changed");
    }

    #[test]
    fn only_one_summary_is_made_at_a_time() {
        let cache = ProjectSummaries::new(PathBuf::from("/tmp/unused"));
        assert!(cache.claim(Path::new("/work/a")));
        assert!(!cache.claim(Path::new("/work/a")));
        cache.release(Path::new("/work/a"));
        assert!(cache.claim(Path::new("/work/a")));
    }

    #[test]
    fn material_is_the_readme_and_manifests() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        assert_eq!(material(root), "");
        fs::write(root.join("README.md"), "# Shop\nAn API.").unwrap();
        fs::write(root.join("package.json"), "{\"name\":\"shop\"}").unwrap();
        fs::write(root.join(".env"), "SECRET=1").unwrap();
        let text = material(root);
        assert!(text.contains("<file path=\"README.md\">\n# Shop"));
        assert!(text.contains("<file path=\"package.json\">"));
        assert!(!text.contains("SECRET"));
    }

    #[test]
    fn the_cache_key_is_stable() {
        assert_eq!(
            fnv1a(Path::new("/work/shop")),
            fnv1a(Path::new("/work/shop"))
        );
        assert_ne!(
            fnv1a(Path::new("/work/shop")),
            fnv1a(Path::new("/work/shop2"))
        );
    }
}
