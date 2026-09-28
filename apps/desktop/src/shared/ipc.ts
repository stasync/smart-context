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
