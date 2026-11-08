export type PreferenceStorage = Pick<Storage, "getItem" | "setItem">;
export type BrowserStorage = PreferenceStorage & Pick<Storage, "removeItem">;

/** The window's local storage, or nothing when it is absent or blocked. */
export function browserStorage(): BrowserStorage | undefined {
  if (typeof window === "undefined") return undefined;
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

/** The text stored under `key`, or null when there is none or storage is blocked. */
export function readStoredText(key: string, storage: PreferenceStorage | undefined = browserStorage()): string | null {
  if (!storage) return null;
  try {
    return storage.getItem(key);
  } catch {
    return null;
  }
}

/** The JSON value stored under `key`, or undefined when there is none or it cannot be read or parsed. */
export function readStoredJson(key: string, storage: PreferenceStorage | undefined = browserStorage()): unknown {
  const text = readStoredText(key, storage);
  if (text === null) return undefined;
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return undefined;
  }
}

/** The JSON object stored under `key`; anything else, including nothing, reads as an empty object. */
export function readStoredRecord(key: string, storage: PreferenceStorage | undefined = browserStorage()): Record<string, unknown> {
  const value = readStoredJson(key, storage);
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

/** Stores text under `key`; when storage is full or blocked the value lasts for this session only. */
export function writeStoredText(key: string, value: string, storage: PreferenceStorage | undefined = browserStorage()): void {
  if (!storage) return;
  try {
    storage.setItem(key, value);
  } catch {
    return;
  }
}

export function writeStoredJson(key: string, value: unknown, storage: PreferenceStorage | undefined = browserStorage()): void {
  writeStoredText(key, JSON.stringify(value), storage);
}
