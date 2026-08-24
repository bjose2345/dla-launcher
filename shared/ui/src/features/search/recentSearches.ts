export const RECENT_SEARCHES_STORAGE_KEY = "dla-launcher:recent-searches:v1";
export const RECENT_SEARCHES_LIMIT = 5;

interface SearchStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export function readRecentSearches(storage = browserStorage()): string[] {
  if (!storage) return [];
  try {
    const value = JSON.parse(storage.getItem(RECENT_SEARCHES_STORAGE_KEY) ?? "[]") as unknown;
    if (!Array.isArray(value)) return [];
    const recent: string[] = [];
    const seen = new Set<string>();
    for (const item of value) {
      if (typeof item !== "string") continue;
      const query = item.trim();
      const key = query.toLocaleLowerCase();
      if (!query || seen.has(key)) continue;
      seen.add(key);
      recent.push(query);
      if (recent.length === RECENT_SEARCHES_LIMIT) break;
    }
    return recent;
  } catch {
    return [];
  }
}

export function recordRecentSearch(query: string, storage = browserStorage()): string[] {
  const normalized = query.trim();
  if (!normalized || !storage) return readRecentSearches(storage);
  const key = normalized.toLocaleLowerCase();
  const next = [
    normalized,
    ...readRecentSearches(storage).filter((item) => item.toLocaleLowerCase() !== key),
  ].slice(0, RECENT_SEARCHES_LIMIT);
  storage.setItem(RECENT_SEARCHES_STORAGE_KEY, JSON.stringify(next));
  return next;
}

export function clearRecentSearches(storage = browserStorage()): void {
  storage?.removeItem(RECENT_SEARCHES_STORAGE_KEY);
}

function browserStorage(): SearchStorage | undefined {
  return typeof window === "undefined" ? undefined : window.localStorage;
}
