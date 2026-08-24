import { describe, expect, it } from "vitest";

import { formatPlaybackRate, PLAYBACK_RATES } from "./MediaDockMenu";
import { clampPlaybackRate } from "./audioPlayer";

describe("playback rate", () => {
  it("offers the conventional ladder around normal speed", () => {
    expect(PLAYBACK_RATES).toContain(1);
    expect(PLAYBACK_RATES[0]).toBe(0.5);
    expect(PLAYBACK_RATES.at(-1)).toBe(2);
  });

  it("labels whole and fractional rates readably", () => {
    expect(formatPlaybackRate(1)).toBe("1×");
    expect(formatPlaybackRate(1.25)).toBe("1.25×");
  });

  it("clamps to a range a media element will accept", () => {
    expect(clampPlaybackRate(0)).toBe(0.25);
    expect(clampPlaybackRate(99)).toBe(4);
    expect(clampPlaybackRate(Number.NaN)).toBe(1);
    expect(clampPlaybackRate(1.5)).toBe(1.5);
  });
})
