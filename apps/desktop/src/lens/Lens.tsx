import type { CSSProperties } from "react";
import type { LensView, Rect } from "../shared/ipc";

function box({ x, y, width, height }: Rect): CSSProperties {
  return { transform: `translate(${x}px, ${y}px)`, width, height };
}

export function Lens({ view }: { view: LensView }) {
  const classes = ["lens"];
  if (view.offHere) classes.push("lens--off");
  if (view.snapped) classes.push("lens--snapped");

  return (
    <>
      {view.highlight && !view.offHere && (
        <div
          className="highlight"
          data-testid="highlight"
          style={box(view.highlight)}
        />
      )}
      <div
        className={classes.join(" ")}
        data-testid="lens"
        style={box(view.rect)}
      >
        {view.offHere && (
          <span className="lens__label">Context is off here</span>
        )}
      </div>
    </>
  );
}
