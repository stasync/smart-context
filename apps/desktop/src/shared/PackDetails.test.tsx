import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { PackView } from "./ipc";
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
    workspace: null,
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

  it("shows the project in code mode", () => {
    const code: PackView = {
      ...view,
      pack: {
        ...view.pack,
        source: "CodeEditor",
        workspace: {
          roots: ["/work/shop"],
          activeFile: "/work/shop/package.json",
          visibleRanges: [{ start: 0, end: 9 }],
          visibleText: '"express": "^5.1.0"',
          selections: [],
          openFiles: [],
          pointer: {
            file: "/work/shop/package.json",
            line: 0,
            word: "express",
            lineText: '"express": "^5.1.0"',
          },
          uriScheme: "vscode",
        },
      },
    };
    render(<PackDetails view={code} />);
    expect(screen.getByTestId("pointer").textContent).toBe(
      "Pointer“express”, line 1",
    );
    const section = screen.getByTestId("workspace");
    expect(section.textContent).toContain("/work/shop/package.json");
    expect(section.textContent).toContain("1–10");
    expect(section.textContent).toContain('"express"');
  });
});
