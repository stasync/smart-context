import type { ElementInfo, PackView, Rect } from "./ipc";

const rect = (r: Rect) =>
  `${Math.round(r.x)}, ${Math.round(r.y)} · ${Math.round(r.width)}×${Math.round(r.height)} pt`;

/** One line per element: role, then its most telling text. */
function summary(e: ElementInfo): string {
  const text = e.title ?? e.value ?? e.description;
  const role = e.role ?? "?";
  return text ? `${role} — ${text}` : role;
}

const ELEMENT_FIELDS: (keyof ElementInfo)[] = [
  "role",
  "subrole",
  "roleDescription",
  "title",
  "value",
  "description",
  "help",
];

export function PackDetails({ view }: { view: PackView }) {
  const { pack } = view;
  const app = pack.window?.app;

  return (
    <article className="details">
      <header>
        <span className="badge" data-testid="source">
          {pack.source}
        </span>
        <h1>{app?.name ?? "Unknown app"}</h1>
        <p className="muted">
          {app?.bundleId ?? "no bundle ID"}
          {pack.window?.title && ` · ${pack.window.title}`}
        </p>
      </header>

      <dl className="facts">
        <dt>URL</dt>
        <dd data-testid="url">{pack.url ?? "—"}</dd>
        <dt>Selection</dt>
        <dd>{pack.selection ?? "—"}</dd>
        <dt>Cursor</dt>
        <dd>
          {Math.round(pack.cursor.x)}, {Math.round(pack.cursor.y)}
        </dd>
        <dt>Lens</dt>
        <dd>{rect(pack.lens)}</dd>
        <dt>Scale</dt>
        <dd>{pack.displayScale}×</dd>
        <dt>Captured</dt>
        <dd>{new Date(pack.createdAt).toLocaleString()}</dd>
      </dl>

      <section className="images">
        <figure>
          {view.lensImageUrl ? (
            <img src={view.lensImageUrl} alt="What was under the lens" />
          ) : (
            <p className="muted">No lens image</p>
          )}
          <figcaption>
            Lens{" "}
            {pack.lensImage &&
              `(${pack.lensImage.width}×${pack.lensImage.height} PNG)`}
          </figcaption>
        </figure>
        <figure>
          {view.windowImageUrl ? (
            <img src={view.windowImageUrl} alt="The window, lens outlined" />
          ) : (
            <p className="muted">No window image</p>
          )}
          <figcaption>
            Window{" "}
            {pack.windowImage &&
              `(${pack.windowImage.width}×${pack.windowImage.height} JPEG)`}
          </figcaption>
        </figure>
      </section>

      <section>
        <h2>Focus element</h2>
        {pack.focus ? (
          <dl className="facts" data-testid="focus">
            {ELEMENT_FIELDS.filter((f) => pack.focus?.[f]).map((f) => (
              <div key={f} className="fact">
                <dt>{f}</dt>
                <dd>{String(pack.focus?.[f])}</dd>
              </div>
            ))}
            {pack.focus.bounds && (
              <div className="fact">
                <dt>bounds</dt>
                <dd>{rect(pack.focus.bounds)}</dd>
              </div>
            )}
          </dl>
        ) : (
          <p className="muted">No accessibility element</p>
        )}
      </section>

      <section>
        <h2>Ancestors</h2>
        <ol className="ancestors">
          {pack.ancestors.map((a, i) => (
            <li key={i}>{summary(a)}</li>
          ))}
        </ol>
      </section>

      <section>
        <h2>Nearby text ({pack.nearbyText.length} chars)</h2>
        <pre className="nearby" data-testid="nearby">
          {pack.nearbyText || "—"}
        </pre>
      </section>
    </article>
  );
}
