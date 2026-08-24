import { describe, expect, it } from "vitest";

import {
  catalogFacetCounts,
  catalogFacetState,
  cycleCatalogFacet,
  decodeCatalogFacetFilters,
  emptyCatalogFacetFilters,
  setCatalogFacetState,
} from "./catalogFilters";

describe("catalog facet filters", () => {
  it("cycles one value through include, exclude, and off", () => {
    const empty = emptyCatalogFacetFilters();
    const included = cycleCatalogFacet(empty, "genres", "睡眠");
    const excluded = cycleCatalogFacet(included, "genres", "睡眠");
    const cleared = cycleCatalogFacet(excluded, "genres", "睡眠");

    expect(catalogFacetState(included, "genres", "睡眠")).toBe("include");
    expect(catalogFacetState(excluded, "genres", "睡眠")).toBe("exclude");
    expect(catalogFacetState(cleared, "genres", "睡眠")).toBe("off");
  });

  it("keeps include and exclude mutually exclusive", () => {
    const included = setCatalogFacetState(emptyCatalogFacetFilters(), "ages", "r18", "include");
    const excluded = setCatalogFacetState(included, "ages", "r18", "exclude");

    expect(excluded.ages).toEqual({ include: [], exclude: ["r18"] });
    expect(catalogFacetCounts(excluded)).toEqual({ include: 0, exclude: 1 });
  });

  it("sanitizes persisted state without discarding valid groups", () => {
    const restored = decodeCatalogFacetFilters(JSON.stringify({
      ages: { include: [" R18 ", "r18", 7], exclude: ["r18", "r15"] },
      categories: { include: ["SOU"] },
      unknown: { include: ["ignored"] },
    }));

    expect(restored.ages).toEqual({ include: ["R18"], exclude: ["r15"] });
    expect(restored.categories.include).toEqual(["SOU"]);
    expect(restored.genres).toEqual({ include: [], exclude: [] });
  });

  it("falls back to empty filters for invalid storage", () => {
    expect(decodeCatalogFacetFilters("not json")).toEqual(emptyCatalogFacetFilters());
  });
});
