import { describe, expect, it } from "vitest";
import { fileReference } from "./files";

describe("fileReference", () => {
  it("reads paths with optional lines", () => {
    expect(fileReference("src/server.ts:12", false)).toEqual({
      path: "src/server.ts",
      line: 12,
    });
    expect(fileReference("package.json", true)).toEqual({
      path: "package.json",
      line: null,
    });
    expect(fileReference("src/routes/", true)).toEqual({
      path: "src/routes/",
      line: null,
    });
  });

  it("leaves code and outside paths alone", () => {
    for (const text of [
      "app.listen",
      "express.json()",
      "https://x.dev",
      "/etc/hosts",
      "../secret.ts",
      "npm",
    ]) {
      expect(fileReference(text, true), text).toBeNull();
    }
    expect(fileReference("https://expressjs.com", false)).toBeNull();
    expect(fileReference("README", false)).toBeNull();
  });
});
