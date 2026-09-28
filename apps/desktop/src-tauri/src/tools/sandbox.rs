//! The path sandbox for local tools (docs/PLAN.md 7): paths resolve inside
//! the project, symlinks included, and secret files are never readable.

use std::fmt;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum ToolError {
    NoProject,
    NotFound(String),
    OutsideProject,
    Secret,
    Binary,
    TooLarge,
    BadInput(String),
    Failed(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoProject => write!(f, "There's no project open."),
            Self::NotFound(path) => write!(f, "No such file or folder: {path}"),
            Self::OutsideProject => write!(f, "That path is outside the project."),
            Self::Secret => write!(f, "That file may contain secrets, so it can't be read."),
            Self::Binary => write!(f, "That's a binary file, not text."),
            Self::TooLarge => write!(f, "That file is too large; read a line range instead."),
            Self::BadInput(why) => write!(f, "Invalid input: {why}"),
            Self::Failed(why) => write!(f, "{why}"),
        }
    }
}

/// Folders whose contents are never read.
const SECRET_DIRS: &[&str] = &[".ssh", ".aws", ".gnupg", ".git"];
const SECRET_NAMES: &[&str] = &[".npmrc", ".pypirc", ".netrc", ".git-credentials"];
const SECRET_PREFIXES: &[&str] = &[".env", "id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"];
const SECRET_EXTENSIONS: &[&str] = &[
    ".pem",
    ".key",
    ".p12",
    ".pfx",
    ".keychain",
    ".keychain-db",
    ".kdbx",
    ".db",
    ".sqlite",
    ".sqlite3",
];
/// Anywhere in a name.
const SECRET_WORDS: &[&str] = &["secret", "credential"];

/// Whether a path (relative to the project) is on the secret deny-list.
pub fn is_secret(relative: &Path) -> bool {
    relative.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        let name = name.to_string_lossy().to_lowercase();
        SECRET_DIRS.contains(&name.as_str())
            || SECRET_NAMES.contains(&name.as_str())
            || SECRET_PREFIXES.iter().any(|p| name.starts_with(p))
            || SECRET_EXTENSIONS.iter().any(|e| name.ends_with(e))
            || SECRET_WORDS.iter().any(|w| name.contains(w))
    })
}

#[derive(Debug)]
pub struct Sandbox {
    /// Canonical project roots; the first one is the default for relative paths.
    roots: Vec<PathBuf>,
    /// The roots as the editor gave them, which may differ from the
    /// canonical ones (macOS's /tmp is /private/tmp).
    given: Vec<PathBuf>,
}

impl Sandbox {
    pub fn new(roots: &[PathBuf]) -> Self {
        let pairs: Vec<(PathBuf, PathBuf)> = roots
            .iter()
            .filter_map(|r| Some((r.canonicalize().ok()?, normalize(r))))
            .collect();
        Self {
            roots: pairs
                .iter()
                .map(|(canonical, _)| canonical.clone())
                .collect(),
            given: pairs.into_iter().map(|(_, given)| given).collect(),
        }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Resolves a path from the model: relative to the first root, or
    /// absolute inside any root. It must be inside the project as written
    /// (so the answer never reveals what exists elsewhere) and after
    /// following symlinks.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, ToolError> {
        let root = self.roots.first().ok_or(ToolError::NoProject)?;
        let path = path.trim();
        let candidate = normalize(&if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            root.join(path)
        });
        let inside_as_written = self
            .roots
            .iter()
            .chain(&self.given)
            .any(|r| candidate.starts_with(r));
        if !inside_as_written {
            return Err(ToolError::OutsideProject);
        }
        let real = candidate
            .canonicalize()
            .map_err(|_| ToolError::NotFound(path.to_string()))?;
        let relative = self
            .relative_to_root(&real)
            .ok_or(ToolError::OutsideProject)?;
        if is_secret(relative) {
            return Err(ToolError::Secret);
        }
        Ok(real)
    }

    /// How the model should see a path: relative to its root.
    pub fn display(&self, path: &Path) -> String {
        match self.relative_to_root(path) {
            Some(relative) if relative.as_os_str().is_empty() => ".".into(),
            Some(relative) => relative.to_string_lossy().into_owned(),
            None => path.to_string_lossy().into_owned(),
        }
    }

    fn relative_to_root<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        self.roots
            .iter()
            .find_map(|root| path.strip_prefix(root).ok())
    }
}

