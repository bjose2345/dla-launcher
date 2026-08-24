import { describe, expect, it } from "vitest";

import {
  calculateScrollbarGeometry,
  scrollTopForThumbOffset,
} from "./scrollbarMath";

describe("shared catalog scrollbar geometry", () => {
  it("sizes and positions the thumb proportionally", () => {
    const geometry = calculateScrollbarGeometry(
      { scrollTop: 450, scrollHeight: 1200, clientHeight: 300 },
      240,
    );

    expect(geometry.thumbHeight).toBe(60);
    expect(geometry.thumbOffset).toBe(90);
    expect(geometry.maxScroll).toBe(900);
    expect(geometry.maxThumbOffset).toBe(180);
  });

  it("keeps a usable minimum thumb and clamps both ends", () => {
    const geometry = calculateScrollbarGeometry(
      { scrollTop: 5000, scrollHeight: 10000, clientHeight: 200 },
      180,
    );

    expect(geometry.thumbHeight).toBe(36);
    expect(geometry.thumbOffset).toBeLessThanOrEqual(geometry.maxThumbOffset);
    expect(scrollTopForThumbOffset(-50, geometry)).toBe(0);
    expect(scrollTopForThumbOffset(500, geometry)).toBe(geometry.maxScroll);
  });

  it("fills the track when the content does not scroll", () => {
    expect(
      calculateScrollbarGeometry(
        { scrollTop: 0, scrollHeight: 300, clientHeight: 300 },
        220,
      ),
    ).toEqual({
      thumbHeight: 220,
      thumbOffset: 0,
      maxScroll: 0,
      maxThumbOffset: 0,
    });
  });
});
