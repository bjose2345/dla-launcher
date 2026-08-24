import type { MediaRepeatMode, MediaSessionItem } from "./types";

export type VideoFit = "contain" | "cover" | "original";
export type VideoPlaybackFailure = "network" | "decode" | "unsupported";

export interface VideoPlayerPreferences {
  fit: VideoFit;
  volume: number;
  muted: boolean;
  subtitleLabel: string | null;
}

type PreferenceStorage = Pick<Storage, "getItem" | "setItem">;

const STORAGE_PREFIX = "dla-launcher:video-player:v2";
const LEGACY_STORAGE_PREFIX = "dla-launcher:video-player:v1";

export const defaultVideoPlayerPreferences: VideoPlayerPreferences = {
  fit: "contain",
  volume: 1,
  muted: false,
  subtitleLabel: null,
};

export function readVideoPlayerPreferences(
  installationId: string,
  storage = browserStorage(),
): VideoPlayerPreferences {
  if (!storage) return defaultVideoPlayerPreferences;
  try {
    const current = storage.getItem(storageKey(STORAGE_PREFIX, installationId));
    const legacy = current === null
      ? storage.getItem(storageKey(LEGACY_STORAGE_PREFIX, installationId))
      : null;
    const value = JSON.parse(current ?? legacy ?? "{}") as Record<string, unknown>;
    const preferences: VideoPlayerPreferences = {
      fit: legacy !== null && value.fit === "original"
        ? defaultVideoPlayerPreferences.fit
        : isVideoFit(value.fit) ? value.fit : defaultVideoPlayerPreferences.fit,
      volume: typeof value.volume === "number"
        ? clampVideoVolume(value.volume)
        : defaultVideoPlayerPreferences.volume,
      muted: typeof value.muted === "boolean" ? value.muted : defaultVideoPlayerPreferences.muted,
      subtitleLabel: typeof value.subtitleLabel === "string" && value.subtitleLabel.trim()
        ? value.subtitleLabel
        : null,
    };
    if (current === null && legacy !== null) {
      storage.setItem(storageKey(STORAGE_PREFIX, installationId), JSON.stringify(preferences));
    }
    return preferences;
  } catch {
    return defaultVideoPlayerPreferences;
  }
}

export function writeVideoPlayerPreferences(
  installationId: string,
  preferences: VideoPlayerPreferences,
  storage = browserStorage(),
): void {
  if (!storage) return;
  try {
    storage.setItem(storageKey(STORAGE_PREFIX, installationId), JSON.stringify(preferences));
  } catch {
    return;
  }
}

export function clampVideoVolume(value: number): number {
  if (!Number.isFinite(value)) return 1;
  return Math.round(Math.max(0, Math.min(1, value)) * 100) / 100;
}

export function videoStepTarget(
  items: MediaSessionItem[],
  currentOrdinal: number,
  direction: -1 | 1,
  repeatMode: MediaRepeatMode,
): MediaSessionItem | undefined {
  const index = items.findIndex((item) => item.ordinal === currentOrdinal);
  if (index < 0 || items.length <= 1) return undefined;
  const adjacent = items[index + direction];
  if (adjacent) return adjacent;
  if (repeatMode !== "all") return undefined;
  return direction === 1 ? items[0] : items.at(-1);
}

export function videoPlaybackFailure(code: number | null | undefined): VideoPlaybackFailure | null {
  if (code === 2) return "network";
  if (code === 3) return "decode";
  if (code === 4) return "unsupported";
  return null;
}

function storageKey(prefix: string, installationId: string): string {
  return `${prefix}:${installationId}`;
}

function browserStorage(): PreferenceStorage | undefined {
  if (typeof window === "undefined") return undefined;
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

function isVideoFit(value: unknown): value is VideoFit {
  return value === "contain" || value === "cover" || value === "original";
}

export function seekFractionForKey(key: string): number | null {
  if (key.length !== 1 || key < "0" || key > "9") return null;
  return Number(key) / 10;
}

export function bufferedAhead(
  ranges: Array<{ start: number; end: number }>,
  positionSeconds: number,
): number {
  for (const range of ranges) {
    if (positionSeconds >= range.start && positionSeconds <= range.end) return range.end;
  }
  return positionSeconds;
}
