import type { CatalogWork } from "./types";

/**
 * A catalog name in the active language: the English one when preferred and
 * the catalog has it, otherwise the original.
 */
export function localizedName(name: string, nameEnglish: string, preferEnglish: boolean): string {
  return preferEnglish && nameEnglish.trim() ? nameEnglish : name;
}

export function workTitle(work: CatalogWork, preferEnglish: boolean): string {
  return localizedName(work.title, work.titleEnglish, preferEnglish);
}
