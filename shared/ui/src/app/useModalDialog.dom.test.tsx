// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useModalDialog } from "./useModalDialog";

beforeEach(() => {
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => window.setTimeout(() => callback(0), 0));
  vi.stubGlobal("cancelAnimationFrame", (handle: number) => window.clearTimeout(handle));
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function Dialog({ name, onClose, children }: { name: string; onClose?: () => void; children?: ReactNode }) {
  const dialog = useRef<HTMLDivElement>(null);
  useModalDialog({ dialog, onClose });
  return <div ref={dialog} role="dialog" aria-label={name} tabIndex={-1}>{children}</div>;
}

const settle = () => act(() => new Promise<void>((resolve) => { window.setTimeout(resolve, 0); }));

describe("useModalDialog", () => {
  it("gives Escape to the dialog on top, stops the page scrolling, and hands focus back", async () => {
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();
    const closeOuter = vi.fn();
    const closeInner = vi.fn();
    const view = render(<Dialog name="outer" onClose={closeOuter} />);
    await settle();
    expect(document.activeElement).toBe(screen.getByRole("dialog", { name: "outer" }));
    expect(document.body.style.overflow).toBe("hidden");

    const inner = render(<Dialog name="inner" onClose={closeInner} />);
    await settle();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(closeInner).toHaveBeenCalledOnce();
    expect(closeOuter).not.toHaveBeenCalled();

    inner.unmount();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(closeOuter).toHaveBeenCalledOnce();

    view.unmount();
    expect(document.activeElement).toBe(opener);
    expect(document.body.style.overflow).toBe("");
    opener.remove();
  });

  it("keeps Tab inside the dialog", async () => {
    render(
      <Dialog name="form">
        <button type="button">first</button>
        <button type="button">last</button>
      </Dialog>,
    );
    await settle();
    const first = screen.getByRole("button", { name: "first" });
    const last = screen.getByRole("button", { name: "last" });

    last.focus();
    fireEvent.keyDown(window, { key: "Tab" });
    expect(document.activeElement).toBe(first);
    fireEvent.keyDown(window, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(last);

    document.body.focus();
    fireEvent.keyDown(window, { key: "Tab" });
    expect(document.activeElement).toBe(first);
  });

  it("leaves Escape alone for a dialog with its own close key", async () => {
    render(<Dialog name="reader" />);
    await settle();
    const event = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  });
});
