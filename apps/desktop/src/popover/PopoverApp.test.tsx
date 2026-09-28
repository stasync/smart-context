import { act, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AnswerEvent } from "../shared/ipc";
import { PopoverApp } from "./PopoverApp";

let emit: (event: AnswerEvent) => void = () => {};

vi.mock("../shared/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../shared/ipc")>()),
  onAnswerEvent: vi.fn((handler: (event: AnswerEvent) => void) => {
    emit = handler;
    return Promise.resolve(() => {});
  }),
  popoverGoDeeper: vi.fn(() => Promise.resolve()),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

describe("PopoverApp", () => {
  it("shows the target in the header and the answer below", () => {
    render(<PopoverApp />);
    act(() => {
      emit({
        kind: "started",
        conversation: 1,
        ceiling: "high",
        awaitingQuestion: false,
      });
      emit({
        kind: "turnStarted",
        conversation: 1,
        turn: 1,
        effort: "low",
        prompt: null,
      });
      emit({
        kind: "delta",
        conversation: 1,
        turn: 1,
        text: "TARGET: The Echo Dot title\n\nA **smart speaker**.",
      });
      emit({ kind: "turnDone", conversation: 1, turn: 1, truncated: false });
    });
    expect(screen.getByText("The Echo Dot title")).toBeTruthy();
    expect(screen.getByTestId("turn").textContent).toBe("A smart speaker.");
    expect(
      (screen.getByRole("button", { name: "Go deeper" }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
  });

  it("offers Settings when the engine needs setup", () => {
    render(<PopoverApp />);
    act(() => {
      emit({
        kind: "started",
        conversation: 1,
        ceiling: "high",
        awaitingQuestion: false,
      });
      emit({
        kind: "turnStarted",
        conversation: 1,
        turn: 1,
        effort: "low",
        prompt: null,
      });
      emit({
        kind: "failed",
        conversation: 1,
        turn: 1,
        message: "Set up an AI engine in Settings to get answers.",
        needsSetup: true,
      });
    });
    expect(screen.getByRole("button", { name: "Open Settings" })).toBeTruthy();
  });

  it("stops going deeper at the ceiling", () => {
    render(<PopoverApp />);
    act(() => {
      emit({
        kind: "started",
        conversation: 1,
        ceiling: "medium",
        awaitingQuestion: false,
      });
      emit({
        kind: "turnStarted",
        conversation: 1,
        turn: 1,
        effort: "medium",
        prompt: null,
      });
      emit({ kind: "turnDone", conversation: 1, turn: 1, truncated: false });
    });
    expect(
      (screen.getByRole("button", { name: "Go deeper" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    const options = screen.getAllByRole("option").map((o) => o.textContent);
    expect(options).toEqual(["Low", "Medium"]);
  });
});
