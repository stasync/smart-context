import { describe, expect, it } from "vitest";
import type { AnswerEvent } from "../shared/ipc";
import { initialState, reduce } from "./state";

const run = (events: AnswerEvent[]) => events.reduce(reduce, initialState);

describe("popover state", () => {
  it("builds a streamed answer", () => {
    const state = run([
      { kind: "preparing", askMode: false },
      {
        kind: "started",
        conversation: 1,
        ceiling: "high",
        awaitingQuestion: false,
      },
      {
        kind: "turnStarted",
        conversation: 1,
        turn: 1,
        effort: "low",
        prompt: null,
      },
      { kind: "status", conversation: 1, turn: 1, text: "Thinking…" },
      { kind: "delta", conversation: 1, turn: 1, text: "TARGET: x\n" },
      { kind: "delta", conversation: 1, turn: 1, text: "Hello" },
      { kind: "turnDone", conversation: 1, turn: 1, truncated: false },
    ]);
    expect(state.preparing).toBe(false);
    expect(state.turns).toHaveLength(1);
    expect(state.turns[0]).toMatchObject({
      text: "TARGET: x\nHello",
      done: true,
      status: null,
    });
  });

  it("ignores events from an older conversation", () => {
    const state = run([
      {
        kind: "started",
        conversation: 2,
        ceiling: "high",
        awaitingQuestion: false,
      },
      {
        kind: "turnStarted",
        conversation: 2,
        turn: 1,
        effort: "low",
        prompt: null,
      },
      { kind: "delta", conversation: 1, turn: 1, text: "stale" },
    ]);
    expect(state.turns[0].text).toBe("");
  });

  it("keeps ask mode waiting until the first question", () => {
    let state = run([
      { kind: "preparing", askMode: true },
      {
        kind: "started",
        conversation: 1,
        ceiling: "max",
        awaitingQuestion: true,
      },
    ]);
    expect(state.awaitingQuestion).toBe(true);
    expect(state.askMode).toBe(true);
    state = reduce(state, {
      kind: "turnStarted",
      conversation: 1,
      turn: 1,
      effort: "low",
      prompt: "Is it waterproof?",
    });
    expect(state.awaitingQuestion).toBe(false);
  });

  it("records failures on their turn", () => {
    const state = run([
      {
        kind: "started",
        conversation: 1,
        ceiling: "high",
        awaitingQuestion: false,
      },
      {
        kind: "turnStarted",
        conversation: 1,
        turn: 1,
        effort: "low",
        prompt: null,
      },
      {
        kind: "failed",
        conversation: 1,
        turn: 1,
        message: "The API key was rejected.",
        needsSetup: true,
      },
    ]);
    expect(state.turns[0].error).toEqual({
      message: "The API key was rejected.",
      needsSetup: true,
    });
  });
});
