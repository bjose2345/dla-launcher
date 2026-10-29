import { browserStorage, type PreferenceStorage } from "../../preferences/browserStorage";
import type { PageBox } from "./readerModel";
import type { MediaSessionItem } from "./types";

export type ViewerFit = "screen" | "width" | "original";
export type ViewerBackground = "black" | "gray" | "ambient";
export type GallerySize = "small" | "medium" | "large";
/** Image sets open in the CG viewer unless the work was moved to the comic reader. */
export type ImageSetViewer = "viewer" | "reader";

export interface ImageViewerPreferences {
  fit: ViewerFit;
  background: ViewerBackground;
  /** Seconds each image stays during a slideshow. */
  interval: number;
  loop: boolean;
  hideControls: boolean;
  openIn: ImageSetViewer;
  gallerySize: GallerySize;
}

/** The images of one folder, by their place in the session. */
export interface ImageFolder {
  key: string;
  /** The folder below the one every image shares; empty for that top folder. */
  name: string;
  indexes: number[];
}

export const SLIDESHOW_INTERVALS = [3, 5, 10] as const;
export const FILMSTRIP_SIZE = 13;
const STORAGE_PREFIX = "dla-launcher:images:v1";
const MIN_ZOOM = 0.1;
const MAX_ZOOM = 8;

export function defaultImageViewerPreferences(): ImageViewerPreferences {
  return {
    fit: "screen",
    background: "black",
    interval: 5,
    loop: true,
    hideControls: true,
    openIn: "viewer",
    gallerySize: "medium",
  };
}

export function readImageViewerPreferences(
  installationId: string,
  storage: PreferenceStorage | undefined = browserStorage(),
): ImageViewerPreferences {
  const defaults = defaultImageViewerPreferences();
  if (!storage) return defaults;
  try {
    const value = JSON.parse(storage.getItem(storageKey(installationId)) ?? "{}") as Record<string, unknown>;
    return {
      fit: oneOf(value.fit, ["screen", "width", "original"] as const, defaults.fit),
      background: oneOf(value.background, ["black", "gray", "ambient"] as const, defaults.background),
      interval: oneOf(value.interval, SLIDESHOW_INTERVALS, defaults.interval),
      loop: typeof value.loop === "boolean" ? value.loop : defaults.loop,
      hideControls: typeof value.hideControls === "boolean" ? value.hideControls : defaults.hideControls,
      openIn: oneOf(value.openIn, ["viewer", "reader"] as const, defaults.openIn),
      gallerySize: oneOf(value.gallerySize, ["small", "medium", "large"] as const, defaults.gallerySize),
    };
  } catch {
    return defaults;
  }
}

export function writeImageViewerPreferences(
  installationId: string,
  preferences: ImageViewerPreferences,
  storage: PreferenceStorage | undefined = browserStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(storageKey(installationId), JSON.stringify(preferences));
  } catch {
    return;
  }
}

