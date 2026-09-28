import { requestPermission } from "../shared/ipc";
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
    </main>
  );
}
