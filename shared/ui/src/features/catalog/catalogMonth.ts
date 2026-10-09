import type { CatalogDayBucket, CatalogMonthBucket } from "./types";

export interface CatalogMonthNavigation {
  previous: string | null;
  next: string | null;
}

export function reconcileCatalogMonth(
  current: string,
  months: CatalogMonthBucket[],
  defaultMonth: string,
  preferLatest = false,
): string {
  if (months.length === 0) return current || defaultMonth;
  if (preferLatest) return defaultMonth || months.at(-1)?.month || current;
  if (!current) return months.at(-1)?.month || defaultMonth;
  if (months.some((bucket) => bucket.month === current)) return current;
  const earlier = [...months].reverse().find((bucket) => bucket.month < current);
  if (earlier) return earlier.month;
  return months.find((bucket) => bucket.month > current)?.month
    ?? defaultMonth
    ?? months.at(-1)?.month
    ?? current;
}

export function catalogMonthNavigation(
  current: string,
  months: CatalogMonthBucket[],
): CatalogMonthNavigation {
  const index = months.findIndex((bucket) => bucket.month === current);
  if (index < 0) return { previous: null, next: null };
  return {
    previous: months[index - 1]?.month ?? null,
    next: months[index + 1]?.month ?? null,
  };
}

export interface CatalogRailDay {
  /** YYYY-MM-DD. */
  day: string;
  /** Day of the month, 1 to 31. */
  date: number;
  /** Works that day on the current timeline and filters; null draws a ghost. */
  count: number | null;
  /** How busy the day is next to the month's busiest day, 0 to 1. */
  fill: number;
}

/** The month's days as the day rail shows them; `buckets` lists every calendar day. */
export function catalogRailDays(buckets: CatalogDayBucket[]): CatalogRailDay[] {
  const maximum = Math.max(1, ...buckets.map((bucket) => bucket.count));
  return buckets.map(({ day, count }) => ({
    day,
    date: Number(day.slice(8, 10)),
    count: count > 0 ? count : null,
    fill: count / maximum,
  }));
}

export function catalogDayLabel(day: string, locale: string): string {
  const [year, month, date] = day.split("-").map(Number);
  if (!year || !month || !date) return day;
  return new Date(Date.UTC(year, month - 1, date)).toLocaleDateString(locale, {
    month: "long",
    day: "numeric",
    timeZone: "UTC",
  });
}

export function catalogMonthLabel(month: string, locale: string): string {
  const parsed = parseCatalogMonth(month);
  if (!parsed) return month;
  return new Date(Date.UTC(parsed.year, parsed.month - 1, 1)).toLocaleDateString(locale, {
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });
}

export function parseCatalogMonth(month: string): { year: number; month: number } | null {
  const match = /^(\d{4})-(\d{2})$/.exec(month);
  if (!match) return null;
  const year = Number(match[1]);
  const value = Number(match[2]);
  return year > 0 && value >= 1 && value <= 12 ? { year, month: value } : null;
}

export function currentCatalogMonth(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`;
}

/** One month cell of the works page's month rail. */
export interface CatalogRailMonth {
  month: string;
  /** Works in the month on the current timeline and filters; null outside
   *  the timeline's range. */
  count: number | null;
  /** Pickable: inside the range and not empty, like the arrows. */
  enabled: boolean;
}

/** The 12 months of `year` as the rail shows them. `minMonth` and `maxMonth`
 *  bound the timeline (inclusive YYYY-MM). */
export function catalogRailMonths(
  year: number,
  minMonth: string,
  maxMonth: string,
  months: CatalogMonthBucket[],
): CatalogRailMonth[] {
  const counts = new Map(months.map((bucket) => [bucket.month, bucket.count]));
  return Array.from({ length: 12 }, (_, index) => {
    const month = `${year}-${String(index + 1).padStart(2, "0")}`;
    const inRange = month >= minMonth && month <= maxMonth;
    const count = inRange ? counts.get(month) ?? 0 : null;
    return { month, count, enabled: count !== null && count > 0 };
  });
}

/** Whether the year picker offers `year`: any of its months is pickable. */
export function catalogYearHasMonths(
  year: number,
  minMonth: string,
  maxMonth: string,
  months: CatalogMonthBucket[],
): boolean {
  return catalogRailMonths(year, minMonth, maxMonth, months).some((month) => month.enabled);
}
