# Context: Product and Build Plan

Handoff for the coding agent. Written 26 Sep 2026 by the project owner (Stanislav) together with Claude. It captures every decision made during planning. Read it in full before writing any code.

## 0. How to use this document

- This file is the source of truth for scope and architecture. If the code and this file disagree, ask the owner, then update this file.
- Build milestone by milestone (section 11). Don't start the next milestone until the current one's acceptance criteria pass. Tick the checkboxes as you go.
- Items marked **Verify** depend on things that change over time: model IDs, crate versions, API parameter names, tool type strings, bundle IDs. Check the official docs before using them. Never trust versions or IDs from memory.
- Open decisions (section 14) belong to the owner. Use the stated default, and say you did.
- All numbers marked "starting value" are meant to be tuned with the eval set (section 13).
- Changes made since the original Google Doc are listed in section 16.

---

## 1. What Context is

Context is an open-source desktop app that explains whatever you point at, in the context of where you are. You hold a hotkey, aim a lens at something on screen, and release. A small popover next to the cursor explains what it is and why it's there.

The default question is always "what is this, and why is it here?" Pointing is enough; typing is optional.

### Examples that define success

| You point at | Context answers |
| --- | --- |
| "express" in `package.json`, in VS Code | What Express is and why this project uses it, found by reading the project: "Express is a Node.js web server framework. Here it serves the REST API from `src/server.ts`, with routes in `src/routes/`." File names are clickable. |
| A product photo on Amazon | What the product is and what it's for, in plain words. Go deeper adds key specs, what reviewers complain about, and alternatives. |
| A ticket in Jira or Linear | What the ticket asks, with the jargon explained. |
| Anything else: a term in a PDF, a UI element, a chart, a diagram | A short explanation in the context of that app or page. |

### Goals

- Portfolio project, open source. Code quality, clear architecture and a polished demo matter more than feature count. Finish a smaller scope well.
- Pure client. No backend and no shared API key. Users bring their own AI access.
- macOS first, Windows next. Only the native layer is per-OS.
- Vendor-neutral AI layer from day one. Switching to another AI vendor must mean writing one adapter, nothing else.

### Competitive context (why it's worth building)

- Tools that screenshot around the cursor already exist: AIPointer (bring-your-own key, source withdrawn, screenshot only), HeyClicky (paid, routes everything through its own backend), Jarvis and Highlight AI. Apple's Siri AI (macOS 27) and Google's AI pointer / Gemini in Chrome are adding on-screen awareness to the OS and the browser.
- They all reason from pixels. Context's edge is depth: it reads the real source behind the screen (accessibility text, the project's files, package registries, the page URL) and explains *why* something is there, not just what it is. It's also open source with no middleman.

---

## 2. Final decisions

| Topic | Decision | Reason |
| --- | --- | --- |
| Stack | Tauri 2: Rust core + React/TypeScript UI | Hooks, screen capture and accessibility need native code either way. Rust is the backend in Tauri. Small binaries. Strong portfolio signal. Electron was rejected: it still needs native modules and is much bigger. |
| Platforms | macOS first (MVP), Windows as the next milestone | The owner develops on a MacBook. UI, AI and extension code are shared. |
| MVP scope | Point-and-explain in any app, plus project reading in VS Code | The express example is the showcase. |
| Answer size | Short (2–4 sentences) by default; Go deeper for more | Fast and cheap. |
| AI access | No backend, no shipped key. Two engines in the MVP: the user's Claude API key, or the user's own Claude Code install | Anthropic does not allow third-party "Sign in with Claude" (see 6.1). |
| Vendors | Vendor-neutral Engine layer in the MVP. OpenAI, Gemini, OpenRouter and Ollama come later as adapters. | Swapping vendors must not touch the rest of the app. |
| Effort | Users pick Low / Medium / High / Max, never a model. Every question starts at Low. | Save credits. |
| Keys | Pasted at runtime in Settings, stored in the OS keychain, used only by the Rust core | Keys never live in source code, config files, builds, logs or the web UI. |
| Distribution | Open source; prebuilt releases built by CI from tagged source | Users don't need to compile, and can check the download matches the code. |
| Package managers | npm workspaces for the UI and the extension; a Cargo workspace for Rust | npm ships with Node, so contributors need nothing extra. |

---

## 3. User experience (MVP)

### 3.1 Pointing

1. Hold the hotkey (default: Right Option ⌥, configurable; see section 14). A translucent lens appears around the cursor and highlights the accessibility element under it.
2. Aim.
   - Moving the mouse retargets.
   - Scroll resizes the lens. Scroll events are swallowed while the lens is up, so the app underneath doesn't scroll.
   - Shift+scroll steps between the element and its parent or child (word → line → block → panel), like a devtools element picker. The lens snaps to that element's bounds.
3. Release to ask the default question. The popover opens next to the cursor and streams the answer.
4. Press Space while holding to switch to ask mode. The lens freezes and the popover opens with a focused text box and no automatic answer. The typed question is asked with the same context.
5. Esc cancels at any point.
6. In an excluded app (password managers, etc.), the lens shows "Context is off here" and nothing is captured.
7. Accidental triggers: pressing any other key while the hotkey is held, or a very short tap, must not ask a question. The exact rule is an open decision (section 14).

### 3.2 Answer popover

- First line: "You pointed at: …", taken from the model's `TARGET:` line (section 4.6). Clicking it lets the user correct the target in free text, which re-asks.
- Answer: 2–4 sentences at Low effort, streamed, rendered as markdown.
  - File paths are clickable and open in VS Code (`vscode://file/<absolute path>:<line>`).
  - Links open in the default browser.
- While working, a status line shows tool activity: "Reading package.json…", "Searching for express…", "Searching the web…".
- Buttons:
  - **Go deeper**: one effort level up.
  - **Copy**.
  - **What was sent**: the crop, the text context and the list of files read.
  - An effort indicator that can also be changed manually.
- Follow-up box at the bottom keeps the same conversation and effort level.
- Closes on Esc or a click outside. It doesn't take keyboard focus until the user clicks into the box (except in ask mode).

### 3.3 Setup, settings and menu bar

- First run:
  1. Explain why Accessibility and Screen Recording permissions are needed.
  2. Open the right System Settings panes.
  3. Re-check status until both are granted.
- Engine choice:
  - Claude API key: a paste field. The key is stored in Keychain. Show a link and a tip: "Create a dedicated key with a spend limit in the Claude Console."
    - The paste field is write-only: the webview sends the key to Rust once, and Rust never sends it (or any part of it) back. After saving, the UI shows only "Key saved" and a Remove button.
  - Claude Code: detect the binary and its sign-in status (section 6.5). No key needed.
