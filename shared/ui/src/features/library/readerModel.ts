import { browserStorage, type PreferenceStorage } from "../../preferences/browserStorage";
import type { CatalogCategory } from "../catalog";

export type ReaderLayout = "single" | "double" | "strip";
export type ReaderDirection = "ltr" | "rtl";
export type ReaderFit = "height" | "width" | "original";
export type ReaderBackground = "black" | "gray" | "paper";

export interface ReaderPreferences {
  layout: ReaderLayout;
  direction: ReaderDirection;
  fit: ReaderFit;
  background: ReaderBackground;
  hideControls: boolean;
  zoom: number;
}

export const READER_LAYOUTS: readonly ReaderLayout[] = ["single", "double", "strip"];
const STORAGE_PREFIX = "dla-launcher:reader:v2";
const MIN_ZOOM = 0.5;
const MAX_ZOOM = 4;
const MANGA_CATEGORIES = new Set(["manga", "digital_comic"]);

/** Manga reads right to left; everything else left to right. */
export function defaultReaderPreferences(manga: boolean): ReaderPreferences {
  return {
    layout: "single",
    direction: manga ? "rtl" : "ltr",
    fit: "height",
    background: "black",
    hideControls: true,
    zoom: 1,
  };
}

export function isMangaWork(categories: readonly Pick<CatalogCategory, "code">[]): boolean {
  return categories.some((category) => MANGA_CATEGORIES.has(category.code));
}

/** The choices made for this work before, over the defaults for its kind. */
export function readReaderPreferences(
  installationId: string,
  defaults: ReaderPreferences,
  storage: PreferenceStorage | undefined = browserStorage(),
): ReaderPreferences {
  if (!storage) return defaults;
  try {
    const value = JSON.parse(storage.getItem(storageKey(installationId)) ?? "{}") as Record<string, unknown>;
    return {
      layout: oneOf(value.layout, READER_LAYOUTS, defaults.layout),
      direction: oneOf(value.direction, ["ltr", "rtl"] as const, defaults.direction),
      fit: oneOf(value.fit, ["height", "width", "original"] as const, defaults.fit),
      background: oneOf(value.background, ["black", "gray", "paper"] as const, defaults.background),
      hideControls: typeof value.hideControls === "boolean" ? value.hideControls : defaults.hideControls,
      zoom: typeof value.zoom === "number" ? clampReaderZoom(value.zoom) : defaults.zoom,
    };
  } catch {
    return defaults;
  }
}

export function writeReaderPreferences(
  installationId: string,
  preferences: ReaderPreferences,
  storage: PreferenceStorage | undefined = browserStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(storageKey(installationId), JSON.stringify(preferences));
  } catch {
    return;
  }
}

export function nextReaderLayout(layout: ReaderLayout): ReaderLayout {
  return READER_LAYOUTS[(READER_LAYOUTS.indexOf(layout) + 1) % READER_LAYOUTS.length]!;
}

export function clampReaderZoom(value: number): number {
  if (!Number.isFinite(value)) return 1;
  return Math.round(Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, value)) * 100) / 100;
}

export function isWidePage(width: number, height: number): boolean {
  return Number.isFinite(width) && Number.isFinite(height) && height > 0 && width / height >= 1;
}

/**
 * The pages shown together with `index` in a two-page layout: the cover and
 * wide pages stand alone, the rest pair up as printed (2–3, 4–5, …).
 */
export function spreadPages(index: number, count: number, wide: ReadonlySet<number>): number[] {
  if (index < 0 || index >= count) return [];
  if (index === 0 || wide.has(index)) return [index];
  const left = index % 2 === 1 ? index : index - 1;
  if (wide.has(left)) return left === index ? [left] : [index];
  const right = left + 1;
  if (right >= count || wide.has(right)) return [left];
  return [left, right];
}

/** The first page of the next or previous group of pages in this layout. */
export function stepPage(
  index: number,
  count: number,
  step: -1 | 1,
  layout: ReaderLayout,
  wide: ReadonlySet<number>,
): number {
  if (layout !== "double") return Math.max(0, Math.min(count - 1, index + step));
  const shown = spreadPages(index, count, wide);
  const anchor = step === 1 ? shown.at(-1) : shown[0];
  if (anchor === undefined) return index;
  const next = anchor + step;
  if (next < 0 || next >= count) return index;
  return spreadPages(next, count, wide)[0] ?? next;
}

/** Pages in the order they appear on screen, left to right. */
export function screenOrder(pages: number[], direction: ReaderDirection): number[] {
  return direction === "rtl" ? [...pages].reverse() : pages;
}

/** Which way the page moves for a key or a click on one side of the screen. */
export function horizontalStep(side: "left" | "right", direction: ReaderDirection): -1 | 1 {
  if (side === "left") return direction === "rtl" ? 1 : -1;
  return direction === "rtl" ? -1 : 1;
}

export interface PageBox {
  width: number;
  height: number;
}

// PDF sizes are in points; CSS pixels are 96 to the inch.
const POINTS_TO_PIXELS = 96 / 72;

/**
 * The size a page is shown at. `shown` pages share the width in a two-page
 * layout; "height" fits the whole page on screen, "width" fills the width,
 * "original" uses the page's own size. Zoom applies on top.
 */
export function pageBox(
  page: PageBox,
  stage: PageBox,
  fit: ReaderFit,
  shown: number,
  zoom: number,
  points = false,
): PageBox {
  const aspect = page.width > 0 && page.height > 0 ? page.width / page.height : 0.7;
  let width: number;
  if (fit === "original") {
    width = points ? page.width * POINTS_TO_PIXELS : page.width;
  } else if (fit === "width") {
    width = stage.width / Math.max(1, shown);
  } else {
    width = Math.min(stage.height * aspect, stage.width / Math.max(1, shown));
  }
  width = Math.max(1, width * zoom);
  return { width, height: width / aspect };
}

/** Page numbers around the current one for the thumbnail strip. */
export function nearbyPages(index: number, count: number, radius = 4): number[] {
  const first = Math.max(0, Math.min(index - radius, count - radius * 2 - 1));
  const last = Math.min(count - 1, first + radius * 2);
  return Array.from({ length: Math.max(0, last - first + 1) }, (_, offset) => first + offset);
}

function storageKey(installationId: string): string {
  return `${STORAGE_PREFIX}:${installationId}`;
}

function oneOf<T extends string>(value: unknown, allowed: readonly T[], fallback: T): T {
  return typeof value === "string" && (allowed as readonly string[]).includes(value) ? value as T : fallback;
}
