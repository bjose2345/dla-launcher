import type {
  CatalogRecommendationItem,
  CatalogRecommendationLane,
  CatalogRecommendationLaneKey,
  CatalogRecommendations,
} from "./types";

const laneOrder: readonly CatalogRecommendationLaneKey[] = ["same_circle", "similar"];

export function visibleRecommendationLanes(
  recommendations: CatalogRecommendations,
): CatalogRecommendationLane[] {
  const lanes = new Map(recommendations.lanes.map((lane) => [lane.key, lane]));
  return laneOrder.flatMap((key) => {
    const lane = lanes.get(key);
    return lane && lane.items.length > 0 ? [lane] : [];
  });
}

export function recommendationReasonLabels(
  item: CatalogRecommendationItem,
  englishLabels: boolean,
): string[] {
  const seen = new Set<string>();
  return item.reasons.flatMap((reason) => {
    const label = englishLabels && reason.labelEnglish.trim()
      ? reason.labelEnglish.trim()
      : reason.label.trim();
    const key = label.toLocaleLowerCase();
    if (!label || seen.has(key)) return [];
    seen.add(key);
    return [label];
  });
}
