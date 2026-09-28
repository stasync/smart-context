import type { AnswerEvent, Effort } from "../shared/ipc";

export type Turn = {
  turn: number;
  effort: Effort;
  /** What the user typed; null for the default question and Go deeper. */
  prompt: string | null;
  text: string;
  status: string | null;
  done: boolean;
  truncated: boolean;
  error: { message: string; needsSetup: boolean } | null;
};

export type PopoverState = {
  /** Reading the screen, before the conversation starts. */
  preparing: boolean;
  askMode: boolean;
  conversation: number | null;
  ceiling: Effort;
  /** Ask mode: nothing asked yet. */
  awaitingQuestion: boolean;
  turns: Turn[];
};

export const initialState: PopoverState = {
  preparing: false,
  askMode: false,
  conversation: null,
  ceiling: "high",
  awaitingQuestion: false,
  turns: [],
};

/** Folds the orchestrator's events into what the popover shows. */
export function reduce(state: PopoverState, event: AnswerEvent): PopoverState {
  switch (event.kind) {
    case "preparing":
      return {
        ...initialState,
        preparing: true,
        askMode: event.askMode,
        awaitingQuestion: event.askMode,
      };
    case "started":
      return {
        ...state,
        preparing: false,
        conversation: event.conversation,
        ceiling: event.ceiling,
        awaitingQuestion: event.awaitingQuestion,
        turns: [],
      };
  }
  // Everything else belongs to one conversation; ignore stale ones.
  if (event.conversation !== state.conversation) return state;
  if (event.kind === "turnStarted") {
    return {
      ...state,
      awaitingQuestion: false,
      turns: [
        ...state.turns,
        {
          turn: event.turn,
          effort: event.effort,
          prompt: event.prompt,
          text: "",
          status: null,
          done: false,
          truncated: false,
          error: null,
        },
      ],
    };
  }
  return {
    ...state,
    turns: state.turns.map((t) => {
      if (t.turn !== event.turn) return t;
      switch (event.kind) {
        case "delta":
          return { ...t, text: t.text + event.text };
        case "status":
          return { ...t, status: event.text };
        case "turnDone":
          return { ...t, done: true, status: null, truncated: event.truncated };
        case "failed":
          return {
            ...t,
            done: true,
            status: null,
            error: { message: event.message, needsSetup: event.needsSetup },
          };
      }
    }),
  };
}
