import { describe, expect, it } from "vitest";

import { readStoredJson, readStoredRecord, readStoredText, writeStoredJson, writeStoredText } from "./browserStorage";

function memoryStorage(initial: Record<string, string> = {}, { full = false } = {}) {
  const items = new Map(Object.entries(initial));
  return {
    items,
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => {
      if (full) throw new Error("QuotaExceededError");
      items.set(key, value);
    },
  };
}

describe("stored values", () => {
  it("reads text, JSON and objects, treating what is missing or broken as absent", () => {
    const storage = memoryStorage({ text: "line", json: '{"a":1}', list: "[1]", broken: "{", number: "3" });
    expect(readStoredText("text", storage)).toBe("line");
    expect(readStoredText("missing", storage)).toBeNull();
    expect(readStoredJson("json", storage)).toEqual({ a: 1 });
    expect(readStoredJson("broken", storage)).toBeUndefined();
    expect(readStoredJson("missing", storage)).toBeUndefined();
    expect(readStoredRecord("json", storage)).toEqual({ a: 1 });
    expect(readStoredRecord("list", storage)).toEqual({});
    expect(readStoredRecord("number", storage)).toEqual({});
    expect(readStoredRecord("missing", storage)).toEqual({});
  });

  it("writes text and JSON and ignores a full or absent storage", () => {
    const storage = memoryStorage();
    writeStoredText("text", "line", storage);
    writeStoredJson("json", { a: [1, 2] }, storage);
    expect(storage.items.get("text")).toBe("line");
    expect(storage.items.get("json")).toBe('{"a":[1,2]}');

    const full = memoryStorage({}, { full: true });
    expect(() => writeStoredJson("json", 1, full)).not.toThrow();
    expect(() => writeStoredText("text", "x", undefined)).not.toThrow();
    expect(readStoredText("text", undefined)).toBeNull();
  });
});
