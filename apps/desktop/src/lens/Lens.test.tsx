import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Lens } from "./Lens";

const rect = { x: 40, y: 60, width: 240, height: 120 };

describe("Lens", () => {
  it("draws the lens at the given rectangle", () => {
    render(<Lens view={{ visible: true, rect, offHere: false }} />);
    const lens = screen.getByTestId("lens");
    expect(lens.style.transform).toBe("translate(40px, 60px)");
    expect(lens.style.width).toBe("240px");
    expect(lens.style.height).toBe("120px");
    expect(lens.textContent).toBe("");
  });

  it("says when Context is off for the app underneath", () => {
    render(<Lens view={{ visible: true, rect, offHere: true }} />);
    expect(screen.getByTestId("lens").textContent).toBe("Context is off here");
  });
});
