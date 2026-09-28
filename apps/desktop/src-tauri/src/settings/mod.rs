//! User settings as JSON in the app config dir.
//!
//! For now only the built-in defaults exist; storing and editing them comes
//! with the settings store.

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

#[derive(Clone, Debug)]
pub struct Settings {
    /// Bundle IDs of apps where pointing is off.
    pub excluded_apps: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            excluded_apps: DEFAULT_EXCLUDED_APPS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

impl Settings {
    pub fn is_excluded(&self, bundle_id: Option<&str>) -> bool {
        bundle_id.is_some_and(|id| self.excluded_apps.iter().any(|e| e == id))
    }
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
}
