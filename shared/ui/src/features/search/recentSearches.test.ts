import { describe, expect, it } from "vitest";

import {
  RECENT_SEARCHES_STORAGE_KEY,
  clearRecentSearches,
  readRecentSearches,
  recordRecentSearch,
} from "./recentSearches";

class MemoryStorage {
  private readonly values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }
}

describe("recent catalog searches", () => {
  it("keeps five normalized unique searches in most-recent order", () => {
    const storage = new MemoryStorage();
    for (const query of ["one", "two", "three", "four", "five", "six", " TWO "]) {
      recordRecentSearch(query, storage);
    }

    expect(readRecentSearches(storage)).toEqual(["TWO", "six", "five", "four", "three"]);
  });

  it("ignores malformed storage and clears the record", () => {
    const storage = new MemoryStorage();
    storage.setItem(RECENT_SEARCHES_STORAGE_KEY, "not-json");
    expect(readRecentSearches(storage)).toEqual([]);

    recordRecentSearch("RJ01326398", storage);
    clearRecentSearches(storage);
    expect(readRecentSearches(storage)).toEqual([]);
  });
});
