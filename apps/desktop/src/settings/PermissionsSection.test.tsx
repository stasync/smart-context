import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PermissionsSection } from "./PermissionsSection";

describe("PermissionsSection", () => {
  it("offers to open System Settings for each missing permission", () => {
    const onRequest = vi.fn();
    render(
      <PermissionsSection
        status={{ accessibility: true, screenRecording: false }}
        onRequest={onRequest}
      />,
    );

    expect(screen.getAllByText("Allowed")).toHaveLength(1);
    const buttons = screen.getAllByRole("button", {
      name: "Open System Settings",
    });
    expect(buttons).toHaveLength(1);

    fireEvent.click(buttons[0]);
    expect(onRequest).toHaveBeenCalledWith("screenRecording");
  });

  it("says when everything is allowed", () => {
    render(
      <PermissionsSection
        status={{ accessibility: true, screenRecording: true }}
        onRequest={() => {}}
      />,
    );
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByText(/All set/)).toBeTruthy();
  });

  it("shows both buttons before the status is known", () => {
    render(<PermissionsSection status={null} onRequest={() => {}} />);
    expect(screen.getAllByRole("button")).toHaveLength(2);
  });
});
