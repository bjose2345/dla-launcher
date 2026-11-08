// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { useImageSourceChain } from "./useImageSourceChain";

describe("useImageSourceChain", () => {
  it("moves on when an image fails and starts over with a new list", () => {
    const first = ["a.webp", "b.webp"];
    const view = renderHook(({ urls, key }: { urls: string[]; key: string }) => useImageSourceChain(urls, key), {
      initialProps: { urls: first, key: "RJ1" },
    });
    expect(view.result.current.source).toBe("a.webp");

    act(() => view.result.current.advance());
    expect(view.result.current.source).toBe("b.webp");
    act(() => view.result.current.advance());
    expect(view.result.current.source).toBeUndefined();

    view.rerender({ urls: ["c.webp"], key: "RJ1" });
    expect(view.result.current.source).toBe("c.webp");

    act(() => view.result.current.advance());
    view.rerender({ urls: ["c.webp"], key: "RJ2" });
    expect(view.result.current.source).toBe("c.webp");
  });
});
