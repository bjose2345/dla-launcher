import { describe, expect, it } from "vitest";

import {
  catalogMonthNavigation,
  catalogPageForDay,
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

  it("maps density days to the direct chronological page", () => {
    const days = [
      { day: "2026-08-01", count: 10 },
      { day: "2026-08-02", count: 20 },
      { day: "2026-08-03", count: 30 },
    ];
    expect(catalogPageForDay("2026-08-02", days, "release_asc")).toBe(1);
    expect(catalogPageForDay("2026-08-02", days, "release_desc")).toBe(2);
    expect(catalogPageForDay("2026-08-02", days, "title_asc")).toBeNull();
    expect(catalogPageForDay("2026-08-02", days, "favorites")).toBeNull();
  });
});
