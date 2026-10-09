import type { CatalogRom, CatalogWork } from "./types";
import { visualImageUrls } from "./workImages";

export function ageRatingLabel(
  value: string,
  labels: { allAges: string; notCataloged: string },
): string {
  const normalized = value.trim().toLowerCase().replaceAll("-", "_");
  const values: Record<string, string> = {
    all_ages: labels.allAges,
    r15: "R15",
    r18: "R18",
  };
  return values[normalized] ?? (value.trim() || labels.notCataloged);
}

export function heroImageUrls(work: CatalogWork): string[] {
  return visualImageUrls(work);
}

export function sampleImageChains(urls: string[]): string[][] {
  const chains = new Map<string, string[]>();
  for (const value of urls) {
    const url = value.trim();
    if (!url) continue;
    const key = imageAssetKey(url);
    const chain = chains.get(key) ?? [];
    if (!chain.includes(url)) chain.push(url);
    chains.set(key, chain);
  }
  return [...chains.values()];
}

export function dlsiteWorkUrl(work: CatalogWork): string | null {
  if (work.synthetic || !["dl", "dlsite"].includes(work.sourceCode.toLowerCase())) return null;
  const code = work.code.toUpperCase();
  const site = code.startsWith("BJ") ? "books" : code.startsWith("VJ") ? "pro" : code.startsWith("RJ") ? "maniax" : null;
  return site ? `https://www.dlsite.com/${site}/work/=/product_id/${encodeURIComponent(work.code)}.html` : null;
}

export function displayName(name: string, nameEnglish: string, english: boolean): string {
  return english && nameEnglish ? nameEnglish : name;
}

function imageAssetKey(value: string): string {
  return imageAssetKeyFromValue(value, true);
}

function imageAssetKeyFromValue(value: string, unwrapSource: boolean): string {
  let path = value.split(/[?#]/, 1)[0] ?? value;
  try {
    path = new URL(value).pathname;
  } catch {
    path = path.replaceAll("\\", "/");
  }
  const filename = (path.split("/").at(-1) ?? "").toLowerCase();
  if (unwrapSource) {
    const source = decodeWrappedImageSource(filename);
    if (source) return imageAssetKeyFromValue(source, false);
  }
  const stem = filename.replace(/\.(?:avif|gif|jpe?g|png|webp)$/i, "");
  return stem ? `asset:${stem}` : `url:${value}`;
}

function decodeWrappedImageSource(value: string): string | null {
  try {
    const decoded = decodeURIComponent(value);
    return decoded !== value && /^https?:\/\//i.test(decoded) ? decoded : null;
  } catch {
    return null;
  }
}

/** All the work's archive bytes and how many releases (update date and
 *  version) they form. */
export function archiveTotals(roms: CatalogRom[]): { bytes: number; versions: number } {
  const releases = new Set(roms.map((rom) => `${rom.updateDate}\u0000${rom.version}`));
  const bytes = roms.reduce((total, rom) => total + (Number(rom.size) || 0), 0);
  return { bytes, versions: releases.size };
}

/** "418 MB": decimal units with one fraction digit at most. */
export function formatArchiveSize(bytes: number, locale: string): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return `${value.toLocaleString(locale, { maximumFractionDigits: unit === 0 ? 0 : 1 })} ${units[unit]}`;
}
