import {
  localeIds,
  translate,
  type LocaleId,
  type MessageKey,
} from "../i18n/catalogs";

export { localeIds, translate };
export type { LocaleId, MessageKey };

export const themeIds = [
  "midnight-rose",
  "arc-graphite",
  "paper-pastel",
  "lumen-accessible",
] as const;

export type ThemeId = (typeof themeIds)[number];

export interface ThemeDefinition {
  id: ThemeId;
  labelKey: MessageKey;
  colors: readonly [string, string, string];
}

export interface LocaleDefinition {
  id: LocaleId;
  label: string;
  shortLabel: string;
}

export const themes: readonly ThemeDefinition[] = [
  { id: "midnight-rose", labelKey: "theme.midnightRose", colors: ["#16161d", "#e75c75", "#e878b8"] },
  { id: "arc-graphite", labelKey: "theme.arcGraphite", colors: ["#1e1f22", "#3574f0", "#e88861"] },
  { id: "paper-pastel", labelKey: "theme.paperPastel", colors: ["#f9f4ec", "#c97b84", "#d9a86b"] },
  { id: "lumen-accessible", labelKey: "theme.lumenAccessible", colors: ["#fafafa", "#0072b2", "#f0e442"] },
];

export const locales: readonly LocaleDefinition[] = [
  { id: "en-US", label: "English", shortLabel: "EN" },
  { id: "ja-JP", label: "日本語", shortLabel: "JA" },
  { id: "es-ES", label: "Español", shortLabel: "ES" },
  { id: "de-DE", label: "Deutsch", shortLabel: "DE" },
  { id: "fr-FR", label: "Français", shortLabel: "FR" },
  { id: "it-IT", label: "Italiano", shortLabel: "IT" },
  { id: "pt-PT", label: "Português", shortLabel: "PT" },
  { id: "ru-RU", label: "Русский", shortLabel: "RU" },
];

export const defaultTheme: ThemeId = "midnight-rose";
export const defaultLocale: LocaleId = "en-US";

const STORAGE_KEY = "dla-launcher:presentation:v1";

export interface PresentationPreferences {
  theme: ThemeId;
  locale: LocaleId;
  showPlayTime: boolean;
  includeUnreviewed: boolean;
  sidebarCollapsed: boolean;
}

export const defaultPresentationPreferences: PresentationPreferences = {
  theme: defaultTheme,
  locale: defaultLocale,
  showPlayTime: true,
  includeUnreviewed: true,
  sidebarCollapsed: false,
};

export function readPresentationPreferences(): PresentationPreferences {
  if (typeof window === "undefined") return defaultPresentationPreferences;
  try {
    const stored = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "{}") as Record<string, unknown>;
    return {
      theme: isThemeId(stored.theme) ? stored.theme : defaultTheme,
      locale: isLocaleId(stored.locale) ? stored.locale : defaultLocale,
      showPlayTime: typeof stored.showPlayTime === "boolean"
        ? stored.showPlayTime
        : defaultPresentationPreferences.showPlayTime,
      includeUnreviewed: typeof stored.includeUnreviewed === "boolean"
        ? stored.includeUnreviewed
        : defaultPresentationPreferences.includeUnreviewed,
      sidebarCollapsed: typeof stored.sidebarCollapsed === "boolean"
        ? stored.sidebarCollapsed
        : defaultPresentationPreferences.sidebarCollapsed,
    };
  } catch {
    return defaultPresentationPreferences;
  }
}

export function writePresentationPreferences(preferences: PresentationPreferences): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences));
  } catch {
    return;
  }
}

export function applyPresentationPreferences(preferences: PresentationPreferences): void {
  document.documentElement.dataset.theme = preferences.theme;
  document.documentElement.lang = preferences.locale;
  document.documentElement.style.colorScheme = preferences.theme === "paper-pastel" || preferences.theme === "lumen-accessible"
    ? "light"
    : "dark";
}

export function initializePresentationPreferences(): PresentationPreferences {
  const preferences = readPresentationPreferences();
  if (typeof document !== "undefined") applyPresentationPreferences(preferences);
  return preferences;
}

function isThemeId(value: unknown): value is ThemeId {
  return typeof value === "string" && themeIds.some((theme) => theme === value);
}

function isLocaleId(value: unknown): value is LocaleId {
  return typeof value === "string" && localeIds.some((locale) => locale === value);
}
