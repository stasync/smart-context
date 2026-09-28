//! The read-only file tools (docs/PLAN.md 7): list_dir, read_file and
//! search_project, plus the word search made before the first answer. Every
//! path goes through the sandbox.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use regex::{Regex, RegexBuilder};

use super::sandbox::{Sandbox, ToolError, is_secret};

// Starting values (docs/PLAN.md 7).
pub const MAX_DEPTH: usize = 3;
const MAX_ENTRIES: usize = 500;
const MAX_READ_BYTES: usize = 200 * 1024;
const MAX_READ_LINES: usize = 2_000;
/// Files bigger than this are never loaded, even for a line range.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MATCHES: usize = 100;
/// Search skips files bigger than this.
const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;
const SEARCH_TIME: Duration = Duration::from_secs(3);
const MAX_MATCH_CHARS: usize = 200;
/// The word search made before the first answer shows fewer matches, and
/// no more than a few per file, so one busy file can't hide the rest.
const WORD_MATCHES: usize = 40;
const WORD_MATCHES_PER_FILE: usize = 5;
/// Lockfiles would drown out the code that uses a package.
const LOCKFILES: &[&str] = &[
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lock",
    "Cargo.lock",
    "poetry.lock",
    "uv.lock",
    "Pipfile.lock",
    "composer.lock",
    "Gemfile.lock",
    "go.sum",
];

/// A tree of names under `path`, gitignore respected, secrets and `.git` left out.
pub fn list_dir(sandbox: &Sandbox, path: &str, depth: usize) -> Result<String, ToolError> {
    let dir = sandbox.resolve(path)?;
    if !dir.is_dir() {
        return Err(ToolError::BadInput(format!(
            "{path} is a file, not a folder"
        )));
    }
    let depth = depth.clamp(1, MAX_DEPTH);
    let mut lines = Vec::new();
    let mut more = 0;
    for entry in walker(&dir)
        .max_depth(Some(depth))
        .sort_by_file_name(|a, b| a.cmp(b))
        .build()
        .flatten()
    {
        if entry.depth() == 0 {
            continue;
        }
        if lines.len() == MAX_ENTRIES {
            more += 1;
            continue;
        }
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        let name = entry.file_name().to_string_lossy();
        let indent = "  ".repeat(entry.depth() - 1);
        lines.push(format!("{indent}{name}{}", if is_dir { "/" } else { "" }));
    }
    let mut out = format!("{}/\n{}", sandbox.display(&dir), lines.join("\n"));
    if more > 0 {
        let _ = write!(out, "\n… {more} more entries not shown");
    }
    Ok(out)
}