/// Resolves `.` and `..` without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn project() -> (tempfile::TempDir, Sandbox) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("app");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("package.json"), "{}").unwrap();
        fs::write(root.join("src/server.ts"), "export {}").unwrap();
        fs::write(dir.path().join("outside.txt"), "private").unwrap();
        let sandbox = Sandbox::new(&[root]);
        (dir, sandbox)
    }

    #[test]
    fn paths_inside_the_project_resolve() {
        let (_dir, sandbox) = project();
        let file = sandbox.resolve("src/server.ts").unwrap();
        assert!(file.ends_with("src/server.ts"));
        assert_eq!(sandbox.display(&file), "src/server.ts");
        let absolute = sandbox.roots()[0].join("package.json");
        assert!(sandbox.resolve(absolute.to_str().unwrap()).is_ok());
        assert_eq!(sandbox.display(&sandbox.resolve(".").unwrap()), ".");
    }

    #[test]
    fn escapes_are_refused() {
        let (dir, sandbox) = project();
        assert_eq!(
            sandbox.resolve("../outside.txt"),
            Err(ToolError::OutsideProject)
        );
        assert_eq!(
            sandbox.resolve("src/../../outside.txt"),
            Err(ToolError::OutsideProject)
        );
        let outside = dir.path().join("outside.txt");
        assert_eq!(
            sandbox.resolve(outside.to_str().unwrap()),
            Err(ToolError::OutsideProject)
        );
        assert_eq!(
            sandbox.resolve("/etc/hosts"),
            Err(ToolError::OutsideProject)
        );
        // Whether something exists outside the project is never revealed.
        assert_eq!(
            sandbox.resolve("../../no/such/file"),
            Err(ToolError::OutsideProject)
        );
        assert_eq!(
            sandbox.resolve("/no/such/file"),
            Err(ToolError::OutsideProject)
        );
        assert!(matches!(
            sandbox.resolve("missing.ts"),
            Err(ToolError::NotFound(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_pointing_outside_are_refused() {
        let (dir, sandbox) = project();
        let root = &sandbox.roots()[0];
        std::os::unix::fs::symlink(dir.path().join("outside.txt"), root.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(dir.path(), root.join("up")).unwrap();
        assert_eq!(sandbox.resolve("link.txt"), Err(ToolError::OutsideProject));
        assert_eq!(
            sandbox.resolve("up/outside.txt"),
            Err(ToolError::OutsideProject)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_secret_is_still_a_secret() {
        let (_dir, sandbox) = project();
        let root = &sandbox.roots()[0];
        fs::write(root.join(".env"), "TOKEN=1").unwrap();
        std::os::unix::fs::symlink(root.join(".env"), root.join("config.txt")).unwrap();
        assert_eq!(sandbox.resolve("config.txt"), Err(ToolError::Secret));
    }

    #[test]
    fn secret_files_are_refused_even_when_they_exist() {
        let (_dir, sandbox) = project();
        let root = sandbox.roots()[0].clone();
        for name in [
            ".env",
            ".env.local",
            "server.pem",
            "tls.key",
            "cert.p12",
            "id_rsa",
            "id_ed25519.pub",
            ".npmrc",
            ".pypirc",
            ".netrc",
            "secrets.json",
            "AWS_Credentials.yml",
            "data.sqlite",
            "login.keychain-db",
        ] {
            fs::write(root.join(name), "x").unwrap();
            assert_eq!(sandbox.resolve(name), Err(ToolError::Secret), "{name}");
        }
        for dir in [".ssh", ".aws", ".git", "config/secrets"] {
            fs::create_dir_all(root.join(dir)).unwrap();
            fs::write(root.join(dir).join("config"), "x").unwrap();
            assert_eq!(
                sandbox.resolve(&format!("{dir}/config")),
                Err(ToolError::Secret),
                "{dir}"
            );
        }
    }

    #[test]
    fn ordinary_names_are_not_secrets() {
        for name in [
            "package.json",
            "src/keyboard.ts",
            "README.md",
            "env.ts",
            "docs/key-points.md",
        ] {
            assert!(!is_secret(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn no_project_means_no_paths() {
        assert_eq!(Sandbox::new(&[]).resolve("a"), Err(ToolError::NoProject));
    }
}