- Other settings:
  - Effort ceiling (default High; Max is only used when picked manually).
  - Hotkey.
  - Excluded apps. The default list (password managers) is in the `settings` module; editing it comes with the settings store.
  - Answer language (default: the system language).
  - Dev mode, which auto-saves context packs for replay.
- Menu bar (tray) icon: enable/disable, settings, quit. No Dock icon.
- A crowded menu bar hides the icon behind the notch, so opening Context again (Finder, Spotlight, `open -a Context`) also shows Settings. Dev builds open Settings at launch, and its Development section opens the capture viewer.

---

## 4. How the context is figured out

The core idea: send the model what the user pointed at, where it sits, and what's around it. Use real text wherever possible (accessibility, editor text, URL). Let the model decide whether to dig deeper with tools.

### 4.1 Pipeline

1. **Key down:** show the lens. In the background, start capturing the window screenshot and resolving the app, window and URL, to hide latency.
2. **While aiming:** query the accessibility element under the cursor, throttled, and highlight its bounds.
3. **Release (or Space):** take the final lens crop and the element, its parents and nearby text, plus the selection. In VS Code, also take the latest state from the extension.
4. Build the ContextPack (4.2). Draw the lens rectangle and a cursor marker onto the window screenshot, so the model sees where the user pointed within the whole window.
5. Classify the source (4.5) into a source hint that steers the answer.
6. Call the engine at Low effort, streaming. The model writes `TARGET: …` first, then the answer. It may call tools: project tools in code mode, npm lookup, web search.
7. **Go deeper** re-asks one effort level up, reusing the same pack and conversation, with a longer answer and a bigger tool budget.

### 4.2 ContextPack

Sketch only; adjust names as needed.

```rust
pub struct ContextPack {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub cursor: Point,               // global screen coords, points
    pub lens: Rect,                  // global screen coords, points
    pub display_scale: f64,
    pub window: Option<WindowInfo>,  // title, bounds, and its app (name, bundle_id, pid)
    pub url: Option<String>,         // browsers only
    pub selection: Option<String>,   // AXSelectedText, capped
    pub focus: ElementInfo,          // element under cursor (role, subrole, title, value, description, bounds)
    pub ancestors: Vec<ElementInfo>, // up to 5 levels
    pub nearby_text: String,         // text of elements intersecting the lens, capped (starting value 4,000 chars)
    pub lens_image: ImageData,       // crop at native resolution, long edge ≤ 1024 px (starting value)
    pub window_image: ImageData,     // downscaled, lens outlined + cursor marker
    pub source: SourceHint,          // CodeEditor | Shopping | WorkTool | WebPage | OtherApp
    pub workspace: Option<Workspace>,// M4: from the VS Code bridge: root(s), active file, visible text, selections
}
```

The pack is serializable (JSON + image files) so it can be saved and replayed (section 13).

### 4.3 Accessibility on macOS

- Get the element at the cursor with `AXUIElementCreateSystemWide` + `AXUIElementCopyElementAtPosition`.
- Read these attributes: `AXRole`, `AXSubrole`, `AXRoleDescription`, `AXTitle`, `AXValue` (truncated), `AXDescription`, `AXHelp`, `AXPosition`, `AXSize`, `AXSelectedText`.
- Walk `AXParent` up to 5 levels.
- Nearby text: hit-test a grid of points inside the lens (a row about every 12 pt, 4 columns, nearest the cursor first), then join the distinct elements' text in reading order. Bounded (starting values: 60 samples, 150 ms). Walking the tree down from an ancestor instead spent the whole budget on off-screen content in long documents (tested in VS Code, 28 Sep 2026).
- Browser URL: find the `AXWebArea` ancestor and read `AXURL`, for browsers only: other apps' web views (VS Code's, for one) carry internal URLs. The window title stays in the pack either way.
- Side effects: `AXManualAccessibility` makes VS Code think a screen reader is running (it offers "Screen Reader Optimized" mode). See section 14. Chrome builds its tree after `AXEnhancedUserInterface` is set, so the first capture in a freshly started Chrome can be shallow.
- Chromium and Electron apps (Chrome, VS Code, Slack) only expose a full tree once asked. Set `AXManualAccessibility = true` (Electron) or `AXEnhancedUserInterface = true` (Chrome) on the app element, once per process. **Verify** the side effects: `AXEnhancedUserInterface` is known to cause window-animation glitches in some apps, so prefer `AXManualAccessibility` where it works.
- Set `AXUIElementSetMessagingTimeout` (starting value 0.25 s) so a hung app can't block Context.
- Coordinates: AX uses global top-left points, while screenshots are in pixels. Handle Retina scale and multiple displays carefully.

### 4.4 Screenshots on macOS

- Use ScreenCaptureKit (`SCScreenshotManager`, macOS 14+). `CGWindowListCreateImage` is deprecated. Candidate crates: `screencapturekit`, `xcap` (**Verify**).
- Exclude Context's own lens and popover windows from capture (content filter).
- Lens crop: native resolution, long edge ≤ 1024 px (starting value).
- ScreenCaptureKit captures both images at their final size (`SCStreamConfiguration` width/height), scaling on the GPU. Resizing afterwards with the `image` crate took over 4 s per capture in dev builds, because its generic code compiles unoptimized into this crate.
- Window image:
  - Downscale to fit the smallest vision limit of the supported models: long edge ≤ 1568 px and ≤ 1.15 megapixels. Checked 28 Sep 2026 in Anthropic's vision docs: the standard tier (including the Haiku used for Low effort) downsizes past 1568 px or 1568 visual tokens of 28×28 px; Claude 4.7 and later allow 2576 px.
  - JPEG quality ~80.
  - Draw the lens rectangle (2 px, high-contrast) and a small cursor marker on it.
- Minimum macOS version: 14 (Sonoma).

### 4.5 Source classifier

The rules live in `config/sources.json`, so they're editable without code changes. Bundle IDs are **Verify**.

- **CodeEditor:**
  - Editors: VS Code (`com.microsoft.VSCode`), VS Code Insiders, Cursor, JetBrains IDEs, Xcode.
  - Terminals: Terminal (`com.apple.Terminal`), iTerm2 (`com.googlecode.iterm2`), Warp, Ghostty.
- **Browsers:** Safari (`com.apple.Safari`), Chrome (`com.google.Chrome`), Arc (`company.thebrowser.Browser`), Edge (`com.microsoft.edgemac`), Brave (`com.brave.Browser`), Firefox (`org.mozilla.firefox`). For browsers, classify by URL host:
  - **Shopping:** `amazon.*`, `ebay.*`, `aliexpress.com`, `etsy.com`, `walmart.com`, `noon.com`, …
  - **WorkTool:** `*.atlassian.net`, `linear.app`, `github.com`, `gitlab.com`, `notion.so`, …
  - Anything else → **WebPage**.
