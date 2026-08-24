import type {
  CatalogBrowseRequest,
  CatalogFacetFilters,
  CatalogFilters,
  CatalogRouteState,
  CatalogSort,
  CatalogTimeline,
} from "./types";
import { emptyCatalogFacetFilters } from "./catalogFilters";

export const catalogPageSize = 60;
export const defaultCatalogFilters: CatalogFilters = {
  search: "",
  category: "",
  tag: "",
  sort: "release_desc",
};

export const defaultCatalogRouteState: CatalogRouteState = {
  ...defaultCatalogFilters,
  timeline: "added",
  month: "",
  page: 1,
};

const validCatalogSorts = new Set<CatalogSort>([
  "release_asc",
  "release_desc",
  "title_asc",
  "title_desc",
  "favorites",
]);

export function parseCatalogSearch(search: Record<string, unknown>): CatalogRouteState {
  return {
    search: stringValue(search.search),
    category: stringValue(search.category),
    tag: stringValue(search.tag),
    sort: isCatalogSort(search.sort) ? search.sort : defaultCatalogFilters.sort,
    timeline: isCatalogTimeline(search.timeline) ? search.timeline : "added",
    month: isCatalogMonth(search.month) ? search.month : "",
    page: positiveInteger(search.page),
  };
}

export function catalogRouteSearch(state: CatalogRouteState) {
  return state;
}

export function catalogRequest(
  filters: CatalogFilters,
  offset: number,
  timeline: CatalogTimeline = "added",
  facets: CatalogFacetFilters = emptyCatalogFacetFilters(),
  month = "",
  day = "",
  limit = catalogPageSize,
): CatalogBrowseRequest {
  return {
    search: filters.search,
    category: filters.category,
    tag: filters.tag,
    sort: filters.sort,
    facets,
    timeline,
    month,
    day,
    limit,
    offset,
  };
}

export function catalogContextRequest(
  filters: CatalogFilters,
  timeline: CatalogTimeline,
  facets: CatalogFacetFilters,
) {
  return {
    category: filters.category,
    tag: filters.tag,
    facets,
    timeline,
  };
}

export function nextCatalogOffset(page: {
  hasMore: boolean;
  offset: number;
  items: unknown[];
}): number | undefined {
  return page.hasMore ? page.offset + page.items.length : undefined;
}

export function isCatalogSort(value: unknown): value is CatalogSort {
  return typeof value === "string" && validCatalogSorts.has(value as CatalogSort);
}

export function isCatalogTimeline(value: unknown): value is CatalogTimeline {
  return value === "added" || value === "release" || value === "updated";
}

export function isCatalogMonth(value: unknown): value is string {
  if (typeof value !== "string" || !/^\d{4}-\d{2}$/.test(value)) return false;
  const year = Number(value.slice(0, 4));
  const month = Number(value.slice(5));
  return year >= 1 && year < 9999 && month >= 1 && month <= 12;
}

function positiveInteger(value: unknown): number {
  const parsed = typeof value === "number" ? value : Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : 1;
}

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}
