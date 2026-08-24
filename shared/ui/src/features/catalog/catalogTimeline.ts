import type { CatalogSort, CatalogTimeline, CatalogWork } from "./types";

export type CatalogViewMode = "grid" | "line";

export interface CatalogDateGroup {
  day: string | null;
  key: string;
  syntheticOnly: boolean;
  works: CatalogWork[];
}

export interface CatalogDateMarker {
  day: string;
  month: string;
  weekday: string;
}

export interface CatalogDateMarkerLabels {
  undated: string;
  unknownWeekday: string;
}

const UNKNOWN_DATE_KEY = "unknown";

export function groupWorksByTimeline(
  works: CatalogWork[],
  timeline: CatalogTimeline,
  sort: CatalogSort,
): CatalogDateGroup[] {
  const grouped = new Map<string, CatalogWork[]>();
  for (const work of works) {
    const key = catalogDay(work[timelineField(timeline)]) ?? UNKNOWN_DATE_KEY;
    const group = grouped.get(key);
    if (group) group.push(work);
    else grouped.set(key, [work]);
  }

  const direction = sort === "release_asc" ? 1 : -1;
  return [...grouped.entries()]
    .sort(([left, leftWorks], [right, rightWorks]) => {
      const leftSynthetic = leftWorks.every((work) => work.synthetic);
      const rightSynthetic = rightWorks.every((work) => work.synthetic);
      if (leftSynthetic !== rightSynthetic) return leftSynthetic ? 1 : -1;
      if (left === UNKNOWN_DATE_KEY) return 1;
      if (right === UNKNOWN_DATE_KEY) return -1;
      return left.localeCompare(right) * direction;
    })
    .map(([key, groupWorks]) => ({
      day: key === UNKNOWN_DATE_KEY ? null : key,
      key,
      syntheticOnly: groupWorks.every((work) => work.synthetic),
      works: groupWorks,
    }));
}

export function dateMarker(
  day: string | null,
  locale: string,
  labels: CatalogDateMarkerLabels,
): CatalogDateMarker {
  if (!day) return { day: "—", month: labels.undated, weekday: labels.unknownWeekday };
  const year = Number(day.slice(0, 4));
  const month = Number(day.slice(5, 7));
  const date = Number(day.slice(8, 10));
  const value = new Date(Date.UTC(year, month - 1, date));
  return {
    day: String(date),
    month: value.toLocaleDateString(locale, { month: "short", timeZone: "UTC" }),
    weekday: value.toLocaleDateString(locale, { weekday: "short", timeZone: "UTC" }),
  };
}

function timelineField(timeline: CatalogTimeline): "addedDate" | "releaseDate" | "updatedDate" {
  if (timeline === "added") return "addedDate";
  if (timeline === "updated") return "updatedDate";
  return "releaseDate";
}

function catalogDay(value: string): string | null {
  const day = value.trim().slice(0, 10);
  return /^\d{4}-\d{2}-\d{2}$/.test(day) ? day : null;
}
