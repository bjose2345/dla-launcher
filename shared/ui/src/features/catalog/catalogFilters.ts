import {
  catalogFacetGroups,
  type CatalogFacetCatalog,
  type CatalogFacetFilters,
  type CatalogFacetGroup,
  type CatalogFacetSelection,
  type CatalogFacetState,
} from "./types";

export const CATALOG_FILTERS_STORAGE_KEY = "dla-launcher:catalog-filters:v1";

export function emptyCatalogFacetFilters(): CatalogFacetFilters {
  const empty = (): CatalogFacetSelection => ({ include: [], exclude: [] });
  return {
    ages: empty(),
    languages: empty(),
    categories: empty(),
    genres: empty(),
    fileTypes: empty(),
    miscellanies: empty(),
    circles: empty(),
  };
}

export function emptyCatalogFacetCatalog(): CatalogFacetCatalog {
  return {
    ages: [],
    languages: [],
    categories: [],
    genres: [],
    fileTypes: [],
    miscellanies: [],
    circles: [],
  };
}

export function catalogFacetState(
  filters: CatalogFacetFilters,
  group: CatalogFacetGroup,
  key: string,
): CatalogFacetState {
  if (filters[group].include.includes(key)) return "include";
  if (filters[group].exclude.includes(key)) return "exclude";
  return "off";
}

export function setCatalogFacetState(
  filters: CatalogFacetFilters,
  group: CatalogFacetGroup,
  key: string,
  state: CatalogFacetState,
): CatalogFacetFilters {
  const current = filters[group];
  const include = current.include.filter((value) => value !== key);
  const exclude = current.exclude.filter((value) => value !== key);
  if (state === "include") include.push(key);
  if (state === "exclude") exclude.push(key);
  return { ...filters, [group]: { include, exclude } };
}

export function cycleCatalogFacet(
  filters: CatalogFacetFilters,
  group: CatalogFacetGroup,
  key: string,
): CatalogFacetFilters {
  const current = catalogFacetState(filters, group, key);
  const next = current === "off" ? "include" : current === "include" ? "exclude" : "off";
  return setCatalogFacetState(filters, group, key, next);
}

export function catalogFacetCounts(filters: CatalogFacetFilters) {
  return catalogFacetGroups.reduce(
    (counts, group) => ({
      include: counts.include + filters[group].include.length,
      exclude: counts.exclude + filters[group].exclude.length,
    }),
    { include: 0, exclude: 0 },
  );
}

export function catalogFacetFilterKey(filters: CatalogFacetFilters): string {
  return JSON.stringify(catalogFacetGroups.map((group) => [
    group,
    filters[group].include,
    filters[group].exclude,
  ]));
}

export function readCatalogFacetFilters(): CatalogFacetFilters {
  if (typeof window === "undefined") return emptyCatalogFacetFilters();
  try {
    return decodeCatalogFacetFilters(window.localStorage.getItem(CATALOG_FILTERS_STORAGE_KEY));
  } catch {
    return emptyCatalogFacetFilters();
  }
}

export function writeCatalogFacetFilters(filters: CatalogFacetFilters): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(CATALOG_FILTERS_STORAGE_KEY, JSON.stringify(filters));
  } catch {
    return;
  }
}

export function decodeCatalogFacetFilters(value: string | null): CatalogFacetFilters {
  if (!value) return emptyCatalogFacetFilters();
  try {
    const input = JSON.parse(value) as Record<string, unknown>;
    const filters = emptyCatalogFacetFilters();
    for (const group of catalogFacetGroups) {
      const selection = isRecord(input[group]) ? input[group] : {};
      filters[group] = normalizeSelection(selection);
    }
    return filters;
  } catch {
    return emptyCatalogFacetFilters();
  }
}

function normalizeSelection(input: Record<string, unknown>): CatalogFacetSelection {
  const include = stringList(input.include);
  const included = new Set(include.map(comparisonKey));
  return {
    include,
    exclude: stringList(input.exclude).filter((value) => !included.has(comparisonKey(value))),
  };
}

function stringList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const result: string[] = [];
  const seen = new Set<string>();
  for (const item of value) {
    if (typeof item !== "string") continue;
    const normalized = item.trim();
    const comparable = comparisonKey(normalized);
    if (!normalized || seen.has(comparable)) continue;
    seen.add(comparable);
    result.push(normalized);
    if (result.length === 128) break;
  }
  return result;
}

function comparisonKey(value: string): string {
  return value.toLocaleLowerCase();
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
