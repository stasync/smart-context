// Typed wrappers for the Rust core's commands and events. Field names match
// the Rust types' serde (camelCase) output.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Rect = { x: number; y: number; width: number; height: number };

/** What the lens window draws, relative to the overlay. */
export type LensView = {
  visible: boolean;
  rect: Rect;
  /** The element under the cursor, while the lens isn't snapped to one. */
  highlight: Rect | null;
  /** Shift+scroll snapped the lens to an element. */
  snapped: boolean;
  /** The cursor is over an excluded app. */
  offHere: boolean;
};

export type Point = { x: number; y: number };

export type ElementInfo = {
  role: string | null;
  subrole: string | null;
  roleDescription: string | null;
  title: string | null;
  value: string | null;
  description: string | null;
  help: string | null;
  bounds: Rect | null;
};

export type AppInfo = { name: string; bundleId: string | null; pid: number };

export type WindowInfo = {
  id: number;
  title: string | null;
  bounds: Rect;
  app: AppInfo;
};

export type SourceHint =
  "CodeEditor" | "Shopping" | "WorkTool" | "WebPage" | "OtherApp";

export type ImageData = {
  file: string;
  mediaType: string;
  width: number;
  height: number;
};

/** The project, from the VS Code extension. Lines are zero-based. */
export type Workspace = {
  roots: string[];
  activeFile: string | null;
  visibleRanges: { start: number; end: number }[];
  visibleText: string;
  selections: { startLine: number; endLine: number; text: string }[];
  openFiles: string[];
  /** What the mouse rested on, as the editor reported it. */
  pointer?: {
    file: string;
    line: number;
    word: string;
    lineText: string;
  } | null;
  uriScheme: string;
};

/** Everything captured for one question (docs/PLAN.md 4.2). */
export type ContextPack = {
  id: string;
  createdAt: string;
  cursor: Point;
  lens: Rect;
  displayScale: number;
  window: WindowInfo | null;
  url: string | null;
  selection: string | null;
  focus: ElementInfo | null;
  ancestors: ElementInfo[];
  nearbyText: string;
  lensImage: ImageData | null;
  windowImage: ImageData | null;
  source: SourceHint;
  workspace?: Workspace | null;
};

export type PackSummary = {
  key: string;
  createdAt: string;
  app: string | null;
  source: SourceHint;
};

/** A saved pack with its images inline as data URLs. */
export type PackView = {
  pack: ContextPack;
  lensImageUrl: string | null;
  windowImageUrl: string | null;
};

export type Permission = "accessibility" | "screenRecording";

export type PermissionStatus = Record<Permission, boolean>;

export function permissionStatus(): Promise<PermissionStatus> {
  return invoke<PermissionStatus>("permission_status");
}

export function requestPermission(which: Permission): Promise<void> {
  return invoke("request_permission", { which });
}

export function onLensUpdate(
  handler: (view: LensView) => void,
): Promise<UnlistenFn> {
  return listen<LensView>("lens:update", (event) => handler(event.payload));
}

export function listPacks(): Promise<PackSummary[]> {
  return invoke<PackSummary[]>("list_packs");
}

export function loadPack(key: string): Promise<PackView> {
  return invoke<PackView>("load_pack", { key });
}

export function onPackSaved(
  handler: (key: string) => void,
): Promise<UnlistenFn> {
  return listen<string>("pack:saved", (event) => handler(event.payload));
}

/** Dev only: opens the viewer for saved context packs. */
export function openViewer(): Promise<void> {
  return invoke("open_viewer");
}

// --- Answers (the popover) ---

export type Effort = "low" | "medium" | "high" | "max";

export const EFFORTS: Effort[] = ["low", "medium", "high", "max"];

export const EFFORT_LABELS: Record<Effort, string> = {
  low: "Low",
  medium: "Medium",
  high: "High",
  max: "Max",
};

/** The conversation, as the Rust orchestrator streams it. */
export type AnswerEvent =
  | { kind: "preparing"; askMode: boolean }
  | {
      kind: "started";
      conversation: number;
      ceiling: Effort;
      awaitingQuestion: boolean;
    }
  | {
      kind: "turnStarted";
      conversation: number;
      turn: number;
      effort: Effort;
      prompt: string | null;
    }
  | { kind: "delta"; conversation: number; turn: number; text: string }
  | {
      kind: "status";
      conversation: number;
      turn: number;
      text: string | null;
    }
  | { kind: "turnDone"; conversation: number; turn: number; truncated: boolean }
  | {
      kind: "failed";
      conversation: number;
      turn: number;
      message: string;
      needsSetup: boolean;
    };

export function onAnswerEvent(
  handler: (event: AnswerEvent) => void,
): Promise<UnlistenFn> {
  return listen<AnswerEvent>("answer:event", (event) => handler(event.payload));
}

export const popoverAsk = (question: string) =>
  invoke<void>("popover_ask", { question });
export const popoverGoDeeper = () => invoke<void>("popover_go_deeper");
export const popoverSetEffort = (effort: Effort) =>
  invoke<void>("popover_set_effort", { effort });
export const popoverCorrect = (target: string) =>
  invoke<void>("popover_correct", { target });
export const popoverClose = () => invoke<void>("popover_close");
/** "What was sent": the context pack, and what the tools read since. */
export type Sent = { view: PackView; activity: string[] };
export const popoverSent = () => invoke<Sent | null>("popover_sent");
/** Opens a project file from an answer in the editor. Rust checks the path. */
export const openFile = (path: string, line: number | null) =>
  invoke<void>("open_file", { path, line });
export const openSettings = () => invoke<void>("open_settings");

// --- The AI engine (Settings) ---

export type EngineInfo = {
  id: string;
  name: string;
  keyLabel: string;
  keyHelp: string;
  keyHelpUrl: string;
};

export type Readiness =
  { state: "ready" } | { state: "needsSetup"; reason: string };

export type EngineStatus = { info: EngineInfo; readiness: Readiness };

export const engineStatus = () => invoke<EngineStatus>("engine_status");
/** Sends the key to the keychain. Nothing ever sends it back. */
export const saveApiKey = (key: string) =>
  invoke<void>("save_api_key", { key });
export const removeApiKey = () => invoke<void>("remove_api_key");
export const effortCeiling = () => invoke<Effort>("effort_ceiling");
export const setEffortCeiling = (effort: Effort) =>
  invoke<void>("set_effort_ceiling", { effort });
