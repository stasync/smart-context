//! Local, read-only tools and the path sandbox (docs/PLAN.md 7). Offered only
//! in code mode, when the VS Code bridge reported a project.

mod files;
mod npm;
pub mod sandbox;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::context::ContextPack;
use crate::engines::ToolSpec;
use crate::orchestrator::{Lookup, NoTools, ToolNote, ToolProvider, Toolbox};
pub use npm::NpmClient;
use sandbox::{Sandbox, ToolError};

/// Gives code-mode conversations the project tools, others nothing.
pub struct ProjectToolProvider {
    npm: Arc<NpmClient>,
}

impl ProjectToolProvider {
    pub fn new() -> Self {
        Self {
            npm: Arc::new(NpmClient::new()),
        }
    }
}

impl ToolProvider for ProjectToolProvider {
    fn tools_for(&self, pack: &ContextPack) -> Arc<dyn Toolbox> {
        match &pack.workspace {
            Some(workspace) if !workspace.roots.is_empty() => Arc::new(ProjectTools {
                sandbox: Arc::new(Sandbox::new(&workspace.roots)),
                npm: self.npm.clone(),
            }),
            _ => Arc::new(NoTools),
        }
    }
}

pub struct ProjectTools {
    sandbox: Arc<Sandbox>,
    npm: Arc<NpmClient>,
}

#[async_trait]
impl Toolbox for ProjectTools {
    fn specs(&self) -> Vec<ToolSpec> {
        vec![
            spec(
                "list_dir",
                "Lists a project folder as a tree (gitignored files left out). Paths are relative to the project root.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Folder, relative to the project root. \".\" for the root." },
                        "depth": { "type": "integer", "minimum": 1, "maximum": files::MAX_DEPTH, "description": "How many levels deep, 1–3. Default 2." }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }),
            ),
            spec(
                "read_file",
                "Reads a text file in the project, with line numbers. Optionally a line range (1-based, inclusive).",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File, relative to the project root." },
                        "start_line": { "type": "integer", "minimum": 1 },
                        "end_line": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }),
            ),
            spec(
                "search_project",
                "Searches the project's files for lines matching a query, as path:line: text. Case-insensitive unless the query has capitals.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "regex": { "type": "boolean", "description": "Treat the query as a regular expression. Default false." },
                        "glob": { "type": "string", "description": "Only files matching this glob, like \"*.ts\" or \"src/**\"." }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            ),
            spec(
                "npm_info",
                "Looks up an npm package: its description, latest version, homepage and repository, plus the version this project declares and has installed.",
                json!({
                    "type": "object",
                    "properties": { "package": { "type": "string" } },
                    "required": ["package"],
                    "additionalProperties": false
                }),
            ),
        ]
    }

    fn describe(&self, name: &str, input: &Value) -> ToolNote {
        let text = |key: &str| input[key].as_str().unwrap_or("?").to_string();
        let (status, record) = match name {
            "list_dir" => (
                format!("Listing {}…", text("path")),
                format!("Listed {}", text("path")),
            ),
            "read_file" => (
                format!("Reading {}…", text("path")),
                format!("Read {}", text("path")),
            ),
            "search_project" => (
                format!("Searching for “{}”…", text("query")),
                format!("Searched for “{}”", text("query")),
            ),
            "npm_info" => (
                format!("Looking up {} on npm…", text("package")),
                format!("Looked up {} on npm", text("package")),
            ),
            other => (format!("Using {other}…"), format!("Used {other}")),
        };
        ToolNote { status, record }
    }

    /// Searches the project for the word under the pointer, so even a quick
    /// answer can say where it's used.
    async fn lookups(&self, pack: &ContextPack) -> Vec<Lookup> {
        let Some(word) = pack
            .workspace
            .as_ref()
            .and_then(|w| w.pointer.as_ref())
            .map(|p| p.word.trim().to_string())
            .filter(|w| worth_searching(w))
        else {
            return Vec::new();
        };
        let sandbox = self.sandbox.clone();
        let query = word.clone();
        let found = tokio::task::spawn_blocking(move || files::find_word(&sandbox, &query)).await;
        match found {
            Ok(Ok(results)) => vec![Lookup {
                record: format!("Searched for “{word}”"),
                text: format!(
                    "Where “{word}” appears in the project (a search made for you):\n<search_results>\n{results}\n</search_results>"
                ),
            }],
            Ok(Err(e)) => {
                log::debug!("the word search failed: {e}");
                Vec::new()
            }
            Err(e) => {
                log::debug!("the word search failed: {e}");
                Vec::new()
            }
        }
    }

    async fn run(&self, name: &str, input: &Value) -> Result<String, String> {
        let sandbox = self.sandbox.clone();
        let input = input.clone();
        let result = match name {
            "npm_info" => {
                let package = string(&input, "package").map_err(|e| e.to_string())?;
                self.npm
                    .info(&package, sandbox.roots().first().map(|r| r.as_path()))
                    .await
            }
            "list_dir" | "read_file" | "search_project" => {
                let name = name.to_string();
                // File work is blocking; keep it off the async threads.
                tokio::task::spawn_blocking(move || run_file_tool(&sandbox, &name, &input))
                    .await
                    .map_err(|e| e.to_string())?
            }
            other => Err(ToolError::BadInput(format!(
                "there is no tool named {other}"
            ))),
        };
        result.map_err(|e| e.to_string())
    }
}

