import { describe, expect, it } from "vitest";
import { splitTarget } from "./target";

describe("splitTarget", () => {
  it("splits the TARGET line from the answer", () => {
    expect(splitTarget("TARGET: Echo Dot title\n\nA smart speaker.")).toEqual({
      target: "Echo Dot title",
      body: "A smart speaker.",
      pending: false,
    });
  });

  it("waits while the TARGET line streams in", () => {
    expect(splitTarget("TAR").pending).toBe(true);
    expect(splitTarget("TARGET: Echo D")).toEqual({
      target: "Echo D",
      body: "",
      pending: true,
    });
    expect(splitTarget("").pending).toBe(true);
  });

  it("treats text without a TARGET line as the whole answer", () => {
    expect(splitTarget("A smart speaker.")).toEqual({
      target: null,
      body: "A smart speaker.",
      pending: false,
    });
  });

  it("tolerates leading whitespace and an empty target", () => {
    expect(splitTarget("\n TARGET:\nBody").target).toBeNull();
    expect(splitTarget("\n TARGET:\nBody").body).toBe("Body");
  });
});
