const dateTimeFormatters = new Map<string, Intl.DateTimeFormat>();
const numberFormatters = new Map<string, Intl.NumberFormat>();
const listFormatters = new Map<string, Intl.ListFormat>();

function cached<T>(cache: Map<string, T>, locale: string, create: () => T): T {
  let formatter = cache.get(locale);
  if (!formatter) {
    formatter = create();
    cache.set(locale, formatter);
  }
  return formatter;
}

export function dateTimeFormatter(locale: string): Intl.DateTimeFormat {
  return cached(dateTimeFormatters, locale, () => (
    new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" })
  ));
}

export function numberFormatter(locale: string): Intl.NumberFormat {
  return cached(numberFormatters, locale, () => new Intl.NumberFormat(locale));
}

export function listFormatter(locale: string): Intl.ListFormat {
  return cached(listFormatters, locale, () => (
    new Intl.ListFormat(locale, { style: "long", type: "conjunction" })
  ));
}

/** A timestamp as a medium date and short time; a value that is not a date is returned as is. */
export function formatDateTime(value: string, locale: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : dateTimeFormatter(locale).format(date);
}