/** Folders in the order their first image comes, named below the folder they all share. */
export function imageFolders(items: readonly Pick<MediaSessionItem, "relativePath">[]): ImageFolder[] {
  const parents = items.map((item) => parentFolder(item.relativePath));
  const shared = sharedFolder(parents);
  const folders = new Map<string, ImageFolder>();
  parents.forEach((parent, index) => {
    let folder = folders.get(parent);
    if (!folder) {
      folder = { key: parent, name: parent.slice(shared.length).replace(/^\//, ""), indexes: [] };
      folders.set(parent, folder);
    }
    folder.indexes.push(index);
  });
  return [...folders.values()];
}

/** The images shown in turn: the whole set, or one folder of it. */
export function viewerSequence(folders: readonly ImageFolder[], count: number, scope: string | null): number[] {
  const folder = scope === null ? undefined : folders.find((candidate) => candidate.key === scope);
  return folder ? folder.indexes : Array.from({ length: count }, (_, index) => index);
}

/** The places in the sequence the filmstrip shows, kept full near both ends. */
export function filmstripWindow(at: number, length: number, size: number = FILMSTRIP_SIZE): number[] {
  const first = Math.min(Math.max(0, at - Math.floor(size / 2)), Math.max(0, length - size));
  return Array.from({ length: Math.min(size, length - first) }, (_, offset) => first + offset);
}

/** How much the image is scaled to fit; a zoom is a scale of its own pixels. */
export function fitScale(natural: PageBox, stage: PageBox, fit: ViewerFit): number {
  if (natural.width <= 0 || natural.height <= 0 || stage.width <= 0 || stage.height <= 0) return 1;
  if (fit === "original") return 1;
  if (fit === "width") return stage.width / natural.width;
  return Math.min(stage.width / natural.width, stage.height / natural.height);
}

/** The image's box on screen; until its size is known it takes a wide frame. */
export function imageBox(natural: PageBox | undefined, stage: PageBox, fit: ViewerFit, zoom: number | null): PageBox {
  if (!natural || natural.width <= 0 || natural.height <= 0) {
    const width = Math.min(stage.width, (stage.height * 16) / 9);
    return { width, height: (width * 9) / 16 };
  }
  const scale = zoom ?? fitScale(natural, stage, fit);
  return { width: natural.width * scale, height: natural.height * scale };
}

/** A part of an image, as fractions of its width and height. */
export interface ImageRegion {
  left: number;
  top: number;
  width: number;
  height: number;
}

interface Offset {
  left: number;
  top: number;
}

const MINIMAP = { width: 168, height: 126 };
const COMPACT_MINIMAP = { width: 112, height: 88 };

/** The panner's size: the whole image inside a small frame. */
export function minimapSize(natural: PageBox, compact: boolean): PageBox {
  const limit = compact ? COMPACT_MINIMAP : MINIMAP;
  if (natural.width <= 0 || natural.height <= 0) return limit;
  const scale = Math.min(limit.width / natural.width, limit.height / natural.height);
  return { width: natural.width * scale, height: natural.height * scale };
}

/** The part of a zoomed image the stage shows, from how far the stage is scrolled. */
export function visibleRegion(scroll: Offset, viewport: PageBox, image: Offset & PageBox): ImageRegion {
  if (image.width <= 0 || image.height <= 0) return { left: 0, top: 0, width: 1, height: 1 };
  const left = clampUnit((scroll.left - image.left) / image.width);
  const top = clampUnit((scroll.top - image.top) / image.height);
  const right = clampUnit((scroll.left + viewport.width - image.left) / image.width);
  const bottom = clampUnit((scroll.top + viewport.height - image.top) / image.height);
  return { left, top, width: right - left, height: bottom - top };
}

/** The scroll that centres the stage on a point of the image, given as fractions. */
export function scrollToPoint(point: { x: number; y: number }, viewport: PageBox, image: Offset & PageBox): Offset {
  return {
    left: image.left + clampUnit(point.x) * image.width - viewport.width / 2,
    top: image.top + clampUnit(point.y) * image.height - viewport.height / 2,
  };
}

export function clampViewerZoom(value: number): number {
  if (!Number.isFinite(value)) return 1;
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, value));
}

export function formatFileSize(bytes: number, locale: string): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toLocaleString(locale, { maximumFractionDigits: unit < 2 ? 0 : 1 })} ${units[unit]}`;
}

function clampUnit(value: number): number {
  return Math.min(1, Math.max(0, value));
}

function parentFolder(path: string): string {
  const slash = path.lastIndexOf("/");
  return slash < 0 ? "" : path.slice(0, slash);
}

function sharedFolder(folders: readonly string[]): string {
  if (!folders.length) return "";
  let shared = folders[0]!.split("/");
  for (const folder of folders.slice(1)) {
    const parts = folder.split("/");
    let same = 0;
    while (same < shared.length && same < parts.length && shared[same] === parts[same]) same += 1;
    shared = shared.slice(0, same);
  }
  return shared.join("/");
}

function oneOf<T extends string | number>(value: unknown, options: readonly T[], fallback: T): T {
  return options.includes(value as T) ? (value as T) : fallback;
}

function storageKey(installationId: string): string {
  return `${STORAGE_PREFIX}:${installationId}`;
}
