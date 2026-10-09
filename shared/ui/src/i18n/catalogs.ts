import enUS from "../../public/locales/en-US/common.json";

export const localeIds = [
  "en-US",
  "ja-JP",
  "es-ES",
  "de-DE",
  "fr-FR",
  "it-IT",
  "pt-PT",
  "ru-RU",
] as const;

export type LocaleId = (typeof localeIds)[number];
export type MessageKey = keyof typeof enUS;
export type MessageCatalog = Record<MessageKey, string>;

// Only English ships in the startup chunk. The other catalogs are separate
// chunks inside the application bundle, so loading one never needs the network.
const catalogModules: Record<Exclude<LocaleId, "en-US">, () => Promise<{ default: MessageCatalog }>> = {
  "ja-JP": () => import("../../public/locales/ja-JP/common.json"),
  "es-ES": () => import("../../public/locales/es-ES/common.json"),
  "de-DE": () => import("../../public/locales/de-DE/common.json"),
  "fr-FR": () => import("../../public/locales/fr-FR/common.json"),
  "it-IT": () => import("../../public/locales/it-IT/common.json"),
  "pt-PT": () => import("../../public/locales/pt-PT/common.json"),
  "ru-RU": () => import("../../public/locales/ru-RU/common.json"),
};

const loadedCatalogs = new Map<LocaleId, MessageCatalog>([["en-US", enUS]]);
const pendingCatalogs = new Map<LocaleId, Promise<MessageCatalog>>();

export function localeCatalogLoaded(locale: LocaleId): boolean {
  return loadedCatalogs.has(locale);
}

export function loadLocaleCatalog(locale: LocaleId): Promise<MessageCatalog> {
  const loaded = loadedCatalogs.get(locale);
  if (loaded) return Promise.resolve(loaded);
  let pending = pendingCatalogs.get(locale);
  if (!pending) {
    pending = catalogModules[locale as Exclude<LocaleId, "en-US">]().then(
      (module) => {
        loadedCatalogs.set(locale, module.default);
        pendingCatalogs.delete(locale);
        return module.default;
      },
      (error: unknown) => {
        pendingCatalogs.delete(locale);
        throw error;
      },
    );
    pendingCatalogs.set(locale, pending);
  }
  return pending;
}

/** English stands in for a catalog that has not been loaded yet. */
export function translate(
  locale: LocaleId,
  key: MessageKey,
  values: Record<string, string | number> = {},
): string {
  let message = (loadedCatalogs.get(locale) ?? enUS)[key];
  for (const [name, value] of Object.entries(values)) {
    message = message.replaceAll(`{${name}}`, String(value));
  }
  return message;
}
