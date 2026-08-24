import deDE from "../../public/locales/de-DE/common.json";
import enUS from "../../public/locales/en-US/common.json";
import esES from "../../public/locales/es-ES/common.json";
import frFR from "../../public/locales/fr-FR/common.json";
import itIT from "../../public/locales/it-IT/common.json";
import jaJP from "../../public/locales/ja-JP/common.json";
import ptPT from "../../public/locales/pt-PT/common.json";
import ruRU from "../../public/locales/ru-RU/common.json";

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

type MessageCatalog = Record<MessageKey, string>;

export const messageCatalogs: Record<LocaleId, MessageCatalog> = {
  "en-US": enUS,
  "ja-JP": jaJP,
  "es-ES": esES,
  "de-DE": deDE,
  "fr-FR": frFR,
  "it-IT": itIT,
  "pt-PT": ptPT,
  "ru-RU": ruRU,
};

export function translate(
  locale: LocaleId,
  key: MessageKey,
  values: Record<string, string | number> = {},
): string {
  let message = messageCatalogs[locale][key];
  for (const [name, value] of Object.entries(values)) {
    message = message.replaceAll(`{${name}}`, String(value));
  }
  return message;
}
