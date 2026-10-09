import { describe, expect, it } from "vitest";

import {
  catalogRequest,
  nextCatalogOffset,
  parseCatalogSearch,
} from "./query";

describe("shared catalog query state", () => {
  it("normalizes route state and advances binding pages", () => {
    const filters = parseCatalogSearch({
      search: "図書館",
      category: "SOU",
      tag: "睡眠",
      sort: "title_asc",
    });

    expect(catalogRequest(filters, 60)).toEqual({
      search: "図書館",
      category: "SOU",
      tag: "睡眠",
      sort: "title_asc",
      facets: {
        ages: { include: [], exclude: [] },
        languages: { include: [], exclude: [] },
        categories: { include: [], exclude: [] },
        genres: { include: [], exclude: [] },
        fileTypes: { include: [], exclude: [] },
        miscellanies: { include: [], exclude: [] },
        circles: { include: [], exclude: [] },
      },
      timeline: "added",
      month: "",
      day: "",
      limit: 60,
      offset: 60,
    });
    expect(nextCatalogOffset({ hasMore: true, offset: 60, items: [1, 2] })).toBe(62);
    expect(nextCatalogOffset({ hasMore: false, offset: 60, items: [1, 2] })).toBeUndefined();
  });

  it("rejects unknown sort values", () => {
    expect(parseCatalogSearch({ sort: "random" }).sort).toBe("release_desc");
    expect(parseCatalogSearch({ sort: "favorites" }).sort).toBe("favorites");
  });

  it("validates month, timeline, and page route state", () => {
    expect(parseCatalogSearch({ month: "2026-08", timeline: "added", page: "7" })).toMatchObject({
      month: "2026-08",
      timeline: "added",
      page: 7,
    });
    expect(parseCatalogSearch({ month: "2026-19", timeline: "other", page: -2 })).toMatchObject({
      month: "",
      timeline: "activity",
      page: 1,
    });
    expect(parseCatalogSearch({ timeline: "activity" }).timeline).toBe("activity");
    expect(parseCatalogSearch({ month: "0000-01" }).month).toBe("");
    expect(parseCatalogSearch({ month: "9999-12" }).month).toBe("");
  });

  it("keeps a locked day only when it is a calendar date", () => {
    expect(parseCatalogSearch({ month: "2026-08", day: "2026-08-09" }).day).toBe("2026-08-09");
    expect(parseCatalogSearch({ day: "2026-08-32" }).day).toBe("");
    expect(parseCatalogSearch({ day: "2026-13-01" }).day).toBe("");
    expect(parseCatalogSearch({ day: "20260809" }).day).toBe("");
    expect(parseCatalogSearch({}).day).toBe("");
  });
});
