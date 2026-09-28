import { useEffect, useState } from "react";
import {
  listPacks,
  loadPack,
  onPackSaved,
  type PackSummary,
  type PackView,
} from "../shared/ipc";
import { PackDetails } from "../shared/PackDetails";

/** Dev-only: every saved context pack, newest first (docs/PLAN.md, M2). */
export function ViewerApp() {
  const [packs, setPacks] = useState<PackSummary[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<{ key: string; view: PackView } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const refresh = (select?: string) =>
      listPacks()
        .then((list) => {
          if (cancelled) return;
          setPacks(list);
          setSelected((current) => select ?? current ?? list[0]?.key ?? null);
        })
        .catch((e: unknown) => setError(String(e)));

    void refresh();
    const unlisten = onPackSaved((key) => void refresh(key));
    return () => {
      cancelled = true;
      void unlisten.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    if (selected === null) return;
    let cancelled = false;
    loadPack(selected)
      .then((view) => {
        if (!cancelled) setLoaded({ key: selected, view });
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [selected]);

  const view = loaded?.key === selected ? loaded.view : null;

  return (
    <div className="viewer">
      <nav className="viewer__list" aria-label="Saved captures">
        {packs.length === 0 && (
          <p className="muted">
            No captures yet. Hold Right Option ⌥, point at something and
            release.
          </p>
        )}
        {packs.map((p) => (
          <button
            key={p.key}
            type="button"
            className={p.key === selected ? "pack pack--selected" : "pack"}
            onClick={() => setSelected(p.key)}
          >
            <strong>{p.app ?? "Unknown app"}</strong>
            <span>
              {p.source} · {new Date(p.createdAt).toLocaleTimeString()}
            </span>
          </button>
        ))}
      </nav>
      <main className="viewer__details">
        {error && <p className="error">{error}</p>}
        {view && <PackDetails view={view} />}
      </main>
    </div>
  );
}
