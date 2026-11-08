import { describe, expect, it } from "vitest";

import { settleNativeSeek } from "./VideoPlayer";

describe("settleNativeSeek", () => {
  const pending = { target: 40, at: 1_000, preview: false };

  it("passes native positions through when no seek is pending", () => {
    expect(settleNativeSeek(null, 12, 1_000)).toEqual({ position: 12, pending: null });
  });

  it("holds the target while the native position is still far from it", () => {
    expect(settleNativeSeek(pending, 12, 1_200)).toEqual({ position: 40, pending });
    expect(settleNativeSeek(pending, 0, 1_200)).toEqual({ position: 40, pending });
  });

  it("settles on a native position near the target", () => {
    expect(settleNativeSeek(pending, 39.4, 1_200)).toEqual({ position: 39.4, pending: null });
  });

  it("gives up holding after the wait runs out", () => {
    expect(settleNativeSeek(pending, 12, 5_000)).toEqual({ position: 12, pending: null });
  });

  it("holds through a scrub's keyframe landings until released or timed out", () => {
    const preview = { ...pending, preview: true };
    expect(settleNativeSeek(preview, 39.9, 1_200)).toEqual({ position: 40, pending: preview });
    expect(settleNativeSeek(preview, 39.9, 5_000)).toEqual({ position: 39.9, pending: null });
  });
});