- Anything else → **OtherApp**.

### 4.6 Prompting

Output contract (every engine, every effort):

```
TARGET: <what the user pointed at and where, ≤ 12 words>
<answer in markdown>
```

The UI strips the `TARGET:` line into the popover header. If it's missing, the header shows "Explaining…" and the whole text is the answer.

System prompt rules (one shared prompt plus a short block per source hint):

- Answer the implicit question: what is this, and why is it here, in this specific context? Never give a generic dictionary answer when the context says more.
- Treat the lens as the focus and the window as the surroundings. Treat accessibility, editor and URL text as the source of truth for names and numbers, over what the pixels seem to say.
- Low effort: 2–4 sentences, no headings. Use tools only if the pack doesn't answer the question (tool budget in 6.3).
- Medium and above: short sections are allowed. Cite the files and links used.
- If unsure what the target is, say what it most likely is in the `TARGET` line and answer that. The user can correct it.
- Answer in the configured language.

Per-source guidance:

| Source | Guidance |
| --- | --- |
| CodeEditor | Explain what it is, why it's in this project, and where it's used. Prefer reading the manifest, the README and searching for usages. For packages, use `npm_info` for the description. |
| Shopping | What the product is and what it's for, in plain words. At Medium and above: key specs, common complaints from reviews, alternatives, using web search. |
| WorkTool | What the item asks or means, with jargon explained. Relate it to what's visible. |
| WebPage / OtherApp | Explain the term, element, chart or diagram as it's used here. |

Prompt caching: the static system prompt, the tool definitions and the per-project summary (4.7) form the cached prefix. The Claude API adapter caches the whole prompt, pack included, so follow-ups and Go deeper at the same level reuse it. Checked 28 Sep 2026: the minimum cacheable prompt is 4096 tokens on Haiku 4.5 and 512–1024 on Sonnet 5 and Opus 5. A first answer with both images is about 4.7k tokens, so Low answers do cache.

As built: the first message is the lens image, then the window image (each introduced by one line of text), then the text read from the screen, then the question. Go deeper appends a "Go deeper." message; follow-ups append the user's text; correcting the target starts a new conversation with the correction. History is append-only, as the newer models bind their reasoning blocks to the conversation that produced them.

### 4.7 Project summary cache (code mode)

- On the first code-mode question in a workspace, generate a short summary at Low effort: what the project is, its main stack, its entry points. Base it on the README and manifest files (`package.json`, `Cargo.toml`, `pyproject.toml`, …).
- Store it in app data, keyed by a hash of the workspace root path. Invalidate it when a manifest's modified time changes.
- Include it in the system prompt for later questions, so they skip the reading step.

---

## 5. Architecture

### 5.1 Overview

```
┌──────────────────────────── Context.app (Tauri 2) ─────────────────────────────┐
│  React + TS UI (webview): lens overlay · answer popover · settings/onboarding  │
│          ▲ events: text deltas, tool status, errors      │ commands            │
│          │                                               ▼                     │
│  Rust core                                                                     │
│   input (hotkey, scroll, keys) ─► capture (screenshots) ─► ax (accessibility)  │
│                          └────────────► context (pack builder, classifier)     │
│                                              │                                 │
│                                              ▼                                 │
│                                        orchestrator ──► engines (Engine trait) │
│                                         │      ▲          ├─ anthropic_api ────┼─► api.anthropic.com
│                                         ▼      │          └─ claude_code ──────┼─► user's own `claude` CLI
│                                        tools (local, read-only)                │
│                                         ▲                                      │
│                                        bridge (WebSocket on 127.0.0.1) ◄───────┼── VS Code extension
│   secrets (Keychain) · settings · replay (save/load packs)                     │
└────────────────────────────────────────────────────────────────────────────────┘
   tools also call: registry.npmjs.org (npm_info)
```

Nothing else leaves the machine. There's no Context server and no telemetry.

### 5.2 Repository layout

The repository is `github.com/stasync/smart-context`; its root plays the role of `context/` below.

```
context/
├── apps/desktop/                 # Tauri 2 app
│   ├── src/                      # React + TypeScript UI
│   │   ├── lens/                 # lens overlay window
│   │   ├── popover/              # answer popover window
│   │   ├── settings/             # settings + onboarding window
│   │   └── shared/               # IPC types, markdown renderer, hooks
│   └── src-tauri/
│       ├── src/
│       │   ├── main.rs / lib.rs  # app setup, windows, tray, commands
│       │   ├── platform/         # traits + per-OS impls (see 5.4)
│       │   │   ├── mod.rs
│       │   │   ├── macos/        # event tap, AX, ScreenCaptureKit, panels
│       │   │   └── windows/      # stubs in MVP (compile, return NotSupported)
│       │   ├── pointing/         # hold-to-point gesture, lens geometry, the pointing session
│       │   ├── popover/          # the answer popover: placement, show/hide, event stream
│       │   ├── context/          # ContextPack, builder, classifier, image annotation
│       │   ├── orchestrator/     # conversation state, effort, tool loop, events to UI
│       │   ├── engines/          # Engine trait, normalized types, adapters
│       │   │   ├── anthropic_api.rs
│       │   │   └── claude_code.rs
│       │   ├── tools/            # list_dir, read_file, search_project, npm_info, sandbox
│       │   ├── bridge/           # WebSocket server for the VS Code extension
│       │   ├── secrets/          # keychain wrapper
│       │   ├── settings/         # settings store
│       │   └── replay/           # save/load context packs
│       ├── Cargo.toml
│       └── tauri.conf.json
├── extensions/vscode/            # VS Code extension (TypeScript)
├── config/
│   ├── models.json               # effort → model mapping per engine
│   └── sources.json              # classifier rules
├── eval/                         # saved packs + expectations + runner
├── docs/PLAN.md                  # this file
├── CLAUDE.md                     # working rules for coding agents
├── Cargo.toml                    # Cargo workspace
└── package.json                  # npm workspaces
```

The root is a Cargo workspace from M0, so the eval runner can join it as `crates/context-eval` in M6.

### 5.3 Module responsibilities

