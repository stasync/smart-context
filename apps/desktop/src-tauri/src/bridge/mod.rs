//! The local WebSocket server for the VS Code extension (docs/PLAN.md 8), and
//! the workspace state it reports.
//!
//! The server listens on 127.0.0.1 on a random port and writes the port and
//! a per-launch token to `bridge.json` (mode 0600). Each VS Code window
//! connects, proves it read the file by sending the token, then pushes its
//! editor state whenever it changes.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

/// The extension must say hello this soon, or it's dropped.
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// Editor state messages are small; anything bigger is refused.
const MAX_MESSAGE: usize = 1 << 20;
/// Caps on what a hover reports (the extension caps them too).
const MAX_WORD_CHARS: usize = 200;
const MAX_LINE_CHARS: usize = 500;
pub const BRIDGE_FILE: &str = "bridge.json";

/// What one VS Code window reports.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct EditorState {
    focused: bool,
    workspace_folders: Vec<PathBuf>,
    active_file: Option<PathBuf>,
    visible_ranges: Vec<LineRange>,
    visible_text: String,
    selections: Vec<Selection>,
    open_files: Vec<PathBuf>,
}

/// Lines, zero-based, end inclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

/// What the mouse rests on in the editor, from the editor's last hover.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pointer {
    pub file: PathBuf,
    /// Zero-based.
    pub line: u32,
    /// The word under the mouse, if it's over one.
    pub word: String,
    pub line_text: String,
}

/// A hover as the extension reports it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Hover {
    file: PathBuf,
    line: u32,
    #[serde(default)]
    word: String,
    #[serde(default)]
    line_text: String,
    /// When the editor asked for the hover, in ms since the Unix epoch.
    at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
}

/// The project the user is pointing into, as the editor sees it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub roots: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
    pub visible_ranges: Vec<LineRange>,
    /// The visible lines of the active file (capped by the extension).
    pub visible_text: String,
    pub selections: Vec<Selection>,
    pub open_files: Vec<PathBuf>,
    /// What the mouse rests on, when the editor's last hover is still current.
    #[serde(default)]
    pub pointer: Option<Pointer>,
    /// The editor's URI scheme (`vscode`, `vscode-insiders`, `cursor`), for
    /// links that open files in it.
    pub uri_scheme: String,
}

impl Workspace {
    /// Drops the open file's text when it's on the secret deny-list
    /// (docs/PLAN.md 7): what the tools can't read, the editor mustn't leak.
    pub fn redact_secrets(mut self) -> Self {
        if self.active_file.as_ref().is_some_and(|f| self.is_secret(f)) {
            self.visible_text.clear();
            self.visible_ranges.clear();
            self.selections.clear();
        }
        if self
            .pointer
            .as_ref()
            .is_some_and(|p| self.is_secret(&p.file))
        {
            self.pointer = None;
        }
        self
    }

