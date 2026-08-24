import { describe, expect, it } from "vitest";

import {
  catalogGridPageSize,
  catalogLinePreviewSize,
  catalogPageLinks,
  catalogPageRange,
  catalogPageSlice,
} from "./catalogPagination";

describe("catalog pagination", () => {
  it("uses the same fixed 24-work page size as the archive grid", () => {
    expect(catalogGridPageSize).toBe(24);
    expect(catalogPageSlice(Array.from({ length: 60 }, (_, index) => index + 1), 2)).toEqual(
      Array.from({ length: 24 }, (_, index) => index + 25),
    );
  });

  it("keeps line days collapsed to the archive's initial 12-card preview", () => {
    expect(catalogLinePreviewSize).toBe(12);
  });

  it("builds compact edge, neighborhood, and ellipsis links", () => {
    expect(catalogPageLinks(1, 21)).toEqual([1, 2, 3, "…right", 21]);
    expect(catalogPageLinks(11, 21)).toEqual([1, "…left", 9, 10, 11, 12, 13, "…right", 21]);
    expect(catalogPageLinks(21, 21)).toEqual([1, "…left", 19, 20, 21]);
  });

  it("reports the visible range for full, final, and empty pages", () => {
    expect(catalogPageRange(1, 24, 496)).toEqual({ from: 1, to: 24 });
    expect(catalogPageRange(21, 16, 496)).toEqual({ from: 481, to: 496 });
    expect(catalogPageRange(1, 0, 0)).toEqual({ from: 0, to: 0 });
  });
});