| Module | Owns |
| --- | --- |
| `platform` | Everything OS-specific: global input hooks, accessibility queries, screen capture, overlay window behavior, opening System Settings panes |
| `pointing` | The hold-to-point gesture (a pure state machine), lens geometry and Shift+scroll stepping, and the session: a lens thread that drives the overlay, and an inspector thread that does all accessibility and screenshot work, one job at a time |
| `context` | Turning a capture into a ContextPack: text caps, image scaling and annotation, source classification |
| `orchestrator` | One conversation per popover: effort level, message history, tool loop, budgets, streaming events to the UI, cancellation (Esc closes → cancel request). Local tools come in through a `Toolbox` trait (M4); the prompts live in `orchestrator/prompts.rs` |
| `popover` | The answer popover window: placing it beside the lens and on screen, showing and hiding it, and forwarding the orchestrator's `AnswerEvent`s to it |
| `engines` | The vendor-neutral Engine trait and its adapters. No UI or platform code. |
| `tools` | Local, read-only tools and the path sandbox |
| `bridge` | The local WebSocket server and workspace state from the VS Code extension |
| `secrets` | Storing and reading API keys in the OS keychain (candidate crate: `keyring`, **Verify**) |
| `settings` | User settings as JSON in the app config dir (candidate: `tauri-plugin-store`, **Verify**) |
| `replay` | Saving and loading packs for the eval set |

### 5.4 Platform abstraction

Keep all OS code behind traits, with `#[cfg(target_os = "...")]` implementations. The Windows implementations are stubs in the MVP, but the crate must still compile on Windows (checked in CI).

```rust
pub trait InputHooks {        // hold-hotkey, scroll (swallowable), Space, Esc, mouse move
    fn start_input(&self, handler: Arc<dyn InputHandler>) -> Result<()>;
}

pub trait InputHandler {      // runs on the input thread; its answer decides swallowing
    fn handle(&self, event: InputEvent) -> Disposition;   // Pass | Swallow
}

pub trait Screens {
    fn display_at(&self, p: Point) -> Option<Rect>;
    fn window_at(&self, p: Point) -> Option<WindowInfo>;  // app windows only, never Context's own
}

pub trait Overlay {           // the lens window: click-through, all Spaces, above full-screen apps
    fn configure_overlay(&self, window: &WebviewWindow) -> Result<()>;
    fn show_overlay(&self, window: &WebviewWindow, display: Rect) -> Result<()>;  // never takes focus
    fn hide_overlay(&self, window: &WebviewWindow) -> Result<()>;
}

pub trait Accessibility {     // one call per hit test or question, so no element handles cross threads
    fn inspect(&self, app: &AppInfo, p: Point, options: &InspectOptions) -> Result<Inspection>;
    // Inspection: element chain (leaf first), nearby text, selection, URL
}

pub trait ScreenCapture {     // both images at once, already scaled to their limits
    fn screenshots(&self, window: Option<&WindowInfo>, lens: Rect, limits: ShotLimits) -> Result<Screenshots>;
}

pub trait Permissions {
    fn permission_status(&self) -> PermissionStatus;   // accessibility, screen recording
    fn request_permission(&self, which: Permission);   // OS prompt + the System Settings pane
}
```

Each OS implements all the traits on one type, `platform::Native`. Swallowing is decided per event by the handler's return value, rather than by a `set_swallow_scroll` switch, so the gesture state machine alone decides what the app underneath sees.

macOS input: use a `CGEventTap` at session level on a dedicated thread with its own `CFRunLoop`.

- Listen for: `flagsChanged` (the modifier hotkey), `keyDown` (Space, Esc), `scrollWheel`, `mouseMoved`.
- Return `NULL` from the callback to swallow scroll and Space/Esc events while the lens is up.
- Re-enable the tap on `kCGEventTapDisabledByTimeout` or `ByUserInput`.
- This needs the Accessibility permission.

Threading:

- Use tokio for async work: HTTP, processes, the WebSocket server.
- Run accessibility calls on one worker thread, serialized.
- Send UI updates as Tauri events.

### 5.5 Windows (the Tauri kind)

- **lens:**
  - A non-activating `NSPanel` (via `tauri-nspanel` 2.1, checked 28 Sep 2026): a plain window from a background app never appears on another app's full-screen Space.
  - Transparent, no decorations, always on top, hidden from the Dock and app switcher.
  - Click-through (`set_ignore_cursor_events(true)`).
  - Covers the display under the cursor, visible on all Spaces and above full-screen apps (window collection behavior: can join all Spaces, full-screen auxiliary).
- **popover:**
  - Transparent, rounded, always on top, placed next to the cursor and kept on screen.
  - A non-activating panel that becomes key only when the user clicks its text box, or in ask mode. Candidate: `tauri-nspanel` (**Verify**).
- **settings:** a normal window.
- App setup:
  - Activation policy: Accessory (menu bar only).
  - Transparent windows on macOS may need `macOSPrivateApi: true` in `tauri.conf.json` (**Verify**).

---

## 6. AI engines

### 6.1 Rules (non-negotiable)

- No backend and no shipped key. Traffic goes only to the engine the user picked, plus `registry.npmjs.org` for `npm_info`.
- Never implement "Sign in with Claude", claude.ai OAuth, or reuse Claude Code's tokens. Anthropic prohibits third-party apps from offering Claude.ai login or routing requests through Free/Pro/Max credentials: <https://code.claude.com/docs/en/legal-and-compliance>.
- Claude Code engine:
  - Run the user's installed, unmodified `claude` binary.
  - Never read, copy or store its credentials. Sign-in happens only inside Claude Code.
  - Never pay for or resell usage.
  - The owner will confirm this use with Anthropic before the public launch.
- API keys live only in the keychain and in Rust memory. Never log them, never send them to the webview, never write them to disk in plain text. (The Settings paste field sends the key to Rust once; see 3.3.)

### 6.2 The Engine trait

```rust
#[async_trait]
pub trait Engine: Send + Sync {
    fn id(&self) -> EngineId;                      // "anthropic_api", "claude_code", later "openai", ...
    fn capabilities(&self) -> Capabilities;        // vision, tool_calling, native_web_search, runs_own_tools
    async fn check_ready(&self) -> Readiness;      // key present / CLI found + signed in / error text
    async fn ask(
        &self,
        req: AskRequest,
        events: mpsc::Sender<EngineEvent>,
        cancel: CancellationToken,
    ) -> Result<AskOutcome>;
}

pub struct AskRequest {
    pub effort: Effort,                  // Low | Medium | High | Max
    pub system: String,
    pub messages: Vec<Message>,          // normalized (below)
    pub tools: Vec<ToolSpec>,            // JSON-schema tools, run locally by the orchestrator
    pub web_search: bool,                // use the vendor's native search if it has one
    pub cwd: Option<PathBuf>,            // workspace root (used by claude_code)
    pub resume: Option<String>,          // vendor session id for follow-ups (claude_code)
}

pub enum EngineEvent {
    TextDelta(String),
    ToolStarted { name: String, summary: String },   // for the status line
    ToolFinished { name: String },
    Usage { input_tokens: u64, output_tokens: u64, cost_usd: Option<f64> },
    Error(EngineError),
}
```

