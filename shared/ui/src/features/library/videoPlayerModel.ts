import { browserStorage, readStoredJson, readStoredRecord, writeStoredJson, type PreferenceStorage } from "../../preferences/browserStorage";
import type {
  MediaRepeatMode,
  MediaSessionItem,
  NativeVideoSubtitleColor,
  NativeVideoSubtitleStyle,
} from "./types";

export type VideoFit = "contain" | "cover" | "original";
export type SubtitleStyle = NativeVideoSubtitleStyle;

export const SUBTITLE_SIZES: readonly number[] = [0.5, 0.75, 1, 1.25, 1.5, 2];
export const SUBTITLE_COLORS: readonly NativeVideoSubtitleColor[] = ["white", "yellow", "cyan"];
export const SUBTITLE_OPACITIES: readonly number[] = [1, 0.75, 0.5];
export const SUBTITLE_BACKGROUNDS: readonly number[] = [0, 0.25, 0.5, 0.75, 1];

export const defaultSubtitleStyle: SubtitleStyle = {
  size: 1,
  color: "white",
  opacity: 1,
  background: 0.75,
};

const SUBTITLE_STYLE_KEY = "dla-launcher:subtitle-style:v1";

/** The subtitle style, shared by every video. */
export function readSubtitleStyle(
  storage: PreferenceStorage | undefined = browserStorage(),
): SubtitleStyle {
  return normalizeSubtitleStyle(readStoredJson(SUBTITLE_STYLE_KEY, storage) ?? {});
}

export function writeSubtitleStyle(
  style: SubtitleStyle,
  storage: PreferenceStorage | undefined = browserStorage(),
): void {
  writeStoredJson(SUBTITLE_STYLE_KEY, style, storage);
}

export function normalizeSubtitleStyle(value: unknown): SubtitleStyle {
  const style = typeof value === "object" && value !== null ? value as Record<string, unknown> : {};
  return {
    size: SUBTITLE_SIZES.find((size) => size === style.size) ?? defaultSubtitleStyle.size,
    color: SUBTITLE_COLORS.find((color) => color === style.color) ?? defaultSubtitleStyle.color,
    opacity: SUBTITLE_OPACITIES.find((opacity) => opacity === style.opacity) ?? defaultSubtitleStyle.opacity,
    background: SUBTITLE_BACKGROUNDS.find((background) => background === style.background)
      ?? defaultSubtitleStyle.background,
  };
}
export type VideoPlaybackFailure = "network" | "decode" | "unsupported";

export interface VideoPlayerPreferences {
  fit: VideoFit;
  volume: number;
  muted: boolean;
  subtitleLabel: string | null;
  /** Whether the letterbox bands carry a blurred copy of the picture, as YouTube's ambient mode. */
  ambient: boolean;
}

const STORAGE_PREFIX = "dla-launcher:video-player:v2";
const LEGACY_STORAGE_PREFIX = "dla-launcher:video-player:v1";

export const defaultVideoPlayerPreferences: VideoPlayerPreferences = {
  fit: "contain",
  volume: 1,
  muted: false,
  subtitleLabel: null,
  ambient: true,
};

export function readVideoPlayerPreferences(
  installationId: string,
  storage: PreferenceStorage | undefined = browserStorage(),
): VideoPlayerPreferences {
  const current = readStoredJson(storageKey(STORAGE_PREFIX, installationId), storage);
  const value = current === undefined
    ? readStoredRecord(storageKey(LEGACY_STORAGE_PREFIX, installationId), storage)
    : readStoredRecord(storageKey(STORAGE_PREFIX, installationId), storage);
  const preferences: VideoPlayerPreferences = {
    // The original-size fit is no longer offered; it reads as the default.
    fit: value.fit === "original"
      ? defaultVideoPlayerPreferences.fit
      : isVideoFit(value.fit) ? value.fit : defaultVideoPlayerPreferences.fit,
    volume: typeof value.volume === "number"
      ? clampVideoVolume(value.volume)
      : defaultVideoPlayerPreferences.volume,
    muted: typeof value.muted === "boolean" ? value.muted : defaultVideoPlayerPreferences.muted,
    subtitleLabel: typeof value.subtitleLabel === "string" && value.subtitleLabel.trim()
      ? value.subtitleLabel
      : null,
    ambient: typeof value.ambient === "boolean" ? value.ambient : defaultVideoPlayerPreferences.ambient,
  };
  if (current === undefined && Object.keys(value).length) {
    writeStoredJson(storageKey(STORAGE_PREFIX, installationId), preferences, storage);
  }
  return preferences;
}

export function writeVideoPlayerPreferences(
  installationId: string,
  preferences: VideoPlayerPreferences,
  storage: PreferenceStorage | undefined = browserStorage(),
): void {
  writeStoredJson(storageKey(STORAGE_PREFIX, installationId), preferences, storage);
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