fn run_file_tool(sandbox: &Sandbox, name: &str, input: &Value) -> Result<String, ToolError> {
    let number = |key: &str| input[key].as_u64().map(|n| n as usize);
    match name {
        "list_dir" => files::list_dir(
            sandbox,
            input["path"].as_str().unwrap_or("."),
            number("depth").unwrap_or(2),
        ),
        "read_file" => files::read_file(
            sandbox,
            &string(input, "path")?,
            number("start_line"),
            number("end_line"),
        ),
        "search_project" => files::search_project(
            sandbox,
            &string(input, "query")?,
            input["regex"].as_bool().unwrap_or(false),
            input["glob"].as_str(),
        ),
        _ => unreachable!("dispatched by run"),
    }
}

/// Language keywords: a search for them finds everything and says nothing.
const KEYWORDS: &[&str] = &[
    "and",
    "async",
    "await",
    "bool",
    "boolean",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "def",
    "default",
    "elif",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "final",
    "finally",
    "for",
    "from",
    "func",
    "function",
    "impl",
    "implements",
    "import",
    "int",
    "interface",
    "let",
    "match",
    "mod",
    "new",
    "nil",
    "none",
    "not",
    "null",
    "number",
    "private",
    "protected",
    "pub",
    "public",
    "return",
    "self",
    "static",
    "string",
    "struct",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "undefined",
    "use",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// Whether the word under the pointer is worth a project search: a name,
/// not a keyword, a number or a stray character.
fn worth_searching(word: &str) -> bool {
    let length = word.chars().count();
    (3..=100).contains(&length)
        && word.chars().any(char::is_alphabetic)
        && !KEYWORDS.contains(&word.to_lowercase().as_str())
}

fn string(input: &Value, key: &str) -> Result<String, ToolError> {
    input[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| ToolError::BadInput(format!("“{key}” is required")))
}

fn spec(name: &str, description: &str, input_schema: Value) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: description.into(),
        input_schema,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::{Pointer, Workspace};
    use crate::context::{Capture, Classifier};
    use crate::platform::{Inspection, Point, Rect, Screenshots};

    fn pointing_at(root: &std::path::Path, word: &str) -> ContextPack {
        let mut pack = ContextPack::build(
            Capture {
                cursor: Point::default(),
                lens: Rect::centered_at(Point::default(), 10.0, 10.0),
                window: None,
                inspection: Inspection::default(),
                focus_level: 0,
                screenshots: Screenshots::default(),
            },
            &Classifier::builtin(),
        );
        pack.workspace = Some(Workspace {
            roots: vec![root.to_path_buf()],
            active_file: None,
            visible_ranges: vec![],
            visible_text: String::new(),
            selections: vec![],
            open_files: vec![],
            pointer: Some(Pointer {
                file: root.join("package.json"),
                line: 0,
                word: word.into(),
                line_text: String::new(),
            }),
            uri_scheme: "vscode".into(),
        });
        pack
    }

    #[tokio::test]
    async fn the_word_under_the_pointer_is_looked_up_first() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.js"),
            "import express from 'express'\n",
        )
        .unwrap();
        let tools = tools(dir.path());

        let lookups = tools.lookups(&pointing_at(dir.path(), "express")).await;
        assert_eq!(lookups.len(), 1);
        assert_eq!(lookups[0].record, "Searched for “express”");
        assert!(
            lookups[0]
                .text
                .contains("index.js:1: import express from 'express'")
        );

        assert!(
            tools
                .lookups(&pointing_at(dir.path(), "const"))
                .await
                .is_empty()
        );
        assert!(tools.lookups(&pointing_at(dir.path(), "")).await.is_empty());
    }

    #[test]
    fn keywords_numbers_and_stray_characters_are_not_searched() {
        for yes in ["express", "useState", "zod", "@types/node", "MAX_RETRY_MS"] {
            assert!(worth_searching(yes), "{yes}");
        }
        for no in [
            "", "{", "a", "id", "42.5", "const", "Return", "true", "function",
        ] {
            assert!(!worth_searching(no), "{no}");
        }
    }

    fn tools(root: &std::path::Path) -> ProjectTools {
        ProjectTools {
            sandbox: Arc::new(Sandbox::new(&[root.to_path_buf()])),
            npm: Arc::new(NpmClient::new()),
        }
    }

    #[tokio::test]
    async fn tools_run_through_the_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.js"), "require('express')\n").unwrap();
        std::fs::write(dir.path().join(".env"), "KEY=1\n").unwrap();
        let tools = tools(dir.path());

        let read = tools
            .run("read_file", &json!({ "path": "index.js" }))
            .await
            .unwrap();
        assert!(read.contains("require('express')"));
        let secret = tools
            .run("read_file", &json!({ "path": ".env" }))
            .await
            .unwrap_err();
        assert!(secret.contains("secrets"));
        let invalid = tools.run("read_file", &Value::Null).await.unwrap_err();
        assert!(
            invalid.contains("required"),
            "bad or truncated input is refused: {invalid}"
        );
    }

    #[test]
    fn every_tool_has_a_schema_and_a_status() {
        let dir = tempfile::tempdir().unwrap();
        let tools = tools(dir.path());
        let specs = tools.specs();
        assert_eq!(specs.len(), 4);
        for spec in specs {
            assert_eq!(spec.input_schema["type"], "object", "{}", spec.name);
            let note = tools.describe(
                &spec.name,
                &json!({ "path": "src", "query": "x", "package": "y" }),
            );
            assert!(note.status.ends_with('…') && !note.record.is_empty());
        }
        assert_eq!(
            tools
                .describe("read_file", &json!({ "path": "package.json" }))
                .status,
            "Reading package.json…"
        );
    }
}
