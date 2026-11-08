// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { installClipboardWriter } from "../../app/clipboard";
import { PresentationProvider } from "../../preferences/PresentationProvider";
import { WorkCodeCopy } from "./WorkDetailPage";

afterEach(cleanup);

function renderChip() {
  render(
    <PresentationProvider>
      <WorkCodeCopy code="RJ01109096" />
    </PresentationProvider>,
  );
  return screen.getByRole("button", { name: "Copy work code: RJ01109096" });
}

describe("WorkCodeCopy", () => {
  it("copies the code through the app's clipboard when the chip's text is clicked", async () => {
    const write = vi.fn(() => Promise.resolve());
    installClipboardWriter(write);
    const chip = renderChip();

    fireEvent.click(screen.getByText(/UID: RJ01109096/));

    await waitFor(() => expect(chip.className).toBe("work-code copied"));
    expect(write).toHaveBeenCalledWith("RJ01109096");
    expect(screen.getByText("Work code copied")).toBeTruthy();
  });

  it("says when the copy was refused instead of doing nothing", async () => {
    installClipboardWriter(() => Promise.reject(new Error("Document is not focused.")));
    const chip = renderChip();

    fireEvent.click(chip);

    await waitFor(() => expect(chip.className).toBe("work-code failed"));
    expect(chip.title).toBe("Couldn't copy the work code");
    expect(screen.getByText("Couldn't copy the work code")).toBeTruthy();
  });
});
