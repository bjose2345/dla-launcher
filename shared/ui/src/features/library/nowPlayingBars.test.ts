import { describe, expect, it } from "vitest";

import {
  envelopeLevel,
  spectrumBarCount,
  spectrumBarHeights,
} from "./NowPlayingBars";

describe("spectrumBarHeights", () => {
  it("produces one height per bar, normalized to 0..1", () => {
    const heights = spectrumBarHeights(new Uint8Array(128).fill(255), 8);
    expect(heights).toHaveLength(8);
    expect(Math.max(...heights)).toBeLessThanOrEqual(1);
    expect(Math.min(...heights)).toBeGreaterThanOrEqual(0);
  });

  it("is silent when the spectrum is silent", () => {
    expect(spectrumBarHeights(new Uint8Array(128), 6)).toEqual([0, 0, 0, 0, 0, 0]);
  });

  it("follows energy across the spectrum rather than averaging it flat", () => {
    const frequencies = new Uint8Array(128);
    frequencies.fill(255, 0, 16);
    const heights = spectrumBarHeights(frequencies, 8);
    expect(heights[0]).toBeGreaterThan(0.9);
    expect(heights[7]).toBe(0);
  });

  it("takes the peak of each band so short transients still show", () => {
    const frequencies = new Uint8Array(64);
    frequencies[3] = 200;
    const heights = spectrumBarHeights(frequencies, 4);
    expect(heights[0]).toBeGreaterThan(0.7);
  });

  it("handles a zero bar count", () => {
    expect(spectrumBarHeights(new Uint8Array(32), 0)).toEqual([]);
  });
});

describe("spectrumBarCount", () => {
  it("scales the bar count with the available width", () => {
    expect(spectrumBarCount(700)).toBe(100);
    expect(spectrumBarCount(350)).toBe(50);
  });

  it("stays within sane bounds at any resolution", () => {
    expect(spectrumBarCount(40)).toBe(12);
    expect(spectrumBarCount(4000)).toBe(128);
  });

  it("survives a container that has not been measured yet", () => {
    expect(spectrumBarCount(0)).toBe(12);
    expect(spectrumBarCount(Number.NaN)).toBe(12);
  });
});

describe("envelopeLevel", () => {
  it("attacks instantly so transients are not smeared", () => {
    expect(envelopeLevel(0.1, 0.9, 0.82)).toBe(0.9);
  });

  it("releases gradually instead of dropping to silence", () => {
    const level = envelopeLevel(1, 0, 0.82);
    expect(level).toBeCloseTo(0.82, 5);
    expect(level).toBeLessThan(1);
  });

  it("settles toward the target when held", () => {
    let level = 1;
    for (let frame = 0; frame < 40; frame += 1) level = envelopeLevel(level, 0, 0.82);
    expect(level).toBeLessThan(0.01);
  });
});
