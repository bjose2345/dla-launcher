import { describe, expect, it } from "vitest";

import { recommendationReasonLabels, visibleRecommendationLanes } from "./catalogRecommendations";
import type {
  CatalogRecommendationItem,
  CatalogRecommendationLane,
  CatalogRecommendationLaneKey,
  CatalogRecommendations,
  CatalogWork,
} from "./types";

describe("catalog recommendations presentation", () => {
  it("uses the stable local lane order and omits empty or unknown lanes", () => {
    const recommendations: CatalogRecommendations = {
      anchorWorkCode: "RJ100",
      lanes: [
        lane("similar", [item("RJ300")]),
        { key: "unknown" as CatalogRecommendationLaneKey, items: [item("RJ999")] },
        lane("same_circle", [item("RJ200")]),
      ],
    };

    expect(visibleRecommendationLanes(recommendations).map((value) => value.key)).toEqual([
      "same_circle",
      "similar",
    ]);
  });

  it("selects localized reason labels and removes duplicates", () => {
    const recommendation = item("RJ200");
    recommendation.reasons = [
      { kind: "shared_tag", key: "healing", label: "癒し", labelEnglish: "Healing" },
      { kind: "shared_category", key: "healing", label: "ヒーリング", labelEnglish: "healing" },
      { kind: "shared_language", key: "jpn", label: "日本語", labelEnglish: "" },
    ];

    expect(recommendationReasonLabels(recommendation, true)).toEqual(["Healing", "日本語"]);
    expect(recommendationReasonLabels(recommendation, false)).toEqual(["癒し", "ヒーリング", "日本語"]);
  });
});

function lane(key: CatalogRecommendationLaneKey, items: CatalogRecommendationItem[]): CatalogRecommendationLane {
  return { key, items };
}

function item(code: string): CatalogRecommendationItem {
  return { work: work(code), score: 1, reasons: [] };
}

function work(code: string): CatalogWork {
  return {
    code,
    sourceCode: "DL",
    title: code,
    titleEnglish: code,
    addedDate: "2026-01-01",
    releaseDate: "2026-01-01",
    updatedDate: "2026-01-01",
    ageRating: "r18",
    releaseType: "digital",
    mainImageUrls: [],
    thumbnailUrls: [],
    circles: [],
    categories: [],
    tags: [],
    synthetic: false,
  };
}
