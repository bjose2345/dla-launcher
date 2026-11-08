import type { MessageKey } from "./catalogs";

type Translate = (key: MessageKey, values?: Record<string, string | number>) => string;

/**
 * `one` for a count of exactly 1, else `many`; both get the count, written
 * for the locale, as `count` next to any other values.
 */
export function pluralText(
  t: Translate,
  locale: string,
  count: number,
  one: MessageKey,
  many: MessageKey,
  values: Record<string, string | number> = {},
): string {
  return t(count === 1 ? one : many, { ...values, count: count.toLocaleString(locale) });
}
