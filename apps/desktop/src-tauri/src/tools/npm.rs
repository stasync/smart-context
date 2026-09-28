//! `npm_info` (docs/PLAN.md 7): what a package is, from registry.npmjs.org,
//! and which version this project has.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::Value;

use super::sandbox::ToolError;

const REGISTRY: &str = "https://registry.npmjs.org";
const TIMEOUT: Duration = Duration::from_secs(5);
const CACHE_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// Fetches package metadata, cached for a day.
pub struct NpmClient {
    http: reqwest::Client,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
}

impl NpmClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .expect("building the HTTP client");
        Self {
            http,
            cache: Mutex::default(),
        }
    }

    /// The package's latest manifest: small, unlike the full document.
    async fn latest(&self, name: &str) -> Result<Value, ToolError> {
        if let Some((at, value)) = self.cache().get(name)
            && at.elapsed() < CACHE_FOR
        {
            return Ok(value.clone());
        }
        let url = format!("{REGISTRY}/{}/latest", name.replace('/', "%2F"));
        let response = self
            .http
            .get(url)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|e| ToolError::Failed(format!("couldn't reach the npm registry: {e}")))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ToolError::Failed(format!(
                "npm has no package named {name}"
            )));
        }
        let value: Value = response
            .error_for_status()
            .map_err(|e| ToolError::Failed(e.to_string()))?
            .json()
            .await
            .map_err(|e| ToolError::Failed(e.to_string()))?;
        self.cache()
            .insert(name.to_string(), (Instant::now(), value.clone()));
        Ok(value)
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, HashMap<String, (Instant, Value)>> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub async fn info(&self, name: &str, root: Option<&Path>) -> Result<String, ToolError> {
        let name = name.trim();
        if !valid_name(name) {
            return Err(ToolError::BadInput(format!(
                "{name} isn't an npm package name"
            )));
        }
        let latest = self.latest(name).await?;
        let local = root.map(|r| local_versions(r, name)).unwrap_or_default();
        Ok(describe(name, &latest, &local))
    }
}

/// What this project says about a package: the declared range and the
/// installed version.
#[derive(Debug, Default, PartialEq)]
pub struct Local {
    pub declared: Option<String>,
    pub installed: Option<String>,
}

pub fn local_versions(root: &Path, name: &str) -> Local {
    let read =
        |path: &Path| -> Option<Value> { serde_json::from_slice(&fs::read(path).ok()?).ok() };
    let declared = read(&root.join("package.json")).and_then(|manifest| {
        [
            "dependencies",
            "devDependencies",
            "peerDependencies",
            "optionalDependencies",
        ]
        .iter()
        .find_map(|section| manifest[section][name].as_str().map(str::to_string))
    });
    let from_lock = read(&root.join("package-lock.json")).and_then(|lock| {
        lock["packages"][format!("node_modules/{name}")]["version"]
            .as_str()
            .or_else(|| lock["dependencies"][name]["version"].as_str())
            .map(str::to_string)
    });
    let installed = from_lock.or_else(|| {
        read(&root.join("node_modules").join(name).join("package.json"))
            .and_then(|m| m["version"].as_str().map(str::to_string))
    });
    Local {
        declared,
        installed,
    }
}

fn describe(name: &str, latest: &Value, local: &Local) -> String {
    let field = |key: &str| latest[key].as_str().filter(|s| !s.is_empty());
    let repository = latest["repository"]["url"]
        .as_str()
        .or_else(|| latest["repository"].as_str());
    let mut lines = vec![format!("{name} (npm)")];
    if let Some(description) = field("description") {
        lines.push(format!("Description: {description}"));
    }
    if let Some(version) = field("version") {
        lines.push(format!("Latest version: {version}"));
    }
    if let Some(declared) = &local.declared {
        lines.push(format!("This project asks for: {declared}"));
    }
    match &local.installed {
        Some(installed) => lines.push(format!("Installed here: {installed}")),
        None if local.declared.is_some() => lines.push("Installed here: not installed".into()),
        None => {}
    }
    if let Some(homepage) = field("homepage") {
        lines.push(format!("Homepage: {homepage}"));
    }
    if let Some(repository) = repository {
        lines.push(format!("Repository: {repository}"));
    }
    if let Some(license) = field("license") {
        lines.push(format!("License: {license}"));
    }
    lines.join("\n")
}

fn valid_name(name: &str) -> bool {
    static NAME: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r"^(@[a-z0-9-~][a-z0-9-._~]*/)?[a-z0-9-~][a-z0-9-._~]*$").unwrap()
    });
    name.len() <= 214 && NAME.is_match(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names_are_validated() {
        for ok in ["express", "@types/node", "lodash.merge", "socket.io"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "../etc", "Express", "a b", "@scope", "x/y"] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn local_versions_come_from_the_lockfile_or_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"express":"^5.1.0"},"devDependencies":{"vitest":"^5.0.0"}}"#,
        )
        .unwrap();
        fs::write(
            root.join("package-lock.json"),
            r#"{"packages":{"node_modules/express":{"version":"5.1.2"}}}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("node_modules/vitest")).unwrap();
        fs::write(
            root.join("node_modules/vitest/package.json"),
            r#"{"version":"5.0.2"}"#,
        )
        .unwrap();

        assert_eq!(
            local_versions(root, "express"),
            Local {
                declared: Some("^5.1.0".into()),
                installed: Some("5.1.2".into())
            }
        );
        assert_eq!(
            local_versions(root, "vitest").installed.as_deref(),
            Some("5.0.2")
        );
        assert_eq!(local_versions(root, "react"), Local::default());
    }

    #[test]
    fn the_description_combines_registry_and_project() {
        let latest = serde_json::json!({
            "name": "express",
            "version": "5.1.0",
            "description": "Fast, unopinionated, minimalist web framework",
            "homepage": "https://expressjs.com/",
            "repository": { "type": "git", "url": "git+https://github.com/expressjs/express.git" },
            "license": "MIT"
        });
        let local = Local {
            declared: Some("^5.1.0".into()),
            installed: None,
        };
        let text = describe("express", &latest, &local);
        assert!(text.contains("Description: Fast, unopinionated"));
        assert!(text.contains("This project asks for: ^5.1.0"));
        assert!(text.contains("Installed here: not installed"));
        assert!(text.contains("Repository: git+https://github.com/expressjs/express.git"));
    }
}