/// A text file's lines, numbered, optionally a 1-based inclusive range.
pub fn read_file(
    sandbox: &Sandbox,
    path: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<String, ToolError> {
    let file = sandbox.resolve(path)?;
    if file.is_dir() {
        return Err(ToolError::BadInput(format!(
            "{path} is a folder; use list_dir"
        )));
    }
    let size = fs::metadata(&file)
        .map_err(|e| ToolError::Failed(e.to_string()))?
        .len();
    if size > MAX_FILE_BYTES {
        return Err(ToolError::TooLarge);
    }
    let bytes = fs::read(&file).map_err(|e| ToolError::Failed(e.to_string()))?;
    let text = text_of(&bytes).ok_or(ToolError::Binary)?;

    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    let first = start_line.unwrap_or(1).max(1);
    let last = end_line
        .unwrap_or(total)
        .min(total)
        .min(first + MAX_READ_LINES - 1);
    if first > total.max(1) {
        return Err(ToolError::BadInput(format!("the file has {total} lines")));
    }

    let mut out = format!(
        "{} (lines {first}–{last} of {total})\n",
        sandbox.display(&file)
    );
    for (number, line) in lines.iter().enumerate().take(last).skip(first - 1) {
        if out.len() + line.len() > MAX_READ_BYTES {
            let _ = write!(
                out,
                "… stopped at {} KB; read a later range for more",
                MAX_READ_BYTES / 1024
            );
            break;
        }
        let _ = writeln!(out, "{:>5}  {line}", number + 1);
    }
    Ok(out)
}

/// Lines matching `query` across the project, as `path:line: text`.
pub fn search_project(
    sandbox: &Sandbox,
    query: &str,
    regex: bool,
    glob: Option<&str>,
) -> Result<String, ToolError> {
    if query.is_empty() {
        return Err(ToolError::BadInput("empty query".into()));
    }
    let pattern = if regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let matcher = smart_case(&pattern, query)?;
    let globs: Vec<String> = glob.into_iter().map(str::to_string).collect();
    let (matches, truncated) = scan(sandbox, &matcher, &globs, MAX_MATCHES, MAX_MATCHES)?;
    Ok(report(query, &matches, truncated))
}

/// Where a word appears in the project, as a whole word, lockfiles left out.
pub fn find_word(sandbox: &Sandbox, word: &str) -> Result<String, ToolError> {
    if word.is_empty() {
        return Err(ToolError::BadInput("empty word".into()));
    }
    let escaped = regex::escape(word);
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let pattern = if is_word(word.chars().next()) && is_word(word.chars().last()) {
        format!(r"\b{escaped}\b")
    } else {
        escaped
    };
    let matcher = smart_case(&pattern, word)?;
    let globs: Vec<String> = LOCKFILES.iter().map(|name| format!("!{name}")).collect();
    let (matches, truncated) = scan(
        sandbox,
        &matcher,
        &globs,
        WORD_MATCHES,
        WORD_MATCHES_PER_FILE,
    )?;
    Ok(report(word, &matches, truncated))
}

/// Case-sensitive only if the query has capitals.
fn smart_case(pattern: &str, query: &str) -> Result<Regex, ToolError> {
    RegexBuilder::new(pattern)
        .case_insensitive(!query.chars().any(char::is_uppercase))
        .size_limit(1 << 20)
        .build()
        .map_err(|e| ToolError::BadInput(e.to_string()))
}

fn report(query: &str, matches: &[String], truncated: bool) -> String {
    if matches.is_empty() {
        return format!("No matches for “{query}”.");
    }
    let mut out = matches.join("\n");
    if truncated {
        out.push_str("\n… more matches not shown; narrow the search");
    }
    out
}

/// Matching lines as `path:line: text`, and whether some were left out.
fn scan(
    sandbox: &Sandbox,
    matcher: &Regex,
    globs: &[String],
    limit: usize,
    per_file: usize,
) -> Result<(Vec<String>, bool), ToolError> {
    let deadline = Instant::now() + SEARCH_TIME;
    let mut matches = Vec::new();
    let mut truncated = false;
    'roots: for root in sandbox.roots() {
        let mut builder = walker(root);
        if !globs.is_empty() {
            let mut overrides = OverrideBuilder::new(root);
            for glob in globs {
                overrides
                    .add(glob)
                    .map_err(|e| ToolError::BadInput(format!("bad glob: {e}")))?;
            }
            let overrides = overrides
                .build()
                .map_err(|e| ToolError::BadInput(format!("bad glob: {e}")))?;
            builder.overrides(overrides);
        }
        for entry in builder.build().flatten() {
            if Instant::now() > deadline {
                truncated = true;
                break 'roots;
            }
            if !entry.file_type().is_some_and(|t| t.is_file())
                || entry
                    .metadata()
                    .map_or(true, |m| m.len() > MAX_SEARCH_FILE_BYTES)
            {
                continue;
            }
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            let Some(text) = text_of(&bytes) else {
                continue;
            };
            let mut in_file = 0;
            for (number, line) in text.lines().enumerate() {
                if matcher.is_match(line) {
                    if matches.len() == limit {
                        truncated = true;
                        break 'roots;
                    }
                    if in_file == per_file {
                        truncated = true;
                        break;
                    }
                    in_file += 1;
                    let line: String = line.trim().chars().take(MAX_MATCH_CHARS).collect();
                    matches.push(format!(
                        "{}:{}: {line}",
                        sandbox.display(entry.path()),
                        number + 1
                    ));
                }
            }
        }
    }
    Ok((matches, truncated))
}

/// A gitignore-respecting walker that never enters secret files or folders.
fn walker(dir: &Path) -> WalkBuilder {
    let base = dir.to_path_buf();
    let mut builder = WalkBuilder::new(dir);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .require_git(false)
        .filter_entry(move |entry| {
            let relative = entry.path().strip_prefix(&base).unwrap_or(entry.path());
            !is_secret(relative)
        });
    builder
}

