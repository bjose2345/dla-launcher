import type { CatalogFacetFilters, CatalogFilters, CatalogTimeline } from "./types";

// Every query over catalog data starts with `catalogKeys.all`, so a catalog
// import or search rebuild refreshes all of them with one prefix invalidation.
export const catalogKeys = {
  all: ["catalog"] as const,
  browse: (filters: CatalogFilters, facetFilters: CatalogFacetFilters, timeline: CatalogTimeline) => (
    ["catalog", "browse", filters, facetFilters, timeline] as const
  ),
  context: (category: string, tag: string, facetFilters: CatalogFacetFilters, timeline: CatalogTimeline) => (
    ["catalog", "context", category, tag, facetFilters, timeline] as const
  ),
  monthDay: (
    filters: CatalogFilters,
    facetFilters: CatalogFacetFilters,
    timeline: CatalogTimeline,
    month: string,
    day: string,
  ) => ["catalog", "month-day", filters, facetFilters, timeline, month, day] as const,
  works: (codes: readonly string[]) => ["catalog", "works", codes] as const,
  work: (code: string | null) => ["catalog", "work", code] as const,
  recommendations: (code: string) => ["catalog", "recommendations", code] as const,
  romContents: (code: string, romPosition: number) => ["catalog", "rom-contents", code, romPosition] as const,
  workPicker: (query: string) => ["catalog", "work-picker", query] as const,
  search: (text: string, filterKey: string, page: number) => ["catalog", "search", text, filterKey, page] as const,
  searchShortcuts: (text: string) => ["catalog", "search-shortcuts", text] as const,
  searchIndex: ["catalog", "search-index"] as const,
  searchRebuild: ["catalog", "search-rebuild"] as const,
};
