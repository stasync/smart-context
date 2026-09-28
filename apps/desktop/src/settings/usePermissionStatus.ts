import { useEffect, useState } from "react";
import { permissionStatus, type PermissionStatus } from "../shared/ipc";

/** Polls the permission status, so the UI updates as soon as the user allows one. */
export function usePermissionStatus(
  intervalMs = 1000,
): PermissionStatus | null {
  const [status, setStatus] = useState<PermissionStatus | null>(null);

  useEffect(() => {
    let cancelled = false;
    const check = () =>
      permissionStatus()
        .then((next) => {
          if (!cancelled) setStatus(next);
        })
        .catch((error: unknown) => {
          console.error("Couldn't read permission status", error);
        });

    void check();
    const timer = setInterval(check, intervalMs);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [intervalMs]);

  return status;
}
