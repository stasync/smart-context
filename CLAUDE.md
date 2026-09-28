# CLAUDE.md: working rules for coding agents

Context is an open-source desktop app: hold a hotkey, point at anything on screen, and get a short explanation of what it is and why it's there. The full spec is in [docs/PLAN.md](docs/PLAN.md). Read it before you change anything.

## Stack

- Tauri 2: Rust core in `apps/desktop/src-tauri`, React + TypeScript UI in `apps/desktop/src`.
- VS Code extension (TypeScript) in `extensions/vscode`.
- macOS 14+ first. Windows comes after the MVP, so keep Windows code as compiling stubs behind the platform traits.

## How to work

1. Build milestone by milestone (docs/PLAN.md, section 11). Don't start a milestone until the previous one's acceptance criteria pass.
2. Tick the milestone checkboxes in docs/PLAN.md as you finish items.
3. Keep changes small and reviewable. Every PR or commit says which milestone item it covers.
4. When a decision isn't in the plan, ask the owner. Open decisions are listed in section 14, each with a default to use meanwhile.
5. If the code must diverge from the plan, update the plan in the same change and explain why.

## Verify before use

These things change over time. Check the official docs, never your memory:

- Claude model IDs (they go only in `config/models.json`)
- The Anthropic API version header and web search tool type string
- Claude Code CLI flags (`claude --help`)
- Crate and npm package versions and whether they're maintained
- macOS and Windows API availability
- App bundle IDs

## Hard rules

- No backend, no telemetry. Network traffic goes only to the AI engine the user picked, plus `registry.npmjs.org` for `npm_info`.
- API keys: OS keychain and Rust memory only. Never log them, never send them to the webview, never write them to disk in plain text, never put them in tests or fixtures.
- Never implement "Sign in with Claude" or claude.ai OAuth, and never read or reuse Claude Code's credentials. The Claude Code engine only runs the user's own unmodified `claude` binary, with the flags in docs/PLAN.md section 6.5 (including `--setting-sources user` and `disableAllHooks`).
- Local tools are read-only and sandboxed to the workspace root, with the secret-file deny-list (section 7). Tests must cover path escapes and the deny-list.
- Vendor-neutral: vendor-specific code lives only in `engines/<vendor>.rs` and `config/models.json`. The UI and orchestrator never mention a vendor or model by name.
- Platform-neutral: OS-specific code lives only under `platform/<os>/` behind the traits in section 5.4.
- Effort: every question starts at Low. Only Go deeper or an explicit user choice raises it, never above the user's ceiling.

## Quality bar

- Rust: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`.
- UI and extension: ESLint, Prettier, `tsc --noEmit`, Vitest.
- CI (GitHub Actions, macOS runner) must be green.
- Prompt or context changes: run the eval (docs/PLAN.md section 13.2) before and after, and include both results in the PR description.

## Commands

Run from the repo root:

| Task | Command |
| --- | --- |
| Install JS dependencies | `npm install` |
| Run the app in dev | `npm run dev` |
| All JS checks | `npm run lint && npm run format:check && npm run typecheck && npm test` |
| All Rust checks | `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test` |
| Build the app | `npm run build` |
