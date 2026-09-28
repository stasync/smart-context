import type { LensView } from "../shared/ipc";

export function Lens({ view }: { view: LensView }) {
  const { x, y, width, height } = view.rect;
  return (
    <div
      className={view.offHere ? "lens lens--off" : "lens"}
      data-testid="lens"
      style={{ transform: `translate(${x}px, ${y}px)`, width, height }}
    >
      {view.offHere && <span className="lens__label">Context is off here</span>}
    </div>
  );
}
