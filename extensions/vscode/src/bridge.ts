// What the extension sends the Context app, and the pure helpers behind it.
// Kept free of the `vscode` module so it can be unit-tested.
import { join } from "node:path";

/** Context's app identifier; its data folder holds bridge.json. Must match
 * `identifier` in apps/desktop/src-tauri/tauri.conf.json. */
export const APP_ID = "dev.context.app";

/** Visible lines sent per update (docs/PLAN.md 8). */
export const MAX_VISIBLE_LINES = 400;
const MAX_SELECTION_CHARS = 4_000;
const MAX_OPEN_FILES = 50;
const MAX_RETRY_MS = 30_000;
const MAX_WORD_CHARS = 200;
const MAX_LINE_CHARS = 500;

/** Zero-based lines, end inclusive. */
export type LineRange = { start: number; end: number };

export type Selection = { startLine: number; endLine: number; text: string };

export type StateMessage = {
  type: "state";
  focused: boolean;
  workspaceFolders: string[];
  activeFile: string | null;
  visibleRanges: LineRange[];
  visibleText: string;
  selections: Selection[];
  openFiles: string[];
};

/** What the mouse rests on: VS Code asked for a hover there. */
export type Pointer = {
  file: string;
  /** Zero-based. */
  line: number;
  character: number;
  word: string;
  lineText: string;
  /** When VS Code asked, in ms since the Unix epoch. */
  at: number;
};

/** `pointer` is null once what's under the mouse may have changed. */
export type PointerMessage = { type: "pointer"; pointer: Pointer | null };

export type HelloMessage = {
  type: "hello";
  token: string;
  vscodeVersion: string;
  uriScheme: string;
};

/** Where the Context app writes bridge.json: its app data folder. */
export function bridgeFilePath(
  platform: string,
  home: string,
  env: Record<string, string | undefined>,
): string {
  switch (platform) {
    case "darwin":
      return join(
        home,
        "Library",
        "Application Support",
        APP_ID,
        "bridge.json",
      );
    case "win32":
      return join(
        env.APPDATA ?? join(home, "AppData", "Roaming"),
        APP_ID,
        "bridge.json",
      );
    default:
      return join(
        env.XDG_DATA_HOME ?? join(home, ".local", "share"),
        APP_ID,
        "bridge.json",
      );
  }
}

/** The visible ranges, cut to `max` lines in total. */
export function capRanges(
  ranges: LineRange[],
  max = MAX_VISIBLE_LINES,
): LineRange[] {
  const capped: LineRange[] = [];
  let left = max;
  for (const range of ranges) {
    if (left <= 0) break;
    const end = Math.min(range.end, range.start + left - 1);
    capped.push({ start: range.start, end });
    left -= end - range.start + 1;
  }
  return capped;
}

/** The text of the given lines, in order. */
export function visibleText(
  lineAt: (line: number) => string,
  ranges: LineRange[],
): string {
  const lines: string[] = [];
  for (const { start, end } of ranges) {
    for (let line = start; line <= end; line++) lines.push(lineAt(line));
  }
  return lines.join("\n");
}

export function capSelection(text: string): string {
  return text.length > MAX_SELECTION_CHARS
    ? text.slice(0, MAX_SELECTION_CHARS)
    : text;
}

/** A hover position as a pointer, with its word and line capped. */
export function pointerAt(
  file: string,
  line: number,
  character: number,
  word: string,
  lineText: string,
  at: number,
): Pointer {
  return {
    file,
    line,
    character,
    word: word.slice(0, MAX_WORD_CHARS),
    lineText: lineText.slice(0, MAX_LINE_CHARS),
    at,
  };
}

export function capOpenFiles(files: string[]): string[] {
  return [...new Set(files)].slice(0, MAX_OPEN_FILES);
}

/** Reconnect backoff: 1 s, 2 s, 4 s … up to 30 s. */
export function nextDelay(attempt: number): number {
  return Math.min(1000 * 2 ** attempt, MAX_RETRY_MS);
}
