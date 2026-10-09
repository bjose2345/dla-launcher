import { describe, expect, it } from "vitest";

import deDE from "../../public/locales/de-DE/common.json";
import enUS from "../../public/locales/en-US/common.json";
import esES from "../../public/locales/es-ES/common.json";
import frFR from "../../public/locales/fr-FR/common.json";
import itIT from "../../public/locales/it-IT/common.json";
import jaJP from "../../public/locales/ja-JP/common.json";
import ptPT from "../../public/locales/pt-PT/common.json";
import ruRU from "../../public/locales/ru-RU/common.json";
import {
  loadLocaleCatalog,
  localeCatalogLoaded,
  localeIds,
  translate,
  type LocaleId,
  type MessageCatalog,
  type MessageKey,
} from "./catalogs";

const messageCatalogs: Record<LocaleId, MessageCatalog> = {
  "en-US": enUS,
  "ja-JP": jaJP,
  "es-ES": esES,
  "de-DE": deDE,
  "fr-FR": frFR,
  "it-IT": itIT,
  "pt-PT": ptPT,
  "ru-RU": ruRU,
};

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

  it("falls back to English until a catalog is loaded, then uses that catalog", async () => {
    expect(localeCatalogLoaded("en-US")).toBe(true);
    expect(localeCatalogLoaded("ja-JP")).toBe(false);
    expect(translate("ja-JP", "nav.catalog")).toBe(enUS["nav.catalog"]);

    const first = loadLocaleCatalog("ja-JP");
    expect(loadLocaleCatalog("ja-JP")).toBe(first);
    await first;

    expect(localeCatalogLoaded("ja-JP")).toBe(true);
    expect(translate("ja-JP", "nav.catalog")).toBe(jaJP["nav.catalog"]);
  });
});

function placeholders(message: string): string[] {
  return [...message.matchAll(placeholderPattern)]
    .map((match) => match[1] ?? "")
    .sort();
}
