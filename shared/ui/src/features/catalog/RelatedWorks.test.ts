import { describe, expect, it } from "vitest";

import type { CatalogRelatedWork } from "./types";
import { relatedWorkShelves } from "./RelatedWorks";

function related(
  code: string,
  relationTypeCode: string,
  relationTypeLabel: string,
  direction: CatalogRelatedWork["direction"],
): CatalogRelatedWork {
  return {
    code,
    title: code,
    titleEnglish: "",
    relationTypeCode,
    relationTypeLabel,
    direction,
    thumbnailUrls: [],
  };
}

describe("related work shelves", () => {
  it("places a base work first and groups the remaining family by relation type", () => {
    const shelves = relatedWorkShelves([
      related("RJ00000001", "bonus", "Store Bonus", "parent"),
      related("RJ00000002", "bonus", "Store Bonus", "sibling"),
      related("RJ00000003", "original", "Original Work", "child"),
      related("RJ00000004", "soundtrack", "Soundtrack", "parent"),
    ], "Base Work");

    expect(shelves.map((shelf) => shelf.label)).toEqual(["Base Work", "Store Bonus", "Soundtrack"]);
    expect(shelves[1]?.items.map((work) => work.code)).toEqual(["RJ00000001", "RJ00000002"]);
  });
});
