import { describe, expect, it } from "vitest";

import type { CatalogWork } from "./types";
import { dateMarker, groupWorksByTimeline } from "./catalogTimeline";

const work = (code: string, releaseDate: string): CatalogWork => ({
  code,
  sourceCode: "DL",
  title: code,
  titleEnglish: "",
  addedDate: "2026-08-01",
  releaseDate,
  updatedDate: "2026-08-02",
  ageRating: "r18",
  releaseType: "digital",
  mainImageUrls: [],
  thumbnailUrls: [],
  circles: [],
  categories: [],
  tags: [],
  synthetic: false,
});

describe("catalog timeline", () => {
  it("groups works by release day and keeps the incoming order within each day", () => {
    const groups = groupWorksByTimeline(
      [work("B", "2026-07-30"), work("A", "2026-07-30"), work("C", "2026-07-28")],
      "release",
      "title_asc",
    );

    expect(groups.map((group) => group.key)).toEqual(["2026-07-30", "2026-07-28"]);
    expect(groups[0]?.works.map((item) => item.code)).toEqual(["B", "A"]);
  });

  it("orders release groups oldest first only for the ascending release sort", () => {
    const works = [work("B", "2026-07-30"), work("A", "2025-01-02")];

    expect(groupWorksByTimeline(works, "release", "release_desc").map((group) => group.key)).toEqual([
      "2026-07-30",
      "2025-01-02",
    ]);
    expect(groupWorksByTimeline(works, "release", "release_asc").map((group) => group.key)).toEqual([
      "2025-01-02",
      "2026-07-30",
    ]);
  });

  it("keeps undated works in a final explicit group", () => {
    const groups = groupWorksByTimeline(
      [work("UNKNOWN", ""), work("DATED", "2026-07-30")],
      "release",
      "release_asc",
    );

    expect(groups.map((group) => group.key)).toEqual(["2026-07-30", "unknown"]);
  });

  it("keeps synthetic chronology probes after real catalog groups", () => {
    const synthetic = { ...work("FUTURE", "2099-12-31"), synthetic: true };
    const groups = groupWorksByTimeline(
      [work("REAL", "2026-07-30"), synthetic],
      "release",
      "release_desc",
    );

    expect(groups.map((group) => group.key)).toEqual(["2026-07-30", "2099-12-31"]);
    expect(groups[1]?.syntheticOnly).toBe(true);
  });

  it("groups the same works by the selected persisted timeline", () => {
    const works = [
      { ...work("A", "2020-01-01"), addedDate: "2026-07-01", updatedDate: "2026-07-05" },
      { ...work("B", "2020-01-01"), addedDate: "2026-07-02", updatedDate: "2026-07-05" },
    ];

    expect(groupWorksByTimeline(works, "added", "release_desc").map((group) => group.key)).toEqual([
      "2026-07-02",
      "2026-07-01",
    ]);
    expect(groupWorksByTimeline(works, "updated", "release_desc")[0]?.works).toHaveLength(2);
  });

  it("formats date markers without local timezone drift", () => {
    expect(dateMarker("2026-07-30", "en", { undated: "Undated", unknownWeekday: "Unknown" })).toEqual({
      day: "30",
      month: "Jul",
      weekday: "Thu",
    });
  });
});
