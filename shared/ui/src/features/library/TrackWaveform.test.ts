import { describe, expect, it } from "vitest";

import {
  resampleWaveformPeaks,
  seekPositionFromPointer,
  waveformBucketCount,
} from "./TrackWaveform";

describe("track waveform", () => {
  it("sizes the cached peak request to the rendered surface", () => {
    expect(waveformBucketCount(0)).toBe(32);
    expect(waveformBucketCount(700)).toBe(100);
    expect(waveformBucketCount(10_000)).toBe(256);
  });

  it("maps and clamps pointer positions to media time", () => {
    expect(seekPositionFromPointer(250, 100, 300, 600)).toBe(300);
    expect(seekPositionFromPointer(0, 100, 300, 600)).toBe(0);
    expect(seekPositionFromPointer(500, 100, 300, 600)).toBe(600);
  });

  it("reduces cached peaks without losing the loudest sample", () => {
    expect(resampleWaveformPeaks([0.1, 0.8, 0.3, 0.6], 2)).toEqual([0.8, 0.6]);
    expect(resampleWaveformPeaks([], 2)).toEqual([0, 0]);
  });
});
