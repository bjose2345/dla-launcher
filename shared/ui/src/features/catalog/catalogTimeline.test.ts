import { describe, expect, it } from "vitest";

import type { CatalogWork } from "./types";
import {
  catalogActivityKind,
  catalogUpdateLabel,
  dateMarker,
  groupWorksByTimeline,
} from "./catalogTimeline";

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

describe("Added + Updated timeline", () => {
  const activity = (addedDate: string, updatedDate: string): CatalogWork => ({
    ...work("W", "2020-01-01"),
    addedDate,
    updatedDate,
  });

  it("places a work at its latest event and says which one it was", () => {
    const updated = activity("2026-07-01", "2026-08-20T10:00:00.000Z");
    const added = activity("2026-08-03", "");
    const sameDay = activity("2026-08-05", "2026-08-05T23:00:00.000Z");

    expect(groupWorksByTimeline([added, updated], "activity", "release_desc").map((group) => group.key))
      .toEqual(["2026-08-20", "2026-08-03"]);
    expect(catalogActivityKind(updated)).toBe("updated");
    expect(catalogActivityKind(added)).toBe("added");
    expect(catalogActivityKind(sameDay)).toBe("added");
  });

  it("labels an update with its version and a short date, adding the year only for older ones", () => {
    const now = new Date("2026-10-08T12:00:00Z");
    expect(catalogUpdateLabel({ date: "2026-09-27", version: "1.3.1" }, "en-US", now)).toBe("v1.3.1 · Sep 27");
    expect(catalogUpdateLabel({ date: "2024-01-05", version: null }, "en-US", now)).toBe("Jan 5, 2024");
  });
});
