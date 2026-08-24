// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KeyBindingsProvider, useBoundKeys } from "./KeyBindingsProvider";

afterEach(() => {
  cleanup();
});

describe("useBoundKeys", () => {
  it("does not fire media shortcuts from an interactive control or its children", () => {
    const playPause = vi.fn();
    render(
      <KeyBindingsProvider>
        <ShortcutProbe playPause={playPause} />
      </KeyBindingsProvider>,
    );

    const icon = screen.getByRole("button", { name: "Player control" }).querySelector("svg");
    if (!icon) throw new Error("test icon was not rendered");
    fireEvent.keyDown(icon, { key: " " });

    expect(playPause).not.toHaveBeenCalled();
  });

  it("handles the same shortcut outside interactive controls", () => {
    const playPause = vi.fn();
    render(
      <KeyBindingsProvider>
        <ShortcutProbe playPause={playPause} />
      </KeyBindingsProvider>,
    );

    fireEvent.keyDown(window, { key: " " });

    expect(playPause).toHaveBeenCalledOnce();
  });
});

function ShortcutProbe({ playPause }: { playPause: () => void }) {
  useBoundKeys("video", { videoPlayPause: playPause });
  return (
    <button type="button" aria-label="Player control">
      <svg aria-hidden="true" />
    </button>
  );
}
