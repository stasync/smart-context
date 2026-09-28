import { openViewer, requestPermission } from "../shared/ipc";
import { PermissionsSection } from "./PermissionsSection";
import { usePermissionStatus } from "./usePermissionStatus";

export function SettingsApp() {
  const status = usePermissionStatus();

  return (
    <main className="settings">
      <h1>Context</h1>
      <p>
        Context runs in the menu bar. Hold the hotkey, point at anything on
        screen, and release to get a short explanation.
      </p>
      <PermissionsSection
        status={status}
        onRequest={(which) => void requestPermission(which)}
      />
      {import.meta.env.DEV && (
        <section aria-labelledby="dev-title">
          <h2 id="dev-title">Development</h2>
          <p className="muted">
            Every capture is saved as a context pack while developing.
          </p>
          <button type="button" onClick={() => void openViewer()}>
            Open captures
          </button>
        </section>
      )}
    </main>
  );
}