Normalized messages: `Message { role: User | Assistant, content: Vec<Block> }`, where `Block` is one of `Text`, `Image { media_type, bytes }`, `ToolUse { id, name, input }` or `ToolResult { id, content, is_error }`. Each adapter converts to and from its vendor's format.

Tool loop:

- For engines that don't run their own tools, the orchestrator runs the loop. It executes `ToolUse` blocks locally, appends `ToolResult` blocks and calls again until the model finishes or the budget runs out.
- Claude Code runs its own tools, so its adapter only reports them for the status line.

### 6.3 Effort levels

Users choose an effort level, never a model.

- Every question starts at Low.
- Go deeper steps up one level, up to the ceiling (default High).
- Max is used only when the user picks it manually.

| Effort | Used for | Claude API | Claude Code | OpenAI (later) |
| --- | --- | --- | --- | --- |
| Low | Every first answer | Haiku | `--model haiku` | a mini model |
| Medium | First Go deeper | Sonnet | `--model sonnet --effort medium` | the standard model |
| High | Second Go deeper | Opus | `--model opus --effort high` | the flagship model |
| Max | Only when picked | Opus with extended thinking or the highest effort setting (**Verify** the API parameter) | `--model opus --effort max` | flagship, high reasoning effort |

The mapping lives in `config/models.json`, so new models need no code change. Put **Verify**-ed model IDs from Anthropic's models page into it, never hardcoded in Rust.

```json
{
  "anthropic_api": {
    "low":    { "model": "<current Haiku model ID>",  "max_tokens": 400,  "tool_budget": 3,  "web_search_max_uses": 1 },
    "medium": { "model": "<current Sonnet model ID>", "max_tokens": 1200, "tool_budget": 6,  "web_search_max_uses": 3 },
    "high":   { "model": "<current Opus model ID>",   "max_tokens": 2500, "tool_budget": 10, "web_search_max_uses": 5 },
    "max":    { "model": "<current Opus model ID>",   "max_tokens": 4000, "tool_budget": 15, "web_search_max_uses": 8, "thinking": true }
  },
  "claude_code": {
    "low":    { "model": "haiku",  "max_turns": 4 },
    "medium": { "model": "sonnet", "effort": "medium", "max_turns": 8 },
    "high":   { "model": "opus",   "effort": "high",   "max_turns": 12 },
    "max":    { "model": "opus",   "effort": "max",    "max_turns": 20 }
  }
}
```

All numbers above are starting values.

### 6.4 Anthropic API adapter (`anthropic_api`)

- Endpoint: the Messages API, `POST https://api.anthropic.com/v1/messages`, with `stream: true` (SSE). Headers: `x-api-key`, `anthropic-version`, `content-type` (**Verify** the version header value).
- HTTP: there is no official Rust SDK. `reqwest` (native TLS, so no C crypto library to build) plus a small SSE parser in `engines/sse.rs`; `eventsource-stream` was last released in 2022. `anthropic-version: 2023-06-01`, checked 28 Sep 2026.
- Images: base64 image content blocks, lens crop first, then the annotated window image.
- Tools:
  - Local tools (section 7) go in as custom tools with JSON Schema.
  - Web search uses Anthropic's server-side web search tool, with `max_uses` from the effort config. The type string depends on the model (`web_search_20260209` on Sonnet 5 and Opus 5, `web_search_20250305` on Haiku 4.5), so it's in `config/models.json`.
  - Refusals: Opus 5 can decline a request (HTTP 200, `stop_reason: "refusal"`). As Anthropic recommends, High and Max opt into server-side fallbacks (`fallbacks: "default"`, beta `server-side-fallback-2026-07-01`), which re-run a declined request on another model. After a fallback, the declined attempt's reasoning isn't sent back. See section 14.
- Prompt caching: a top-level `cache_control` caches the whole prompt.
- Blocks the orchestrator doesn't understand (reasoning, server-side tool results) come back as opaque blocks and are sent back unchanged in later turns.
- Retries: overloads and 5xx up to twice with backoff, rate limits when `retry-after` is under 20 s; only before any output arrives.
- The key comes from the keychain (`keyring` 4) per request, cached in memory. Settings' paste field sends it once; nothing returns it. Unsigned dev builds may make macOS ask for keychain access after a rebuild.
- Errors become friendly popover messages:
  - 401: invalid key. 403 or billing errors: no credits.
  - 429: rate limited; honor `retry-after`.
  - 529: overloaded; back off and retry.
  - Network errors: offline.
- Cancel: Esc or closing the popover drops the stream.

### 6.5 Claude Code adapter (`claude_code`)

Command (flags verified against the Claude Code docs on 26 Sep 2026; re-check with `claude --help`):

```sh
claude -p "<prompt with the context pack as text + image file paths>" \
  --output-format stream-json --verbose --include-partial-messages \
  --model <haiku|sonnet|opus> [--effort <medium|high|max>] \
  --allowedTools "Read,Grep,Glob,WebSearch,WebFetch" \
  --permission-mode dontAsk \
  --setting-sources user \
  --settings '{"disableAllHooks": true}' \
  --append-system-prompt "<Context system prompt>" \
  --add-dir "<temp dir holding the screenshots>" \
  --max-turns <from config>
```

- Working directory: the workspace root in code mode, otherwise the temp dir.
- Don't use `--bare`. Bare mode ignores the subscription sign-in, which is the whole point of this engine.
- Security: `--setting-sources user` and `disableAllHooks` stop a repository's own hooks, project settings and `.mcp.json` servers from running when Context points Claude Code at a folder. Source: "What runs before you trust a folder" in <https://code.claude.com/docs/en/permissions>.
- **Verify** in M5 (checked against `claude --help` for Claude Code 2.1.283 on 28 Sep 2026):
  - `--strict-mcp-config` exists ("only use MCP servers from `--mcp-config`"). Adding it would also stop the user's own MCP servers from starting for every question.
  - `--restricted` exists: it removes tools that run commands or code, ignores user, project and local settings files, and confines file tools to the working directories. It may replace `--setting-sources user` + `disableAllHooks`; check that it keeps the subscription sign-in and allows `WebFetch` via `--tools`.
  - `--max-turns` isn't listed in `--help`; confirm it still works in print mode.
  - `--effort` now accepts `low, medium, high, xhigh, max`.
- Screenshots: write them as files to a per-conversation temp dir. The prompt lists their paths, and the Read tool loads images. If `--input-format stream-json` accepts image content blocks (**Verify**), prefer that.
- Parsing the stream:
  - Text deltas come from `stream_event` lines whose delta type is `text_delta`.
  - `tool_use` blocks drive the status line.
  - The final `result` line carries `session_id` and cost.
