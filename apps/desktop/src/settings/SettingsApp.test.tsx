import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SettingsApp } from "./SettingsApp";

describe("SettingsApp", () => {
  it("shows the app name as the heading", () => {
    render(<SettingsApp />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Context",
    );
  });
});
