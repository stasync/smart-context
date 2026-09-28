import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import {
  EFFORT_LABELS,
  EFFORTS,
  effortCeiling,
  engineStatus,
  removeApiKey,
  saveApiKey,
  setEffortCeiling,
  type Effort,
  type EngineStatus,
} from "../shared/ipc";

/** The AI engine's key and the effort ceiling. The key field is write-only:
 * the UI only ever learns whether a key is saved. */
export function EngineSection() {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [ceiling, setCeiling] = useState<Effort | null>(null);
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);

  const refresh = () =>
    engineStatus()
      .then(setStatus)
      .catch((e: unknown) => setError(String(e)));

  useEffect(() => {
    engineStatus()
      .then(setStatus)
      .catch((e: unknown) => setError(String(e)));
    effortCeiling()
      .then(setCeiling)
      .catch((e: unknown) => setError(String(e)));
  }, []);

  if (!status) return null;
  const { info, readiness } = status;

  const save = (event: React.FormEvent) => {
    event.preventDefault();
    setError(null);
    saveApiKey(key)
      .then(() => {
        setKey("");
        return refresh();
      })
      .catch((e: unknown) => setError(String(e)));
  };

  const remove = () => {
    setError(null);
    removeApiKey()
      .then(refresh)
      .catch((e: unknown) => setError(String(e)));
  };

  const changeCeiling = (value: Effort) => {
    setCeiling(value);
    setEffortCeiling(value).catch((e: unknown) => setError(String(e)));
  };

  return (
    <section aria-labelledby="engine-title">
      <h2 id="engine-title">AI engine: {info.name}</h2>
      {readiness.state === "ready" ? (
        <div className="permission">
          <p>
            <span className="granted">{info.keyLabel} saved</span>
            <span className="muted"> in your keychain.</span>
          </p>
          <button type="button" onClick={remove}>
            Remove
          </button>
        </div>
      ) : (
        <form className="key-form" onSubmit={save}>
          <label htmlFor="api-key">{info.keyLabel}</label>
          <div className="key-row">
            <input
              id="api-key"
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={key}
              onChange={(e) => setKey(e.target.value)}
            />
            <button type="submit" disabled={!key.trim()}>
              Save
            </button>
          </div>
          <p className="muted small">
            {info.keyHelp}{" "}
            <a
              href={info.keyHelpUrl}
              onClick={(e) => {
                e.preventDefault();
                void openUrl(info.keyHelpUrl);
              }}
            >
              Open
            </a>
          </p>
        </form>
      )}

      {ceiling && (
        <div className="permission">
          <div>
            <strong>Effort ceiling</strong>
            <p className="muted">
              Every answer starts at Low. Go deeper never goes past this.
            </p>
          </div>
          <select
            aria-label="Effort ceiling"
            value={ceiling}
            onChange={(e) => changeCeiling(e.target.value as Effort)}
          >
            {EFFORTS.map((level) => (
              <option key={level} value={level}>
                {EFFORT_LABELS[level]}
              </option>
            ))}
          </select>
        </div>
      )}
      {error && <p className="error">{error}</p>}
    </section>
  );
}
