import type { CatalogWork } from "./types";
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
  let path = value.split(/[?#]/, 1)[0] ?? value;
  try {
    path = new URL(value).pathname;
  } catch {
    path = path.replaceAll("\\", "/");
  }
  const filename = (path.split("/").at(-1) ?? "").toLowerCase();
  const stem = filename.replace(/\.(?:avif|gif|jpe?g|png|webp)$/i, "");
  return stem ? `asset:${stem}` : `url:${value}`;
}