    fn is_secret(&self, file: &Path) -> bool {
        let relative = self
            .roots
            .iter()
            .find_map(|root| file.strip_prefix(root).ok())
            .unwrap_or(file);
        crate::tools::sandbox::is_secret(relative)
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Incoming {
    #[serde(rename_all = "camelCase")]
    Hello {
        token: String,
        #[serde(default)]
        uri_scheme: Option<String>,
    },
    State(EditorState),
    /// The editor asked for a hover: the mouse came to rest on a word.
    /// `null` when what's under the mouse changed (scrolling, editing).
    Pointer {
        pointer: Option<Hover>,
    },
}

struct Client {
    uri_scheme: String,
    state: EditorState,
    hover: Option<Hover>,
    /// When this window last reported having focus.
    focused_at: Option<Instant>,
}

#[derive(Default)]
struct Clients {
    map: HashMap<u64, Client>,
}

pub struct Bridge {
    token: String,
    clients: Mutex<Clients>,
    next_id: AtomicU64,
}

impl Bridge {
    /// Starts listening and writes `bridge.json` into `dir`.
    pub async fn start(dir: &Path) -> io::Result<Arc<Self>> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let bridge = Arc::new(Self::new(new_token()));
        write_bridge_file(dir, port, &bridge.token)?;
        log::info!("bridge listening on 127.0.0.1:{port}");

        let server = bridge.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let bridge = server.clone();
                        tauri::async_runtime::spawn(async move { bridge.serve(stream).await });
                    }
                    Err(e) => log::warn!("bridge accept failed: {e}"),
                }
            }
        });
        Ok(bridge)
    }

    fn new(token: String) -> Self {
        Self {
            token,
            clients: Mutex::default(),
            next_id: AtomicU64::new(1),
        }
    }

    /// The workspace of the VS Code window the user is most likely in: the
    /// focused one, or else the one focused most recently. Its last hover is
    /// the pointer only if it came after the mouse stopped (`still_since`).
    pub fn workspace(&self, still_since: Option<SystemTime>) -> Option<Workspace> {
        let clients = self.clients();
        let client = clients
            .map
            .values()
            .filter(|c| !c.state.workspace_folders.is_empty())
            .max_by_key(|c| (c.state.focused, c.focused_at))?;
        let state = &client.state;
        Some(Workspace {
            roots: state.workspace_folders.clone(),
            active_file: state.active_file.clone(),
            visible_ranges: state.visible_ranges.clone(),
            visible_text: state.visible_text.clone(),
            selections: state.selections.clone(),
            open_files: state.open_files.clone(),
            pointer: client
                .hover
                .as_ref()
                .filter(|hover| still_since.is_some_and(|since| hover.time() >= since))
                .map(Hover::pointer),
            uri_scheme: client.uri_scheme.clone(),
        })
    }

    fn clients(&self) -> MutexGuard<'_, Clients> {
        self.clients.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn serve(self: Arc<Self>, stream: TcpStream) {
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE));
        let mut socket =
            match tokio_tungstenite::accept_async_with_config(stream, Some(config)).await {
                Ok(socket) => socket,
                Err(e) => {
                    log::debug!("bridge handshake failed: {e}");
                    return;
                }
            };

        // The first message must carry the token from bridge.json.
        let hello = tokio::time::timeout(HELLO_TIMEOUT, socket.next()).await;
        let uri_scheme = match hello {
            Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str(&text) {
                Ok(Incoming::Hello { token, uri_scheme })
                    if constant_time_eq(&token, &self.token) =>
                {
                    uri_scheme.unwrap_or_else(|| "vscode".into())
                }
                _ => {
                    log::warn!("bridge: a client failed the token check");
                    return;
                }
            },
            _ => return,
        };

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.clients().map.insert(
            id,
            Client {
                uri_scheme,
                state: EditorState::default(),
                hover: None,
                focused_at: None,
            },
        );
        log::info!("bridge: editor window {id} connected");

        while let Some(Ok(message)) = socket.next().await {
            let Message::Text(text) = message else {
                continue;
            };
            match serde_json::from_str(&text) {
                Ok(Incoming::State(state)) => self.update(id, state),
                Ok(Incoming::Pointer { pointer }) => {
                    if let Some(client) = self.clients().map.get_mut(&id) {
                        client.hover = pointer;
                    }
                }
                _ => {}
            }
        }
        self.clients().map.remove(&id);
        log::info!("bridge: editor window {id} disconnected");
    }

    fn update(&self, id: u64, state: EditorState) {
        let mut clients = self.clients();
        if let Some(client) = clients.map.get_mut(&id) {
            if state.focused {
                client.focused_at = Some(Instant::now());
            }
            client.state = state;
        }
    }
}

impl Hover {
    fn time(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(self.at)
    }

    fn pointer(&self) -> Pointer {
        Pointer {
            file: self.file.clone(),
            line: self.line,
            word: self.word.chars().take(MAX_WORD_CHARS).collect(),
            line_text: self.line_text.chars().take(MAX_LINE_CHARS).collect(),
        }
    }
}

/// 256 random bits, hex-encoded.
fn new_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Writes `{ "port": …, "token": … }`, readable only by the user.
fn write_bridge_file(dir: &Path, port: u16, token: &str) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join(BRIDGE_FILE);
    let temp = path.with_extension("json.tmp");
    let contents = serde_json::to_vec(&serde_json::json!({ "port": port, "token": token }))?;
    write_private(&temp, &contents)?;
    fs::rename(temp, path)
}

