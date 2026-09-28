//! User settings as JSON in the app config dir.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

use serde::{Deserialize, Serialize};

use crate::engines::Effort;

/// Apps where Context never captures anything (docs/PLAN.md section 9).
/// Bundle IDs checked on 28 Sep 2026: Apple's on macOS 26, the others in
/// their Homebrew casks.
const DEFAULT_EXCLUDED_APPS: &[&str] = &[
    "com.apple.keychainaccess",
    "com.apple.Passwords",
    "com.1password.1password",
    "com.bitwarden.desktop",
    "org.keepassx.keepassxc",
    "me.proton.pass.electron",
    "in.sinew.Enpass-Desktop",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Bundle IDs of apps where pointing is off.
    pub excluded_apps: Vec<String>,
    /// Go deeper and manual choices never go past this (docs/PLAN.md 6.3).
    pub effort_ceiling: Effort,
    /// A BCP 47 language tag for answers; None follows the system.
    pub answer_language: Option<String>,
    /// Save every context pack for replay, and offer the capture viewer.
    /// Follows the build, not the settings file.
    #[serde(skip)]
    pub dev_mode: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            excluded_apps: DEFAULT_EXCLUDED_APPS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            effort_ceiling: Effort::High,
            answer_language: None,
            dev_mode: cfg!(debug_assertions),
        }
    }
}

impl Settings {
    pub fn is_excluded(&self, bundle_id: Option<&str>) -> bool {
        bundle_id.is_some_and(|id| self.excluded_apps.iter().any(|e| e == id))
    }

    /// The language answers are written in.
    pub fn language(&self) -> String {
        self.answer_language
            .clone()
            .or_else(sys_locale::get_locale)
            .unwrap_or_else(|| "en".into())
    }
}

/// Settings shared across the app and kept in a file.
pub struct SettingsStore {
    path: PathBuf,
    current: RwLock<Settings>,
}

impl SettingsStore {
    /// Loads `path`, falling back to defaults if it's missing or unreadable.
    pub fn load(path: PathBuf) -> Self {
        let current = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!("ignoring unreadable settings: {e}");
                Settings::default()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(e) => {
                log::warn!("couldn't read settings: {e}");
                Settings::default()
            }
        };
        Self {
            path,
            current: RwLock::new(current),
        }
    }

    pub fn get(&self) -> Settings {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Changes the settings and saves them.
    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> io::Result<Settings> {
        let mut current = self.current.write().unwrap_or_else(PoisonError::into_inner);
        change(&mut current);
        write_atomically(&self.path, &serde_json::to_vec_pretty(&*current)?)?;
        Ok(current.clone())
    }
}

/// Writes to a temporary file, then renames it over `path`, so a crash never
/// leaves half a file.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_managers_are_excluded_by_default() {
        let settings = Settings::default();
        assert!(settings.is_excluded(Some("com.1password.1password")));
        assert!(settings.is_excluded(Some("com.apple.Passwords")));
        assert!(!settings.is_excluded(Some("com.microsoft.VSCode")));
        assert!(!settings.is_excluded(None));
    }

    #[test]
    fn settings_survive_a_save_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let store = SettingsStore::load(path.clone());
        assert_eq!(store.get().effort_ceiling, Effort::High);

        store.update(|s| s.effort_ceiling = Effort::Max).unwrap();
        let reloaded = SettingsStore::load(path);
        assert_eq!(reloaded.get().effort_ceiling, Effort::Max);
        assert_eq!(
            reloaded.get().excluded_apps,
            Settings::default().excluded_apps
        );
    }

    #[test]
    fn a_broken_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, b"{ not json").unwrap();
        assert_eq!(SettingsStore::load(path).get(), Settings::default());
    }

    #[test]
    fn answers_follow_the_system_language_unless_set() {
        let mut settings = Settings::default();
        assert!(!settings.language().is_empty());
        settings.answer_language = Some("de".into());
        assert_eq!(settings.language(), "de");
    }
}
