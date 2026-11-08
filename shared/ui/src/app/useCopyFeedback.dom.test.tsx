// @vitest-environment jsdom

import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { installClipboardWriter } from "./clipboard";
import { useCopyFeedback } from "./useCopyFeedback";

afterEach(() => {
  installClipboardWriter(() => Promise.reject(new Error("no clipboard in tests")));
});

describe("useCopyFeedback", () => {
  it("reports a copy for a moment and a refused copy a little longer", async () => {
    const written: string[] = [];
    installClipboardWriter(async (text) => { written.push(text); });
    const view = renderHook(() => useCopyFeedback(300));

    act(() => view.result.current.copy("RJ01"));
    await waitFor(() => expect(view.result.current.result).toBe("copied"));
    expect(written).toEqual(["RJ01"]);
    await waitFor(() => expect(view.result.current.result).toBeNull(), { timeout: 1_500 });

    installClipboardWriter(() => Promise.reject(new Error("refused")));
    act(() => view.result.current.copy("RJ02"));
    await waitFor(() => expect(view.result.current.result).toBe("failed"));
    await waitFor(() => expect(view.result.current.result).toBeNull(), { timeout: 2_500 });
  });
});
