import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SettingsApp } from "./SettingsApp";

vi.mock("../shared/ipc", () => ({
  permissionStatus: vi.fn(() =>
    Promise.resolve({ accessibility: false, screenRecording: false }),
  ),
  requestPermission: vi.fn(() => Promise.resolve()),
  openViewer: vi.fn(() => Promise.resolve()),
}));

describe("SettingsApp", () => {
  it("shows the app name as the heading", () => {
    render(<SettingsApp />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Context",
    );
  });
});
