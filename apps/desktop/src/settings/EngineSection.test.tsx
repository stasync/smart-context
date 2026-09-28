import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as ipc from "../shared/ipc";
import { EngineSection } from "./EngineSection";

vi.mock("../shared/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../shared/ipc")>()),
  engineStatus: vi.fn(),
  effortCeiling: vi.fn(() => Promise.resolve("high")),
  saveApiKey: vi.fn(() => Promise.resolve()),
  removeApiKey: vi.fn(() => Promise.resolve()),
  setEffortCeiling: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

const info = {
  id: "engine",
  name: "Test engine",
  keyLabel: "Test key",
  keyHelp: "Make a key with a spend limit.",
  keyHelpUrl: "https://example.com/keys",
};

describe("EngineSection", () => {
  beforeEach(() => vi.clearAllMocks());

  it("saves a pasted key and never shows it back", async () => {
    vi.mocked(ipc.engineStatus)
      .mockResolvedValueOnce({
        info,
        readiness: { state: "needsSetup", reason: "" },
      })
      .mockResolvedValueOnce({ info, readiness: { state: "ready" } });
    render(<EngineSection />);

    const field = (await screen.findByLabelText(
      "Test key",
    )) as HTMLInputElement;
    expect(field.type).toBe("password");
    fireEvent.change(field, { target: { value: "secret-123" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await screen.findByText("Test key saved");
    expect(ipc.saveApiKey).toHaveBeenCalledWith("secret-123");
    expect(document.body.textContent).not.toContain("secret-123");
  });

  it("changes the effort ceiling", async () => {
    vi.mocked(ipc.engineStatus).mockResolvedValue({
      info,
      readiness: { state: "ready" },
    });
    render(<EngineSection />);
    const select = await screen.findByLabelText("Effort ceiling");
    fireEvent.change(select, { target: { value: "max" } });
    await waitFor(() =>
      expect(ipc.setEffortCeiling).toHaveBeenCalledWith("max"),
    );
  });
});
