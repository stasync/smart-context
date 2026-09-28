import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LensView } from "../shared/ipc";
import { Lens } from "./Lens";

const rect = { x: 40, y: 60, width: 240, height: 120 };
const view: LensView = {
  visible: true,
  rect,
  highlight: null,
  snapped: false,
  offHere: false,
};

describe("Lens", () => {
  it("draws the lens at the given rectangle", () => {
    render(<Lens view={view} />);
    const lens = screen.getByTestId("lens");
    expect(lens.style.transform).toBe("translate(40px, 60px)");
    expect(lens.style.width).toBe("240px");
    expect(lens.style.height).toBe("120px");
    expect(lens.textContent).toBe("");
    expect(screen.queryByTestId("highlight")).toBeNull();
  });

  it("outlines the element under the cursor", () => {
    const highlight = { x: 10, y: 20, width: 300, height: 18 };
    render(<Lens view={{ ...view, highlight }} />);
    expect(screen.getByTestId("highlight").style.transform).toBe(
      "translate(10px, 20px)",
    );
  });

  it("marks a lens snapped to an element", () => {
    render(<Lens view={{ ...view, snapped: true }} />);
    expect(screen.getByTestId("lens").className).toContain("lens--snapped");
  });

  it("says when Context is off for the app underneath", () => {
    const highlight = { x: 10, y: 20, width: 300, height: 18 };
    render(<Lens view={{ ...view, highlight, offHere: true }} />);
    expect(screen.getByTestId("lens").textContent).toBe("Context is off here");
    expect(screen.queryByTestId("highlight")).toBeNull();
  });
});
