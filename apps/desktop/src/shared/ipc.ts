// Typed wrappers for the Rust core's commands and events. Field names match
// the Rust types' serde (camelCase) output.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Rect = { x: number; y: number; width: number; height: number };

/** What the lens window draws, relative to the overlay. */
export type LensView = {
  visible: boolean;
  rect: Rect;
  /** The cursor is over an excluded app. */
  offHere: boolean;
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
