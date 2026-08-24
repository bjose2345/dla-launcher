import { describe, expect, it } from "vitest";

import {
  defaultLocale,
  defaultPresentationPreferences,
  defaultTheme,
  readPresentationPreferences,
  translate,
} from "./preferences";

describe("presentation preferences", () => {
  it("uses stable defaults outside a browser", () => {
    expect(readPresentationPreferences()).toEqual({
      locale: defaultLocale,
      theme: defaultTheme,
      showPlayTime: true,
      includeUnreviewed: true,
      sidebarCollapsed: false,
    });
  });

  it("shows play time and unreviewed works until the user opts out", () => {
    expect(defaultPresentationPreferences.showPlayTime).toBe(true);
    expect(defaultPresentationPreferences.includeUnreviewed).toBe(true);
  });

  it("starts with the sidebar expanded", () => {
    expect(defaultPresentationPreferences.sidebarCollapsed).toBe(false);
  });

  it("translates launcher labels and interpolates values", () => {
    expect(translate("es-ES", "nav.settings")).toBe("Ajustes");
    expect(translate("ja-JP", "catalog.loaded", { loaded: 60, total: 1000 })).toBe("1000件中60件を読込済み");
  });

  it("uses the selected locale without an English fallback", () => {
    expect(translate("de-DE", "detail.gallery")).toBe("Galerie");
  });
});
