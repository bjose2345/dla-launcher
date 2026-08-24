import { describe, expect, it } from "vitest";

import {
  clampPlaybackPosition,
  formatPlaybackTime,
  resumePlaybackPosition,
  restorePlaybackPosition,
} from "./audioPlayer";

describe("audio player presentation", () => {
  it("formats elapsed, remaining, and long-form playback times", () => {
    expect(formatPlaybackTime(0)).toBe("0:00");
    expect(formatPlaybackTime(223.9)).toBe("3:43");
    expect(formatPlaybackTime(367.2)).toBe("6:07");
    expect(formatPlaybackTime(3_661)).toBe("1:01:01");
    expect(formatPlaybackTime(null)).toBe("--:--");
  });

  it("keeps seek positions inside the known media duration", () => {
    expect(clampPlaybackPosition(-15, 120)).toBe(0);
    expect(clampPlaybackPosition(45, 120)).toBe(45);
    expect(clampPlaybackPosition(150, 120)).toBe(120);
    expect(clampPlaybackPosition(Number.NaN, 120)).toBe(0);
    expect(clampPlaybackPosition(45, null)).toBe(45);
  });

  it("restores unfinished playback without resuming at the completed boundary", () => {
    expect(restorePlaybackPosition(223.5, 367)).toBe(223.5);
    expect(restorePlaybackPosition(366.9, 367)).toBe(0);
    expect(restorePlaybackPosition(223.5, 367, true)).toBe(0);
    expect(restorePlaybackPosition(45, null)).toBe(45);
  });

  it("nudges a paused WebKit media pipeline without losing the saved position", () => {
    expect(resumePlaybackPosition(223.5, 367)).toBe(223.501);
    expect(resumePlaybackPosition(0, null)).toBe(0.001);
    expect(resumePlaybackPosition(366.99, 367)).toBe(0);
    expect(resumePlaybackPosition(223.5, 367, true)).toBe(0);
  });
});
