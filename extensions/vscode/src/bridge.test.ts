import { describe, expect, it } from "vitest";
import {
  bridgeFilePath,
  capOpenFiles,
  capRanges,
  capSelection,
  nextDelay,
  pointerAt,
  visibleText,
} from "./bridge";

describe("bridge helpers", () => {
  it("finds bridge.json in the app's data folder", () => {
    expect(bridgeFilePath("darwin", "/Users/me", {})).toBe(
      "/Users/me/Library/Application Support/dev.context.app/bridge.json",
    );
    expect(bridgeFilePath("linux", "/home/me", {})).toBe(
      "/home/me/.local/share/dev.context.app/bridge.json",
    );
    expect(
      bridgeFilePath("win32", "C:/Users/me", {
        APPDATA: "C:/Users/me/AppData/Roaming",
      }),
    ).toContain("dev.context.app");
  });

  it("caps the visible lines across ranges", () => {
    expect(capRanges([{ start: 0, end: 9 }], 400)).toEqual([
      { start: 0, end: 9 },
    ]);
    expect(
      capRanges(
        [
          { start: 0, end: 299 },
          { start: 500, end: 799 },
        ],
        400,
      ),
    ).toEqual([
      { start: 0, end: 299 },
      { start: 500, end: 599 },
    ]);
  });

  it("joins the visible lines in order", () => {
    const lines = ["a", "b", "c", "d", "e"];
    expect(
      visibleText(
        (n) => lines[n],
        [
          { start: 0, end: 1 },
          { start: 3, end: 4 },
        ],
      ),
    ).toBe("a\nb\nd\ne");
  });

  it("caps selections and open files", () => {
    expect(capSelection("x".repeat(5000))).toHaveLength(4000);
    expect(capOpenFiles(["a", "a", "b"])).toEqual(["a", "b"]);
  });

  it("backs off up to 30 seconds", () => {
    expect([0, 1, 2, 5, 10].map(nextDelay)).toEqual([
      1000, 2000, 4000, 30000, 30000,
    ]);
  });

  it("caps what a pointer carries", () => {
    const pointer = pointerAt(
      "/p/a.ts",
      3,
      7,
      "w".repeat(300),
      "x".repeat(900),
      5,
    );
    expect(pointer.word).toHaveLength(200);
    expect(pointer.lineText).toHaveLength(500);
    expect(pointer).toMatchObject({
      file: "/p/a.ts",
      line: 3,
      character: 7,
      at: 5,
    });
  });
});
