# Context

Point at anything on your screen and get a short explanation of what it is and why it's there.

Hold a hotkey, aim a lens at a word, a button, a product photo or a line of code, and release. A small popover next to the cursor explains it in the context of where you are. In VS Code it reads your project, so pointing at `"express"` in `package.json` tells you what Express does *in this project* and which files use it.

- **No backend, no telemetry.** Context talks only to the AI you choose: your own Claude API key, or your own Claude Code install.
- **Reads the real source**, not just pixels: accessibility text, the page URL, your project's files.
- **Open source.** macOS first; Windows next.

> Status: early development. Pointing, capturing and answers with your own Claude API key work (milestones M1–M3); project reading in VS Code arrives in M4. The full plan is in [docs/PLAN.md](docs/PLAN.md).

## Run in development

Requirements:

- macOS 14 or later
- Xcode Command Line Tools: `xcode-select --install`
- Rust (via [rustup](https://rustup.rs)); the version is pinned in `rust-toolchain.toml`
- Node.js 22 or later

```sh
npm install
npm run dev
```

The app lives in the menu bar (look for the ◉ icon; on a crowded menu bar it can hide behind the notch). Dev builds open the Settings window on launch.

- Hold **Right Option ⌥** and move the mouse to aim the lens. The element under the cursor gets a dashed outline.
- Scroll to resize the lens; **Shift+scroll** steps out to the parent element (and back in), snapping the lens to it.
- Release to get an answer in a popover next to the cursor. Esc cancels, or closes the popover.
- Press **Space** while holding to type your own question instead.
- In the popover: click "You pointed at" to correct it, **Go deeper** for a fuller answer, **What was sent** to see exactly what left your Mac, and the box at the bottom for follow-ups.
- In dev builds every capture is saved as a context pack. **Settings → Development → Open captures** shows them all: screenshots, accessibility text, URL and source.

For answers, paste a Claude API key into **Settings → AI engine** (create a dedicated key with a spend limit in the [Claude Console](https://platform.claude.com/settings/keys)). It's stored in your macOS Keychain and never shown again. Answers start at Low effort (a small, cheap model); Go deeper steps up to the ceiling set in Settings.

Context needs **Accessibility** and **Screen Recording** (Settings opens on launch until both are allowed). In development, macOS grants them to the terminal or editor that runs `npm run dev`, such as Terminal or VS Code, not to Context itself. After allowing Screen Recording, restart that app.

Pointing at VS Code switches on its accessibility tree, and VS Code may then offer "Screen Reader Optimized" mode. You can answer No (or set `editor.accessibilitySupport` to `off`).

## Checks

```sh
npm run lint && npm run format:check && npm run typecheck && npm test
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## Repository layout

| Path | What's there |
| --- | --- |
| `apps/desktop/src` | React + TypeScript UI (settings now; lens and popover later) |
| `apps/desktop/src-tauri` | Rust core: tray, windows, and the modules from the plan |
| `extensions/vscode` | VS Code extension that shares editor state with the app |
| `config` | Effort → model mapping and source-classifier rules |
| `eval` | Saved context packs and expectations |
| `docs/PLAN.md` | Product and build plan |

Contributors and coding agents: read [CLAUDE.md](CLAUDE.md) first.
