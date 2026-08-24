import { catalogGridPageSize } from "./catalogPagination";
import type { CatalogDayBucket, CatalogMonthBucket, CatalogSort } from "./types";

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

export function catalogPageForDay(
  day: string,
  buckets: CatalogDayBucket[],
  sort: CatalogSort,
  pageSize = catalogGridPageSize,
): number | null {
  const index = buckets.findIndex((bucket) => bucket.day === day);
  if (index < 0 || buckets[index]?.count === 0) return null;
  if (sort !== "release_asc" && sort !== "release_desc") return null;
  const preceding = sort === "release_asc"
    ? buckets.slice(0, index)
    : buckets.slice(index + 1);
  const offset = preceding.reduce((total, bucket) => total + bucket.count, 0);
  return Math.floor(offset / pageSize) + 1;
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
