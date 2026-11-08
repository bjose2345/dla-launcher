// @vitest-environment jsdom

import { renderHook } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useResizeObserver } from "./useResizeObserver";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("useResizeObserver", () => {
  it("measures once right away and again on every size change until unmount", () => {
    const observed: Element[] = [];
    let notify: ((entries: ResizeObserverEntry[]) => void) | undefined;
    const disconnect = vi.fn();
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: (entries: ResizeObserverEntry[]) => void) { notify = callback; }
      observe(element: Element) { observed.push(element); }
      disconnect = disconnect;
    });
    const element = document.createElement("div");
    const ref = createRef<HTMLDivElement>();
    (ref as { current: HTMLDivElement | null }).current = element;
    const onResize = vi.fn();

    const view = renderHook(() => useResizeObserver(ref, onResize));

    expect(onResize).toHaveBeenCalledWith(element, null);
    expect(observed).toEqual([element]);
    const entry = { contentRect: { width: 320, height: 40 } } as ResizeObserverEntry;
    notify?.([entry]);
    expect(onResize).toHaveBeenLastCalledWith(element, entry);

    view.unmount();
    expect(disconnect).toHaveBeenCalledOnce();
  });

  it("still measures once where ResizeObserver is missing and skips an absent element", () => {
    vi.stubGlobal("ResizeObserver", undefined);
    const element = document.createElement("div");
    const present = createRef<HTMLDivElement>();
    (present as { current: HTMLDivElement | null }).current = element;
    const onResize = vi.fn();
    renderHook(() => useResizeObserver(present, onResize));
    expect(onResize).toHaveBeenCalledTimes(1);

    const absent = createRef<HTMLDivElement>();
    const never = vi.fn();
    renderHook(() => useResizeObserver(absent, never));
    expect(never).not.toHaveBeenCalled();
  });
});
