import type { CatalogWork } from "./types";

export function visualImageUrls(
  work: Pick<CatalogWork, "mainImageUrls" | "thumbnailUrls">,
): string[] {
  return [...new Set([...work.mainImageUrls, ...[...work.thumbnailUrls].reverse()])];
}
