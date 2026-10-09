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
