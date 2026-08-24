import { describe, expect, it } from "vitest";

import {
  localeIds,
  messageCatalogs,
  translate,
  type MessageKey,
} from "./catalogs";

const placeholderPattern = /\{([^{}]+)\}/g;

describe("message catalogs", () => {
  const englishEntries = Object.entries(messageCatalogs["en-US"]);
  const englishKeys = englishEntries.map(([key]) => key).sort();

  it.each(localeIds)("keeps %s in parity with the source catalog", (locale) => {
    expect(Object.keys(messageCatalogs[locale]).sort()).toEqual(englishKeys);
  });

  it.each(localeIds)("does not contain blank messages in %s", (locale) => {
    const blankKeys = Object.entries(messageCatalogs[locale])
      .filter(([, message]) => message.trim().length === 0)
      .map(([key]) => key);

    expect(blankKeys).toEqual([]);
  });

  it.each(localeIds)("preserves interpolation placeholders in %s", (locale) => {
    const mismatches = englishEntries.flatMap(([key, englishMessage]) => {
      const localizedMessage = messageCatalogs[locale][key as MessageKey];
      const expected = placeholders(englishMessage);
      const actual = placeholders(localizedMessage);
      return expected.join("\u0000") === actual.join("\u0000")
        ? []
        : [{ key, expected, actual }];
    });

    expect(mismatches).toEqual([]);
  });

  it("interpolates every occurrence of a supplied value", () => {
    expect(translate("en-US", "catalog.loaded", { loaded: 12, total: 12 }))
      .toBe("12 of 12 loaded");
  });
});

function placeholders(message: string): string[] {
  return [...message.matchAll(placeholderPattern)]
    .map((match) => match[1] ?? "")
    .sort();
}
