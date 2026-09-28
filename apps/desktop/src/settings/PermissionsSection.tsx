import type { Permission, PermissionStatus } from "../shared/ipc";

const PERMISSIONS: { which: Permission; title: string; why: string }[] = [
  {
    which: "accessibility",
    title: "Accessibility",
    why: "Notices the hotkey and reads the text of what you point at.",
  },
  {
    which: "screenRecording",
    title: "Screen Recording",
    why: "Takes a screenshot of what you point at, only when you ask.",
  },
];

type Props = {
  status: PermissionStatus | null;
  onRequest: (which: Permission) => void;
};

export function PermissionsSection({ status, onRequest }: Props) {
  const allGranted =
    status !== null && PERMISSIONS.every(({ which }) => status[which]);

  return (
    <section className="permissions" aria-labelledby="permissions-title">
      <h2 id="permissions-title">Permissions</h2>
      <p>
        {allGranted
          ? "All set. Hold Right Option ⌥ and point at anything."
          : "Context needs two macOS permissions before it can explain what you point at."}
      </p>
      <ul>
        {PERMISSIONS.map(({ which, title, why }) => (
          <li key={which} className="permission">
            <div>
              <strong>{title}</strong>
              <p className="muted">{why}</p>
            </div>
            {status?.[which] ? (
              <span className="granted">Allowed</span>
            ) : (
              <button type="button" onClick={() => onRequest(which)}>
                Open System Settings
              </button>
            )}
          </li>
        ))}
      </ul>
      {!allGranted && (
        <p className="muted small">
          If a permission stays off after you allow it, quit Context from the
          menu bar and open it again.
          {import.meta.env.DEV &&
            " In development, macOS asks on behalf of the terminal or editor that runs Context."}
        </p>
      )}
    </section>
  );
}
