import { describe, expect, it } from "vitest";

import { balanceGalleryColumns, bentoSpans, galleryColumnCount } from "./galleryLayout";

describe("gallery layout", () => {
  it("uses the same responsive column limits as the Archive gallery", () => {
    expect(galleryColumnCount(520)).toBe(1);
    expect(galleryColumnCount(521)).toBe(2);
    expect(galleryColumnCount(1023)).toBe(2);
    expect(galleryColumnCount(1024)).toBe(3);
  });

  it("keeps source order while balancing native image heights", () => {
    expect(balanceGalleryColumns([1 / 3, 1, 1, 1, 1 / 3], 2)).toEqual([
      [0, 1, 2],
      [3, 4],
    ]);
  });

  it("puts equal-height overflow in earlier columns like sequential CSS columns", () => {
    expect(balanceGalleryColumns(Array.from({ length: 7 }, () => 1), 3)).toEqual([
      [0, 1, 2],
      [3, 4],
      [5, 6],
    ]);
  });

  it("does not create empty columns and normalizes invalid ratios", () => {
    expect(balanceGalleryColumns([0, Number.NaN], 3)).toEqual([[0], [1]]);
    expect(balanceGalleryColumns([], 3)).toEqual([]);
  });
});

describe("bento mosaic", () => {
  it("fills every two-row block, alternating the feature's side", () => {
    expect(bentoSpans(1)).toEqual(["full"]);
    expect(bentoSpans(2)).toEqual(["feature", "feature"]);
    expect(bentoSpans(5)).toEqual(["feature", "single", "single", "single", "single"]);
    expect(bentoSpans(8)).toEqual([
      "feature", "single", "single", "single", "single",
      "feature-r", "wide", "wide",
    ]);
  });

  it("turns a remainder of one into two blocks of three", () => {
    expect(bentoSpans(6)).toEqual(["feature", "wide", "wide", "feature-r", "wide", "wide"]);
    expect(bentoSpans(11)).toHaveLength(11);
    expect(bentoSpans(11).slice(-6)).toEqual(["feature-r", "wide", "wide", "feature", "wide", "wide"]);
  });
});
