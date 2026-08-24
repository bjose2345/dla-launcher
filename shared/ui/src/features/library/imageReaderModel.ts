import type { MediaSessionItem } from "./types";

export type ImageReaderProfile = "gallery" | "manga";
export type ImageReaderLayout = "page" | "spread" | "continuous";
export type ImageReaderDirection = "ltr" | "rtl";
export type ImageReaderFit = "width" | "height" | "original";

export interface ImageReaderPreferences {
  profile: ImageReaderProfile;
  layout: ImageReaderLayout;
  direction: ImageReaderDirection;
  fit: ImageReaderFit;
  zoom: number;
}

export interface ImageReaderChapter {
  path: string;
  items: MediaSessionItem[];
}

type PreferenceStorage = Pick<Storage, "getItem" | "setItem">;

const STORAGE_PREFIX = "dla-launcher:image-reader:v1";
const MIN_ZOOM = 0.5;
const MAX_ZOOM = 4;

export const defaultImageReaderPreferences: ImageReaderPreferences = {
  profile: "gallery",
  layout: "page",
  direction: "ltr",
  fit: "height",
  zoom: 1,
};

export function imageReaderProfilePreferences(
  profile: ImageReaderProfile,
): ImageReaderPreferences {
  return profile === "manga"
    ? { profile, layout: "spread", direction: "rtl", fit: "height", zoom: 1 }
    : { ...defaultImageReaderPreferences };
}

export function readImageReaderPreferences(
  installationId: string,
  storage = browserStorage(),
): ImageReaderPreferences {
  if (!storage) return defaultImageReaderPreferences;
  try {
    const value = JSON.parse(storage.getItem(storageKey(installationId)) ?? "{}") as Record<string, unknown>;
    return {
      profile: isProfile(value.profile) ? value.profile : defaultImageReaderPreferences.profile,
      layout: isLayout(value.layout) ? value.layout : defaultImageReaderPreferences.layout,
      direction: isDirection(value.direction) ? value.direction : defaultImageReaderPreferences.direction,
      fit: isFit(value.fit) ? value.fit : defaultImageReaderPreferences.fit,
      zoom: typeof value.zoom === "number"
        ? clampImageReaderZoom(value.zoom)
        : defaultImageReaderPreferences.zoom,
    };
  } catch {
    return defaultImageReaderPreferences;
  }
}

export function writeImageReaderPreferences(
  installationId: string,
  preferences: ImageReaderPreferences,
  storage = browserStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(storageKey(installationId), JSON.stringify(preferences));
  } catch {
    return;
  }
}

export function clampImageReaderZoom(value: number): number {
  if (!Number.isFinite(value)) return 1;
  return Math.round(Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, value)) * 100) / 100;
}

export function imageReaderChapters(items: MediaSessionItem[]): ImageReaderChapter[] {
  const chapters: ImageReaderChapter[] = [];
  for (const item of items) {
    const path = readerParentPath(item.relativePath);
    const previous = chapters.at(-1);
    if (previous?.path === path) previous.items.push(item);
    else chapters.push({ path, items: [item] });
  }
  return chapters;
}

export function isWidePage(width: number, height: number): boolean {
  return Number.isFinite(width)
    && Number.isFinite(height)
    && height > 0
    && width / height >= 1;
}

export function spreadPageOrdinals(
  items: MediaSessionItem[],
  currentOrdinal: number,
  widePathKeys: ReadonlySet<string>,
): number[] {
  const index = items.findIndex((item) => item.ordinal === currentOrdinal);
  if (index < 0) return [];
  const current = items[index]!;
  if (index === 0 || widePathKeys.has(current.relativePath)) return [current.ordinal];

  const leftIndex = index % 2 === 1 ? index : index - 1;
  const left = items[leftIndex];
  const right = items[leftIndex + 1];
  if (!left) return [current.ordinal];
  if (widePathKeys.has(left.relativePath)) {
    return leftIndex === index ? [left.ordinal] : [current.ordinal];
  }
  if (!right || widePathKeys.has(right.relativePath)) return [left.ordinal];
  return [left.ordinal, right.ordinal];
}

export function spreadDisplayOrder(
  ordinals: number[],
  direction: ImageReaderDirection,
): number[] {
  return direction === "rtl" ? [...ordinals].reverse() : ordinals;
}

export function spreadStep(
  items: MediaSessionItem[],
  currentOrdinal: number,
  direction: -1 | 1,
  widePathKeys: ReadonlySet<string>,
): number {
  const shown = spreadPageOrdinals(items, currentOrdinal, widePathKeys);
  const anchor = direction === 1 ? shown.at(-1) : shown[0];
  const anchorIndex = items.findIndex((item) => item.ordinal === anchor);
  if (anchorIndex < 0) return currentOrdinal;
  const nextIndex = anchorIndex + direction;
  const next = items[nextIndex];
  if (!next) return currentOrdinal;
  return spreadPageOrdinals(items, next.ordinal, widePathKeys)[0] ?? next.ordinal;
}

export function readerParentPath(relativePath: string): string {
  const normalized = relativePath.replaceAll("\\", "/");
  return normalized.includes("/") ? normalized.slice(0, normalized.lastIndexOf("/")) : "";
}

export function adjacentReaderItems(
  items: MediaSessionItem[],
  currentOrdinal: number,
  radius = 2,
): MediaSessionItem[] {
  const index = items.findIndex((item) => item.ordinal === currentOrdinal);
  if (index < 0 || radius <= 0) return [];
  return items.filter((_, candidateIndex) => (
    candidateIndex !== index && Math.abs(candidateIndex - index) <= radius
  ));
}

export function readerHorizontalStep(
  key: "ArrowLeft" | "ArrowRight",
  direction: ImageReaderDirection,
): -1 | 1 {
  if (key === "ArrowLeft") return direction === "rtl" ? 1 : -1;
  return direction === "rtl" ? -1 : 1;
}

export function readerClickStep(
  side: "left" | "right",
  direction: ImageReaderDirection,
): -1 | 1 {
  return readerHorizontalStep(side === "left" ? "ArrowLeft" : "ArrowRight", direction);
}

function storageKey(installationId: string): string {
  return `${STORAGE_PREFIX}:${installationId}`;
}

function browserStorage(): PreferenceStorage | undefined {
  if (typeof window === "undefined") return undefined;
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

function isProfile(value: unknown): value is ImageReaderProfile {
  return value === "gallery" || value === "manga";
}

function isLayout(value: unknown): value is ImageReaderLayout {
  return value === "page" || value === "continuous";
}

function isDirection(value: unknown): value is ImageReaderDirection {
  return value === "ltr" || value === "rtl";
}

function isFit(value: unknown): value is ImageReaderFit {
  return value === "width" || value === "height" || value === "original";
}