- Follow-ups and Go deeper: `--resume <session_id>` with the new model and effort.
- Detection:
  - GUI apps don't inherit the shell `PATH`, so search the common locations: `~/.claude/local`, `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin` and the npm global bin. Also allow a manual path in Settings.
  - Check sign-in with `claude auth status` (exit code 0 means signed in).
- Latency: process startup adds time. Measure it, show "Starting Claude Code…", and don't over-engineer pre-warming in the MVP.
- Cleanup: delete the temp files when the conversation closes.

### 6.6 Adding a vendor later (checklist)

1. Add an adapter that implements `Engine`, converting normalized messages, images and tools to the vendor's format.
2. Map `web_search` to the vendor's native search, or leave it off.
3. Add the vendor's section to `config/models.json`.
4. Add a keychain entry and a Settings option.
5. Add the vendor to the eval runner.

Nothing else in the app should change. If it has to, the abstraction is leaking: fix the abstraction.

---

## 7. Local tools (for engines without their own tools)

| Tool | Input | Output | Limits (starting values) |
| --- | --- | --- | --- |
| `list_dir` | `path` (relative to the workspace root), `depth` (≤ 3) | Tree of names; `.gitignore` respected | ≤ 500 entries |
| `read_file` | `path`, optional `start_line`, `end_line` | File text with line numbers | ≤ 200 KB and ≤ 2,000 lines per call; text files only |
| `search_project` | `query` (literal or regex), optional `glob` | `path:line: text` matches | ≤ 100 matches; `.gitignore` respected (use ripgrep's crates `ignore` + `grep`; **Verify**) |
| `npm_info` | `package` | Description, latest version, homepage, repository, plus the installed version from the lockfile or `node_modules/<pkg>/package.json` | Uses `https://registry.npmjs.org/<pkg>`; 5 s timeout; cached for 24 h |

Sandbox rules, enforced in `tools/sandbox.rs` and covered by unit tests:

- Paths are resolved against the workspace root and canonicalized. Anything outside the root, including via symlinks, is refused.
- Never read secret files:
  - `.env*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `id_rsa*`, `id_ed25519*`
  - `.npmrc`, `.pypirc`, `.netrc`
  - Anything under `.aws/`, `.ssh/` or `.gnupg/`
  - `*secret*` and `*credential*` files
  - Keychain and database files
- Refuse binary files.
- No tool can write, delete or execute anything.
- Tools are only offered in code mode, when the bridge has a workspace. Outside code mode, only web search is available.

---

## 8. VS Code extension and bridge

Desktop side (bridge):

- Runs a WebSocket server on `127.0.0.1` on a random port.
- Writes `{ "port": …, "token": … }` to `~/Library/Application Support/<app id>/bridge.json` with permissions `0600`. A new token is generated each launch.

Extension (`extensions/vscode`, TypeScript):

- Activates on `onStartupFinished`, reads `bridge.json`, connects and sends `hello { token, vscodeVersion }`.
- Pushes `state` messages on change, debounced (starting value 150 ms):
  - `workspaceFolders` (absolute paths)
  - `activeFile`
  - `visibleRanges` and the visible text of the active editor (≤ 400 lines)
  - `selections`
  - `openFiles`
  - `focused` (`vscode.window.state.focused`)
- Reconnects with backoff when the app isn't running. It does nothing else: no commands, no UI in the MVP (maybe a status bar item showing "Context connected").

Multiple windows:

- Each VS Code window has its own extension host, so each one is a separate connection.
- The app uses the connection whose window is focused while VS Code is the frontmost app.

Pointing inside VS Code:

- Editor: the lens crop shows the token and the extension provides the exact visible text. Together they pin down the line, so no line-height maths is needed.
- Explorer sidebar: accessibility gives the row's name, and the model resolves it with `list_dir` / `search_project`.

Later (not MVP): publish to Open VSX so Cursor and VSCodium can use the same extension.

---

## 9. Privacy and security requirements

- Capture only on the hotkey. No background recording and no screenshot history. Packs are saved only in dev mode or when the user explicitly saves one.
- What was sent is always one click away in the popover.
- Excluded apps: default list of password managers (1Password, Bitwarden, Keychain Access; **Verify** bundle IDs), editable by the user.
- Read-only, scoped file access with the secret deny-list from section 7.
- Keys: keychain only, Rust only, never logged. Redact anything key-like in logs and error reports.
- No telemetry, no analytics, no crash reporting that sends data anywhere.
- Network: only the selected AI vendor, `registry.npmjs.org`, and links the user clicks.

---

## 10. Performance and cost targets

- Lens: visible ≤ 50 ms after key down.
- Capture budgets: accessibility query ≤ 150 ms, capture ≤ 150 ms (starting values).
- First streamed words: about 2 s (typical) at Low effort on the API engine. Claude Code will be slower; measure it and report.
- Cost: log input and output tokens and estimated cost per answer in dev mode.
  - Keep images small: full resolution for the lens crop only.
  - Rely on prompt caching.
  - Keep Low answers short.

---

## 11. Build plan: MVP milestones (macOS)

Each milestone ends with a demo the owner can run. Tick items as they're done.

### M0: Skeleton

- [x] Tauri 2 app scaffold (React + TypeScript + Vite), menu-bar-only app with tray (enable/disable, settings, quit)
- [x] Repo layout as in 5.2; `config/` files with placeholders (the UI's `lens/`, `popover/` and `shared/` folders arrive with their windows)
- [x] Lint and format: rustfmt, clippy `-D warnings`, ESLint, Prettier, `tsc --noEmit`
- [x] Tests wired: `cargo test`, Vitest
- [x] GitHub Actions on a macOS runner: lint, test, build
- [x] GitHub Actions on a Windows runner: `cargo clippy -D warnings` of the Rust crate (keeps the Windows stubs compiling)
- [x] `README.md` stub (what it is, how to run in dev)

**Accepted when:** the dev command launches a tray-only app with a settings window, and CI is green.

### M1: Pointing

- [x] Permissions onboarding: status checks for Accessibility and Screen Recording, buttons that open System Settings, re-check loop
- [x] `CGEventTap` input thread: hold-to-show hotkey (default Right Option), Esc, Space, mouse move, scroll; re-enable on timeout
- [x] Accidental-trigger rule from section 14 (other key pressed while held, short taps)
- [x] Lens overlay window: follows the cursor, click-through, all Spaces, above full-screen apps
- [x] Scroll resizes the lens (swallowed); Shift+scroll reserved for M2
- [x] Excluded-apps check with the "Context is off here" state

**Accepted when:** the lens works over any app, including full-screen ones. Scrolling resizes it without scrolling the app, and Esc cancels.

### M2: Capture and the context pack

- [x] AX element at cursor, ancestors, nearby text, selection, app/window, browser URL; Electron/Chrome accessibility switch-on
- [x] Element highlight and snapping; Shift+scroll steps parent/child
- [x] ScreenCaptureKit lens crop and window image; own windows excluded; lens outline and cursor marker drawn on the window image
- [x] Source classifier from `config/sources.json` (**Verify** its bundle IDs)
- [x] ContextPack builder with caps; save and load a pack (JSON + images)
- [x] Dev-only pack viewer (shows everything a pack contains)

**Accepted when:** packs captured in VS Code, Chrome (an Amazon product page), Safari, Finder and Slack look right in the viewer: correct focus text, URL and source hint. (Slack isn't installed on the owner's Mac; any Electron app, such as GitHub Desktop, stands in.)

### M3: Engine layer, Claude API and the popover

- [x] Engine trait, normalized messages, orchestrator with tool loop, budgets and cancellation
- [x] `anthropic_api` adapter: streaming, images, web search server tool, prompt caching, error mapping
- [x] `config/models.json` loading (**Verify** model IDs); effort levels, Go deeper and ceiling
- [x] Keychain storage for the API key; engine settings UI with the spend-limit tip
- [x] Popover UI: "You pointed at" header with correction, streaming markdown, status line, Go deeper, Copy, What was sent, follow-up box, effort indicator
- [x] Ask mode (Space while holding)
- [x] System prompt and per-source guidance (4.6)

**Accepted when:**

- Pointing at an Amazon product gives a correct `TARGET` and a 2–4 sentence answer.
- Go deeper returns specs, complaints and alternatives.
- First words arrive in about 2 s (typical).
- The key never appears in logs or the webview (checked).

### M4: Code projects

- [ ] VS Code extension and the bridge (token handshake, state messages, multi-window selection)
- [ ] Local tools with the sandbox; unit tests for path escapes and the secret deny-list
- [ ] `npm_info` with lockfile/installed version
- [ ] Project summary cache (4.7)
- [ ] Clickable file paths (`vscode://file/…`)

**Accepted when:**

- The express example works: the answer says why this project uses Express and names the files that use it.
- Secret files are never read (tests).
- A second question in the same project is noticeably faster.

### M5: Claude Code engine

- [ ] Detection (paths + manual path) and `claude auth status` readiness
- [ ] Spawn with the flags in 6.5 (resolve its **Verify** list first); stream-json parsing; status line from tool use
- [ ] Screenshots via temp files (or stream-json images if supported); cleanup
- [ ] Follow-ups and Go deeper via `--resume`
- [ ] Engine picker in onboarding and Settings

**Accepted when:** the Amazon and express examples also work on the Claude Code engine, with no API key configured.

### M6: Hardening and demo

- [ ] Error states: no key, invalid key, no credits, rate limit, offline, Claude Code missing or signed out
- [ ] Eval set of 20–30 saved packs across sources, with expectations (13.2), and the runner
- [ ] Dev-mode cost and latency panel
- [ ] Unsigned `.dmg` build for personal use (signing comes in phase 4)
- [ ] README: demo GIF, install and dev instructions, architecture overview, privacy statement

**Accepted when:** the eval pass rate is agreed with the owner and met, and a fresh Mac can go from download to first answer by following the README alone.

---

## 12. After the MVP

### Phase 2: Windows

- Input: `SetWindowsHookEx` with `WH_KEYBOARD_LL` and `WH_MOUSE_LL` (swallow scroll while the lens is up).
- Accessibility: UI Automation `ElementFromPoint`, tree walking, reading the URL from the browser's address bar (**Verify** Chromium's behavior with UIA clients).
- Capture: `Windows.Graphics.Capture`.
- Secrets: Credential Manager (via `keyring`).
- Bridge: `bridge.json` goes in `%APPDATA%\<app id>\`.
- Overlay windows: topmost, non-activating, layered and click-through.
- Limitation: a normal app can't inspect windows running as administrator (UIPI). Show "Can't read this window".
- Installer: via the Tauri bundler (MSI or NSIS).
- **Accepted when:** the same demos pass on Windows.

### Phase 3: Depth

- Chromium browser extension (Chrome, Edge, Arc, Brave) that reports the hovered element's exact text, the page text and the URL.
- More engines: OpenAI, Gemini, OpenRouter, local models via Ollama.
- MCP server mode: Claude Desktop or Claude Code can ask Context "what is the user pointing at?"
- Editable mode profiles (prompt + tools + app/URL match), pinned answers and history.
- PyPI and NuGet lookups; Cursor (Open VSX) and JetBrains support.

### Phase 4: Public launch

- Signed and notarized macOS builds, signed Windows builds, auto-update (Tauri updater), releases built by CI from tagged source.
- Voice questions.
- README with a demo GIF, an architecture write-up (blog post), contribution guide.
- Before launch: license chosen, name and trademark check, and Anthropic confirmation for the Claude Code engine.

---

## 13. Testing and quality

### 13.1 Automated tests

- Unit tests:
  - Classifier rules
  - Sandbox (path escapes, symlinks, deny-list, size caps)
  - Effort mapping and ceiling
  - `TARGET` line parsing
  - SSE and stream-json parsers, using recorded fixture streams
  - Tool loop budgets
- Integration tests against real APIs only when an env var is set (for example `CONTEXT_LIVE_TESTS=1`). They are never required in CI.

### 13.2 Replay and eval

- Every pack can be saved to `eval/cases/<name>/` as `pack.json` + images + `expect.yaml`:

  ```yaml
  target_contains: ["express"]
  answer_mentions: ["server", "src/server.ts"]
  answer_must_not: ["I can't see"]
  max_sentences_low: 4
  ```

- Runner: `cargo run -p context-eval -- --engine anthropic_api --effort low [--case <name>]`. It prints a pass/fail table with latency and cost.
- Run it before and after every prompt or context change, and keep the results in PR descriptions.

### 13.3 Manual demo checklist (each milestone)

VS Code `package.json`, VS Code explorer folder, Amazon product, Linear or Jira ticket, a PDF term, a chart in a web page, an excluded app.

---

## 14. Open decisions (owner)

| Decision | Default until decided |
| --- | --- |
| License: MIT or Apache-2.0 | Don't add a LICENSE file yet |
| Hotkey | Hold Right Option ⌥ (AIPointer already uses Right Cmd) |
| Server-side refusal fallbacks (High and Max) re-run a declined request on another model | On, as Anthropic recommends; the popover just keeps streaming. Turn off by removing `fallbacks` from `config/models.json`. |
| Accidental triggers: Right Option types characters on many non-US keyboard layouts, and Option+key shortcuts are common | If any other key (except Space, Esc and Shift) is pressed while the hotkey is held, cancel silently and pass the key through. A hold shorter than 200 ms (starting value) never asks. The lens may still appear within 50 ms. |
| Name: "Context" is hard to search for and may clash with trademarks. The repo is named `smart-context`. | Keep "Context" as the display name; use a placeholder app ID (`dev.context.app`) that's easy to change |
| Claude Code engine terms check with Anthropic | Build it; confirm before the public launch |
| Default answer language | The system language |
| VS Code offers "Screen Reader Optimized" mode once Context switches on its accessibility tree | Keep switching it on (M2 needs VS Code's text). Tell users they can answer No, or set `editor.accessibilitySupport` to `off`. Revisit in M4, when the extension provides the editor text. |
| The menu-bar icon can hide behind the notch | Opening Context again shows Settings. A global shortcut for Settings is possible later. |

---

## 15. Glossary

| Term | Meaning |
| --- | --- |
| Lens | The translucent rectangle around the cursor that marks the focus area |
| Context pack | Everything captured for one question: images, accessibility text, app, window, URL, workspace |
| Source hint | Classifier output (CodeEditor, Shopping, WorkTool, WebPage, OtherApp) that steers the prompt |
| Engine | An adapter to an AI vendor or runtime, behind the Engine trait |
| Effort level | Low, Medium, High or Max; maps to a model and budgets per engine |
| Go deeper | Re-ask one effort level up in the same conversation |
| Bridge | The local WebSocket connection between the desktop app and the VS Code extension |

---

## 16. Changes since the original Google Doc

28 Sep 2026, when the plan moved into the repo:

- 0: points to this change log.
- 2: added the package-manager decision (npm workspaces + Cargo workspace).
- 3.1: added the accidental-trigger point; the rule itself is a new open decision in 14.
- 3.3 and 6.1: the API key paste field is write-only (webview → Rust once, never back).
- 3.3: fixed the reference for Claude Code detection (6.6 → 6.5). 4.6: fixed the reference for the tool budget (6.4 → 6.3).
- 4.6: added a **Verify** note on the minimum cacheable prompt length.
- 5.2: named the repo and added the root workspace manifests; the Cargo workspace exists from M0. The agent rules live in `CLAUDE.md` (the original `AGENTS.md` was merged into it).
- 5.4 and 11 (M0): CI also runs `cargo clippy` on Windows, since the Windows stubs must keep compiling and nothing else checks them.
- M0 setup notes: TypeScript is pinned to 6.0.x because typescript-eslint doesn't support TypeScript 7 yet; jsdom is pinned to 29 because jsdom 30 needs Node 22.22+; Vite 8 calls the multi-page option `build.rolldownOptions`. Dev builds open Settings on launch because a menu-bar icon can hide behind the notch.
- 6.5: added a **Verify** list from `claude --help` (Claude Code 2.1.283): `--strict-mcp-config`, `--restricted`, `--max-turns`, `--effort` values.
- 11: M1 gained the accidental-trigger item; M2, M3 and M5 items point at their **Verify** work.
- 14: added the accidental-trigger decision and noted the repo name.

28 Sep 2026, M1:

- 5.2 and 5.3: new `pointing` module for the gesture, lens geometry and pointing session; the plan had no home for them.
- 5.4: the traits as built. Handlers return `Pass`/`Swallow` per event instead of `set_swallow_scroll`; new `Screens` and `Overlay` traits; `Permissions::request_permission` shows the OS prompt and opens the pane.
- 3.3: Settings opens on launch when a permission is missing (replacing M0's dev-only auto-open). The default excluded-apps list lives in `settings`; editing it comes with the settings store.
- 5.5: the lens is a non-activating `NSPanel`, not a plain window. Testing showed a plain window stays off screen over full-screen apps.
- M1 findings: on macOS 26 the Dock keeps a full-screen window at the Dock level, so window lookups skip layers from the Dock up. In development, macOS grants permissions to the terminal or editor running `npm run dev`, not to Context; Screen Recording only takes effect after that app restarts.

28 Sep 2026, M2:

- 3.3: opening Context again shows Settings; dev builds open Settings at launch, with a button for the capture viewer. The tray icon was hidden behind the notch on the owner's Mac.
- 4.2: `window: Option<WindowInfo>` carries the app, replacing separate `app` and `window` fields; `workspace` arrives in M4.
- 4.3: nearby text comes from a hit-test grid inside the lens, not a tree walk; URLs are kept for browsers only; accessibility side effects noted.
- 4.4: ScreenCaptureKit scales the screenshots; the window limit (1568 px, 1.15 MP) is checked against Anthropic's vision docs.
- 5.3: `pointing` gained the inspector thread. The traits as built: `Accessibility::inspect` (one call per question, returning the element chain, nearby text, selection and URL), `ScreenCapture::screenshots` (both images at once, within size limits), `platform::is_reopen`.
- 14: two new open decisions (VS Code screen-reader mode, the hidden menu-bar icon).
- M2 findings: a capture takes about 350–450 ms (accessibility ~40, screenshots ~170–280, images ~135). Listing windows for ScreenCaptureKit dominates the screenshot time; M3 should start it at key down, as 4.1 step 1 says.

28 Sep 2026, M3:

- 2 and 3.3: engine-specific words in Settings ("Claude API key", the Console link) come from the engine (`EngineInfo`), so the UI itself never names a vendor.
- 3.2: the popover is a fixed 440×380 panel that scrolls, placed right of the lens (else left, always on screen). It closes on Esc or a click outside (seen by the input hook), and a new gesture replaces it. "What was sent" shows the pack inline. Changing the effort re-asks at that level, up to the ceiling.
- 4.1 step 1: the window listing for screenshots is prefetched at key down.
- 4.6: the prompt as built (see 4.6's last paragraph).
- 5.2 and 5.3: new `popover` module; the orchestrator's `Toolbox` trait and `AnswerEvent` stream.
- 6.2: `Block::Opaque` round-trips vendor blocks; `EngineEvent::Status`, `Limits`, `EngineInfo` and `Readiness` added.
- 6.3 and 6.4: `config/models.json` as built: per level `model`, `max_tokens`, `tool_budget`, `web_search_max_uses`, `web_search_tool`, optional `effort`, `fallbacks` and `betas`, and prices for dev-mode cost logs. Verified IDs: `claude-haiku-4-5` (no effort setting), `claude-sonnet-5` (effort medium), `claude-opus-5` (effort high; max). `max_tokens` grew, because it caps reasoning plus the answer; the answer's length comes from the prompt.
- 14: new open decision on refusal fallbacks.
- M3 findings: the first real answer (Low, over VS Code) cost about $0.0066: 137 output tokens plus a ~4.7k-token cache write. The API key doesn't appear in the logs, the dev output or saved packs (checked).