#[cfg(unix)]
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(contents)
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    // The per-user app data folder is already private on Windows.
    fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use futures_util::SinkExt;
    use tokio_tungstenite::connect_async;

    use super::*;

    async fn start(dir: &Path) -> (Arc<Bridge>, u16, String) {
        let bridge = Bridge::start(dir).await.unwrap();
        let file: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(BRIDGE_FILE)).unwrap()).unwrap();
        let port = file["port"].as_u64().unwrap() as u16;
        let token = file["token"].as_str().unwrap().to_string();
        (bridge, port, token)
    }

    fn state(root: &str, focused: bool) -> String {
        serde_json::json!({
            "type": "state",
            "focused": focused,
            "workspaceFolders": [root],
            "activeFile": format!("{root}/package.json"),
            "visibleRanges": [{ "start": 0, "end": 20 }],
            "visibleText": "{\n  \"dependencies\": { \"express\": \"^5.1.0\" }\n}",
            "selections": [],
            "openFiles": [],
        })
        .to_string()
    }

    async fn settle() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_bridge_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        start(dir.path()).await;
        let mode = fs::metadata(dir.path().join(BRIDGE_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[tokio::test]
    async fn an_editor_with_the_token_reports_its_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let (bridge, port, token) = start(dir.path()).await;
        let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .unwrap();
        let hello = serde_json::json!({ "type": "hello", "token": token, "uriScheme": "vscode" });
        socket.send(Message::text(hello.to_string())).await.unwrap();
        socket
            .send(Message::text(state("/work/shop", true)))
            .await
            .unwrap();
        settle().await;

        let workspace = bridge.workspace(None).unwrap();
        assert_eq!(workspace.roots, [PathBuf::from("/work/shop")]);
        assert_eq!(workspace.uri_scheme, "vscode");
        assert!(workspace.visible_text.contains("express"));

        drop(socket);
        settle().await;
        assert!(
            bridge.workspace(None).is_none(),
            "gone once the window disconnects"
        );
    }

    #[tokio::test]
    async fn the_last_hover_counts_only_if_the_mouse_has_not_moved_since() {
        let dir = tempfile::tempdir().unwrap();
        let (bridge, port, token) = start(dir.path()).await;
        let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .unwrap();
        let hello = serde_json::json!({ "type": "hello", "token": token });
        socket.send(Message::text(hello.to_string())).await.unwrap();
        socket
            .send(Message::text(state("/work/shop", true)))
            .await
            .unwrap();
        let hovered_at = SystemTime::now();
        let millis = hovered_at.duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        let hover = serde_json::json!({
            "type": "pointer",
            "pointer": {
                "file": "/work/shop/package.json",
                "line": 1,
                "character": 20,
                "word": "express",
                "lineText": "  \"dependencies\": { \"express\": \"^5.1.0\" }",
                "at": millis,
            },
        });
        socket.send(Message::text(hover.to_string())).await.unwrap();
        settle().await;

        let still_since =
            |ms: i64| Some(UNIX_EPOCH + Duration::from_millis((millis as i64 + ms) as u64));
        let pointer = bridge.workspace(still_since(-500)).unwrap().pointer;
        assert_eq!(
            pointer.map(|p| (p.line, p.word)),
            Some((1, "express".into())),
            "the mouse stopped, then the editor hovered"
        );
        assert_eq!(
            bridge.workspace(still_since(200)).unwrap().pointer,
            None,
            "the mouse moved after the hover"
        );
        assert_eq!(bridge.workspace(None).unwrap().pointer, None, "unknown");

        let cleared = serde_json::json!({ "type": "pointer", "pointer": null });
        socket
            .send(Message::text(cleared.to_string()))
            .await
            .unwrap();
        settle().await;
        assert_eq!(bridge.workspace(still_since(-500)).unwrap().pointer, None);
    }

    #[tokio::test]
    async fn a_wrong_token_gets_nothing_in() {
        let dir = tempfile::tempdir().unwrap();
        let (bridge, port, _) = start(dir.path()).await;
        let (mut socket, _) = connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .unwrap();
        let hello = serde_json::json!({ "type": "hello", "token": "guess" });
        socket.send(Message::text(hello.to_string())).await.unwrap();
        let _ = socket.send(Message::text(state("/work/evil", true))).await;
        settle().await;
        assert!(bridge.workspace(None).is_none());
    }

    #[tokio::test]
    async fn the_focused_window_wins() {
        let dir = tempfile::tempdir().unwrap();
        let (bridge, port, token) = start(dir.path()).await;
        let hello = serde_json::json!({ "type": "hello", "token": token }).to_string();
        let (mut a, _) = connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .unwrap();
        let (mut b, _) = connect_async(format!("ws://127.0.0.1:{port}"))
            .await
            .unwrap();
        a.send(Message::text(hello.clone())).await.unwrap();
        b.send(Message::text(hello)).await.unwrap();

        a.send(Message::text(state("/work/a", true))).await.unwrap();
        b.send(Message::text(state("/work/b", false)))
            .await
            .unwrap();
        settle().await;
        assert_eq!(
            bridge.workspace(None).unwrap().roots,
            [PathBuf::from("/work/a")]
        );

        a.send(Message::text(state("/work/a", false)))
            .await
            .unwrap();
        b.send(Message::text(state("/work/b", true))).await.unwrap();
        settle().await;
        assert_eq!(
            bridge.workspace(None).unwrap().roots,
            [PathBuf::from("/work/b")]
        );
    }

    #[test]
    fn an_open_secret_file_is_never_passed_on() {
        let workspace = |file: &str| Workspace {
            roots: vec!["/work/shop".into()],
            active_file: Some(PathBuf::from(file)),
            visible_ranges: vec![LineRange { start: 0, end: 1 }],
            visible_text: "STRIPE_KEY=sk_live_123".into(),
            selections: vec![Selection {
                start_line: 0,
                end_line: 0,
                text: "sk_live_123".into(),
            }],
            open_files: vec![],
            pointer: Some(Pointer {
                file: PathBuf::from(file),
                line: 0,
                word: "STRIPE_KEY".into(),
                line_text: "STRIPE_KEY=sk_live_123".into(),
            }),
            uri_scheme: "vscode".into(),
        };
        let redacted = workspace("/work/shop/.env").redact_secrets();
        assert!(redacted.visible_text.is_empty() && redacted.selections.is_empty());
        assert_eq!(redacted.pointer, None);
        assert_eq!(redacted.active_file, Some(PathBuf::from("/work/shop/.env")));
        let kept = workspace("/work/shop/src/server.ts").redact_secrets();
        assert!(!kept.visible_text.is_empty() && kept.pointer.is_some());
    }

    #[test]
    fn tokens_compare_exactly() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert_eq!(new_token().len(), 64);
    }
}
