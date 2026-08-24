import { describe, expect, it } from "vitest";

import { nearestTargetIndex, railEdgeMask, railPageTargets } from "./ContentCarousel";

describe("railPageTargets", () => {
  it("returns no targets when the rail does not overflow", () => {
    expect(railPageTargets(300, 1200, 0)).toEqual([]);
    expect(railPageTargets(300, 1200, 4)).toEqual([]);
  });

  it("returns no targets without a measurable card step", () => {
    expect(railPageTargets(0, 1200, 900)).toEqual([]);
  });

  it("pages by whole cards and always ends flush", () => {
    expect(railPageTargets(300, 1200, 900)).toEqual([0, 900]);
    expect(railPageTargets(300, 1200, 1800)).toEqual([0, 1200, 1800]);
  });

  it("keeps a full page before the flush stop so no card is skipped", () => {
    expect(railPageTargets(300, 900, 2100)).toEqual([0, 900, 1800, 2100]);
  });
});

describe("nearestTargetIndex", () => {
  it("selects the closest target", () => {
    expect(nearestTargetIndex([0, 900, 1800], 0)).toBe(0);
    expect(nearestTargetIndex([0, 900, 1800], 500)).toBe(1);
    expect(nearestTargetIndex([0, 900, 1800], 1700)).toBe(2);
  });

  it("keeps the first target when the list holds one stop", () => {
    expect(nearestTargetIndex([0], 4000)).toBe(0);
  });
});

describe("railEdgeMask", () => {
  it("fades only the sides that still hold content", () => {
    expect(railEdgeMask(false, false)).toBeUndefined();
    expect(railEdgeMask(false, true)).toContain("calc(100% - 36px)");
    expect(railEdgeMask(true, false)).toBe("linear-gradient(to right, transparent, #000 36px)");
    expect(railEdgeMask(true, true)).toContain("transparent, #000 36px");
  });
});