/// Text if the bytes look like text: valid UTF-8 with no NUL in the first 8 KB.
fn text_of(bytes: &[u8]) -> Option<&str> {
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, Sandbox) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src/routes")).unwrap();
        fs::create_dir_all(root.join("node_modules/express")).unwrap();
        fs::write(root.join(".gitignore"), "node_modules/\ndist/\n").unwrap();
        fs::write(
            root.join("package.json"),
            "{\n  \"dependencies\": {\n    \"express\": \"^5.1.0\"\n  }\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("src/server.ts"),
            "import express from \"express\";\nconst app = express();\napp.listen(3000);\n",
        )
        .unwrap();
        fs::write(
            root.join("src/routes/users.ts"),
            "import { Router } from \"express\";\nexport const users = Router();\n",
        )
        .unwrap();
        fs::write(
            root.join("node_modules/express/index.js"),
            "module.exports = express;\n",
        )
        .unwrap();
        fs::write(root.join(".env"), "EXPRESS_SECRET=abc\n").unwrap();
        fs::write(root.join("logo.png"), [0x89, b'P', b'N', b'G', 0, 0, 1]).unwrap();
        let sandbox = Sandbox::new(&[root.to_path_buf()]);
        (dir, sandbox)
    }

    #[test]
    fn list_dir_shows_the_tree_without_ignored_or_secret_files() {
        let (_dir, sandbox) = project();
        let tree = list_dir(&sandbox, ".", 3).unwrap();
        assert!(tree.contains("src/"));
        assert!(
            tree.contains("    users.ts"),
            "nested entries are indented:\n{tree}"
        );
        assert!(tree.contains("package.json"));
        assert!(tree.contains(".gitignore"));
        assert!(!tree.contains("node_modules"), "gitignored:\n{tree}");
        assert!(!tree.contains(".env"), "secret:\n{tree}");

        let shallow = list_dir(&sandbox, ".", 1).unwrap();
        assert!(!shallow.contains("server.ts"));
        assert!(list_dir(&sandbox, "package.json", 1).is_err());
    }

    #[test]
    fn read_file_numbers_lines_and_honors_ranges() {
        let (_dir, sandbox) = project();
        let all = read_file(&sandbox, "src/server.ts", None, None).unwrap();
        assert!(all.starts_with("src/server.ts (lines 1–3 of 3)"));
        assert!(all.contains("    2  const app = express();"));

        let one = read_file(&sandbox, "src/server.ts", Some(2), Some(2)).unwrap();
        assert!(one.contains("    2  const app") && !one.contains("    1  import"));
        assert!(read_file(&sandbox, "src/server.ts", Some(9), None).is_err());
    }

    #[test]
    fn read_file_refuses_secrets_binaries_and_folders() {
        let (_dir, sandbox) = project();
        assert_eq!(
            read_file(&sandbox, ".env", None, None),
            Err(ToolError::Secret)
        );
        assert_eq!(
            read_file(&sandbox, "logo.png", None, None),
            Err(ToolError::Binary)
        );
        assert!(matches!(
            read_file(&sandbox, "src", None, None),
            Err(ToolError::BadInput(_))
        ));
        assert_eq!(
            read_file(&sandbox, "../../etc/passwd", None, None).unwrap_err(),
            ToolError::OutsideProject
        );
    }

    #[test]
    fn read_file_caps_long_files() {
        let (_dir, sandbox) = project();
        let long: String = (1..=3000).map(|i| format!("line {i}\n")).collect();
        fs::write(sandbox.roots()[0].join("long.txt"), long).unwrap();
        let out = read_file(&sandbox, "long.txt", None, None).unwrap();
        assert!(out.starts_with("long.txt (lines 1–2000 of 3000)"));
        assert!(!out.contains("line 2001"));
    }

    #[test]
    fn search_finds_usages_but_not_in_ignored_or_secret_files() {
        let (_dir, sandbox) = project();
        let found = search_project(&sandbox, "express", false, None).unwrap();
        assert!(found.contains("package.json:3:"), "{found}");
        assert!(found.contains("src/server.ts:1: import express from \"express\";"));
        assert!(found.contains("src/routes/users.ts:1:"));
        assert!(!found.contains("node_modules"), "gitignored");
        assert!(!found.contains(".env"), "secret");
    }

    #[test]
    fn search_supports_regex_globs_and_smart_case() {
        let (_dir, sandbox) = project();
        let routers = search_project(&sandbox, r"Router\(\)", true, None).unwrap();
        assert!(routers.contains("users.ts:2:") && !routers.contains("server.ts"));
        let ts_only = search_project(&sandbox, "express", false, Some("*.ts")).unwrap();
        assert!(!ts_only.contains("package.json"));
        assert!(
            search_project(&sandbox, "EXPRESS", false, None)
                .unwrap()
                .starts_with("No matches")
        );
        assert!(search_project(&sandbox, "(", true, None).is_err());
    }

    #[test]
    fn find_word_matches_whole_words_and_skips_lockfiles() {
        let (_dir, sandbox) = project();
        let root = sandbox.roots()[0].clone();
        fs::write(
            root.join("package-lock.json"),
            "{\"packages\":{\"node_modules/express\":{}}}\n",
        )
        .unwrap();
        fs::write(root.join("src/expressive.ts"), "const expressive = 1;\n").unwrap();
        let busy: String = (0..20).map(|i| format!("express.get({i});\n")).collect();
        fs::write(root.join("src/busy.ts"), busy).unwrap();

        let found = find_word(&sandbox, "express").unwrap();
        assert!(found.contains("package.json:3:"), "{found}");
        assert!(found.contains("src/server.ts:1:"));
        assert!(found.contains("src/routes/users.ts:1:"));
        assert!(!found.contains("package-lock.json"), "lockfile");
        assert!(!found.contains("expressive"), "whole words only");
        assert!(!found.contains(".env"), "secret");
        assert_eq!(
            found.matches("src/busy.ts:").count(),
            WORD_MATCHES_PER_FILE,
            "{found}"
        );
        assert!(found.ends_with("more matches not shown; narrow the search"));
        assert!(
            find_word(&sandbox, "@types/node")
                .unwrap()
                .starts_with("No matches")
        );
    }
}
