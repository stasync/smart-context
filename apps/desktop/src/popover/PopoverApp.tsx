import { useEffect, useReducer, useRef, useState } from "react";
import {
  EFFORT_LABELS,
  EFFORTS,
  onAnswerEvent,
  openSettings,
  popoverAsk,
  popoverClose,
  popoverCorrect,
  popoverGoDeeper,
  popoverSent,
  popoverSetEffort,
  type Effort,
  type Sent,
} from "../shared/ipc";
import { PackDetails } from "../shared/PackDetails";
import { Markdown } from "./Markdown";
import { initialState, reduce, type Turn } from "./state";
import { splitTarget } from "./target";

export function PopoverApp() {
  const [state, dispatch] = useReducer(reduce, initialState);
  useEffect(() => {
    const unlisten = onAnswerEvent(dispatch);
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);
  // A fresh popover per conversation resets everything local.
  return <Popover key={state.conversation ?? "new"} state={state} />;
}

type Props = { state: ReturnType<typeof reduce> };

function Popover({ state }: Props) {
  const [question, setQuestion] = useState("");
  const [editingTarget, setEditingTarget] = useState(false);
  const [correction, setCorrection] = useState("");
  const [sent, setSent] = useState<Sent | null>(null);
  const [copied, setCopied] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const body = useRef<HTMLElement>(null);

  const first = state.turns[0];
  const last = state.turns.at(-1);
  const busy = last !== undefined && !last.done;
  const target = first ? splitTarget(first.text).target : null;
  const effort: Effort = last?.effort ?? "low";
  const allowed = EFFORTS.slice(0, EFFORTS.indexOf(state.ceiling) + 1);
  const canGoDeeper = !busy && last !== undefined && effort !== state.ceiling;

  // In ask mode the popover has the keyboard: put it in the question box.
  useEffect(() => {
    if (state.awaitingQuestion) input.current?.focus();
  }, [state.awaitingQuestion]);

  // Follow the answer as it streams, unless the user scrolled up.
  useEffect(() => {
    const el = body.current;
    if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 80) {
      el.scrollTop = el.scrollHeight;
    }
  }, [state.turns]);

  const ask = (event: React.FormEvent) => {
    event.preventDefault();
    const text = question.trim();
    if (!text || busy) return;
    setQuestion("");
    void popoverAsk(text);
  };

  const correct = (event: React.FormEvent) => {
    event.preventDefault();
    const text = correction.trim();
    setEditingTarget(false);
    if (text) void popoverCorrect(text);
  };

  const copy = () => {
    if (!last) return;
    const text = last === first ? splitTarget(last.text).body : last.text;
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  const toggleSent = () => {
    if (sent) setSent(null);
    else void popoverSent().then(setSent);
  };

  return (
    <div className="popover">
      <header className="popover__header">
        {editingTarget ? (
          <form className="correction" onSubmit={correct}>
            <input
              autoFocus
              value={correction}
              placeholder="What did you point at?"
              onChange={(e) => setCorrection(e.target.value)}
              onBlur={() => setEditingTarget(false)}
            />
          </form>
        ) : (
          <button
            type="button"
            className="target"
            title="Not what you meant? Click to correct it."
            disabled={!first}
            onClick={() => {
              setCorrection(target ?? "");
              setEditingTarget(true);
            }}
          >
            <span className="muted">You pointed at: </span>
            <strong>{target ?? (first ? "Explaining…" : "…")}</strong>
          </button>
        )}
        <button
          type="button"
          className="close"
          aria-label="Close"
          onClick={() => void popoverClose()}
        >
          ×
        </button>
      </header>

      <main className="popover__body" ref={body}>
        {state.preparing && !state.askMode && (
          <p className="muted">Reading the screen…</p>
        )}
        {state.awaitingQuestion && (
          <p className="muted">Ask anything about what you pointed at.</p>
        )}
        {state.turns.map((turn) => (
          <TurnView key={turn.turn} turn={turn} isFirst={turn === first} />
        ))}
        {sent && (
          <section className="sent" aria-label="What was sent">
            {sent.activity.length > 0 && (
              <>
                <h2>Read by the tools</h2>
                <ul className="activity">
                  {sent.activity.map((item, i) => (
                    <li key={i}>{item}</li>
                  ))}
                </ul>
              </>
            )}
            <PackDetails view={sent.view} />
          </section>
        )}
      </main>

      <footer className="popover__footer">
        {last?.status && <p className="status">{last.status}</p>}
        <div className="actions">
          <button
            type="button"
            disabled={!canGoDeeper}
            onClick={() => void popoverGoDeeper()}
          >
            Go deeper
          </button>
          <button type="button" disabled={!last?.done} onClick={copy}>
            {copied ? "Copied" : "Copy"}
          </button>
          <button type="button" disabled={!first} onClick={toggleSent}>
            {sent ? "Hide what was sent" : "What was sent"}
          </button>
          <select
            aria-label="Effort"
            value={effort}
            disabled={busy || !last}
            onChange={(e) => void popoverSetEffort(e.target.value as Effort)}
          >
            {allowed.map((level) => (
              <option key={level} value={level}>
                {EFFORT_LABELS[level]}
              </option>
            ))}
          </select>
        </div>
        <form className="ask" onSubmit={ask}>
          <input
            ref={input}
            value={question}
            disabled={busy || state.preparing}
            placeholder={
              state.awaitingQuestion
                ? "Ask about what you pointed at…"
                : "Ask a follow-up…"
            }
            onChange={(e) => setQuestion(e.target.value)}
          />
        </form>
      </footer>
    </div>
  );
}

function TurnView({ turn, isFirst }: { turn: Turn; isFirst: boolean }) {
  const text = isFirst ? splitTarget(turn.text).body : turn.text;
  const label =
    turn.prompt ??
    (isFirst ? null : `Go deeper · ${EFFORT_LABELS[turn.effort]}`);
  return (
    <article className="turn" data-testid="turn">
      {label && <p className="turn__prompt">{label}</p>}
      {text && <Markdown text={text} />}
      {!turn.done && !text && !turn.status && <p className="muted">…</p>}
      {turn.truncated && (
        <p className="muted small">The answer was cut short. Try Go deeper.</p>
      )}
      {turn.error && (
        <p className="error">
          {turn.error.message}{" "}
          {turn.error.needsSetup && (
            <button type="button" onClick={() => void openSettings()}>
              Open Settings
            </button>
          )}
        </p>
      )}
    </article>
  );
}
