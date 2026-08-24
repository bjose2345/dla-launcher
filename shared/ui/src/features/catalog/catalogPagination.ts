export type CatalogPageLink = number | "…left" | "…right";

export const catalogGridPageSize = 24;
export const catalogLinePreviewSize = 12;

export function catalogPageLinks(
  current: number,
  total: number,
  edge = 1,
  around = 2,
): CatalogPageLink[] {
  if (total <= 1) return [1];

  const left = Math.max(1, current - around);
  const right = Math.min(total, current + around);
  const links: CatalogPageLink[] = [];

  for (let page = 1; page <= Math.min(edge, total); page += 1) links.push(page);
  if (left > edge + 1) links.push("…left");
  for (let page = left; page <= right; page += 1) {
    if (!links.includes(page)) links.push(page);
  }
  if (right < total - edge) links.push("…right");
  for (let page = Math.max(total - edge + 1, 1); page <= total; page += 1) {
    if (!links.includes(page)) links.push(page);
  }

  return links;
}

export function catalogPageSlice<T>(items: T[], page: number, pageSize = catalogGridPageSize): T[] {
  const start = (Math.max(1, page) - 1) * pageSize;
  return items.slice(start, start + pageSize);
}

export function catalogPageRange(
  page: number,
  visible: number,
  total: number,
  pageSize = catalogGridPageSize,
): { from: number; to: number } {
  if (visible === 0 || total === 0) return { from: 0, to: 0 };
  const from = (Math.max(1, page) - 1) * pageSize + 1;
  return { from, to: Math.min(from + visible - 1, total) };
}
