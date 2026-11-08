// @vitest-environment jsdom

import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useGatewaySubscription } from "./useGatewaySubscription";

describe("useGatewaySubscription", () => {
  it("delivers values to the latest listener and releases the subscription on unmount", async () => {
    let emit: ((value: number) => void) | undefined;
    const release = vi.fn();
    const subscribe = vi.fn(async (listener: (value: number) => void) => {
      emit = listener;
      return release;
    });
    const seen: string[] = [];
    const view = renderHook(
      ({ label }: { label: string }) => useGatewaySubscription(subscribe, (value) => { seen.push(`${label}:${value}`); }, []),
      { initialProps: { label: "first" } },
    );
    await vi.waitFor(() => expect(emit).toBeTypeOf("function"));

    emit?.(1);
    view.rerender({ label: "second" });
    emit?.(2);
    expect(seen).toEqual(["first:1", "second:2"]);
    expect(subscribe).toHaveBeenCalledOnce();

    view.unmount();
    expect(release).toHaveBeenCalledOnce();
    emit?.(3);
    expect(seen).toEqual(["first:1", "second:2"]);
  });

  it("releases a subscription that completes after unmount and skips a missing one", async () => {
    let settle: ((release: () => void) => void) | undefined;
    const release = vi.fn();
    const subscribe = vi.fn(() => new Promise<() => void>((done) => { settle = done; }));
    const view = renderHook(() => useGatewaySubscription(subscribe, () => undefined, []));
    view.unmount();
    settle?.(release);
    await vi.waitFor(() => expect(release).toHaveBeenCalledOnce());

    const idle = renderHook(() => useGatewaySubscription(undefined, () => undefined, []));
    idle.unmount();
    expect(subscribe).toHaveBeenCalledOnce();
  });
});
