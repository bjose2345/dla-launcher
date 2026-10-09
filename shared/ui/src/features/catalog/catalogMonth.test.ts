import { describe, expect, it } from "vitest";

import {
  catalogDayLabel,
  catalogMonthNavigation,
  catalogRailDays,
  catalogRailMonths,
  catalogYearHasMonths,
  reconcileCatalogMonth,
} from "./catalogMonth";

const months = [
  { month: "2026-01", count: 3 },
  { month: "2026-03", count: 8 },
  { month: "2026-06", count: 5 },
];

describe("catalog month navigation", () => {
  it("keeps enabled months and otherwise prefers the nearest earlier bucket", () => {
    expect(reconcileCatalogMonth("2026-03", months, "2026-06")).toBe("2026-03");
    expect(reconcileCatalogMonth("2026-05", months, "2026-06")).toBe("2026-03");
    expect(reconcileCatalogMonth("2025-12", months, "2026-06")).toBe("2026-01");
  });

  it("moves to the newest matching month only when explicitly requested", () => {
    expect(reconcileCatalogMonth("2026-01", months, "2026-06", true)).toBe("2026-06");
    expect(reconcileCatalogMonth("", months, "2026-08")).toBe("2026-06");
    expect(reconcileCatalogMonth("2026-02", [], "2026-08", true)).toBe("2026-02");
    expect(reconcileCatalogMonth("", [], "2026-08")).toBe("2026-08");
  });

  it("skips unavailable months in both directions", () => {
    expect(catalogMonthNavigation("2026-03", months)).toEqual({
      previous: "2026-01",
      next: "2026-06",
    });
  });

  it("turns the month's day buckets into rail cells with fills and ghosts", () => {
    const days = [
      { day: "2026-08-01", count: 10 },
      { day: "2026-08-02", count: 0 },
      { day: "2026-08-03", count: 40 },
    ];
    expect(catalogRailDays(days)).toEqual([
      { day: "2026-08-01", date: 1, count: 10, fill: 0.25 },
      { day: "2026-08-02", date: 2, count: null, fill: 0 },
      { day: "2026-08-03", date: 3, count: 40, fill: 1 },
    ]);
    expect(catalogDayLabel("2026-08-03", "en-US")).toBe("August 3");
    expect(catalogDayLabel("nonsense", "en-US")).toBe("nonsense");
  });
});

describe("catalog month rail", () => {
  it("shows every month of the year with its count, picking only the populated ones in range", () => {
    const rail = catalogRailMonths(2026, "2026-01", "2026-06", months);
    expect(rail).toHaveLength(12);
    expect(rail[0]).toEqual({ month: "2026-01", count: 3, enabled: true });
    expect(rail[1]).toEqual({ month: "2026-02", count: 0, enabled: false });
    expect(rail[6]).toEqual({ month: "2026-07", count: null, enabled: false });
  });

  it("offers a year only when one of its months can be picked", () => {
    expect(catalogYearHasMonths(2026, "2025-11", "2026-06", months)).toBe(true);
    expect(catalogYearHasMonths(2025, "2025-11", "2026-06", months)).toBe(false);
  });
});
