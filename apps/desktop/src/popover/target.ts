const PREFIX = "TARGET:";

export type Split = {
  /** What the user pointed at, from the model's TARGET line. */
  target: string | null;
  /** The answer without the TARGET line. */
  body: string;
  /** The TARGET line is still streaming in. */
  pending: boolean;
};

/**
 * Separates the model's first line, `TARGET: …`, from the answer
 * (docs/PLAN.md 4.6). While the line streams in, nothing is shown yet;
 * without one, the whole text is the answer.
 */
export function splitTarget(text: string): Split {
  const start = text.replace(/^\s+/, "");
  if (!start.startsWith(PREFIX)) {
    const couldStillBe =
      start.length < PREFIX.length && PREFIX.startsWith(start);
    return couldStillBe
      ? { target: null, body: "", pending: true }
      : { target: null, body: text, pending: false };
  }
  const newline = start.indexOf("\n");
  if (newline === -1) {
    return {
      target: start.slice(PREFIX.length).trim() || null,
      body: "",
      pending: true,
    };
  }
  return {
    target: start.slice(PREFIX.length, newline).trim() || null,
    body: start.slice(newline + 1).replace(/^\s*\n/, ""),
    pending: false,
  };
}
