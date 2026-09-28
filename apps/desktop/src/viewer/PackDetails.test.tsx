import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { PackView } from "../shared/ipc";
import { PackDetails } from "./PackDetails";

const view: PackView = {
  pack: {
    id: "1",
    createdAt: "2026-09-28T18:00:00Z",
    cursor: { x: 600, y: 500 },
    lens: { x: 480, y: 440, width: 240, height: 120 },
    displayScale: 2,
    window: {
      id: 7,
      title: "Echo Dot",
      bounds: { x: 0, y: 0, width: 1200, height: 800 },
      app: { name: "Google Chrome", bundleId: "com.google.Chrome", pid: 42 },
    },
    url: "https://www.amazon.com/dp/B0",
    selection: null,
    focus: {
      role: "AXStaticText",
      subrole: null,
      roleDescription: "text",
      title: null,
      value: "Echo Dot (5th Gen)",
      description: null,
      help: null,
      bounds: { x: 500, y: 480, width: 200, height: 24 },
    },
    ancestors: [
      {
        role: "AXHeading",
        subrole: null,
        roleDescription: null,
        title: "Product title",
        value: null,
        description: null,
        help: null,
        bounds: null,
      },
    ],
    nearbyText: "Echo Dot (5th Gen)\n4.7 out of 5 stars",
    lensImage: null,
    windowImage: null,
    source: "Shopping",
  },
  lensImageUrl: null,
  windowImageUrl: null,
};

describe("PackDetails", () => {
  it("shows what was captured", () => {
    render(<PackDetails view={view} />);
    expect(screen.getByTestId("source").textContent).toBe("Shopping");
    expect(screen.getByTestId("url").textContent).toBe(
      "https://www.amazon.com/dp/B0",
    );
    expect(screen.getByTestId("focus").textContent).toContain(
      "Echo Dot (5th Gen)",
    );
    expect(screen.getByText("AXHeading — Product title")).toBeTruthy();
    expect(screen.getByTestId("nearby").textContent).toContain("4.7 out of 5");
    expect(screen.getByText("No lens image")).toBeTruthy();
  });
});
