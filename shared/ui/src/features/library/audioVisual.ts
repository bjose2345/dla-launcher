import {
  browserStorage,
  readStoredText,
  writeStoredText,
  type PreferenceStorage,
} from "../../preferences/browserStorage";
import type { MessageKey } from "../../preferences/preferences";

/**
 * What the audio player draws over the artwork. The first three show the
 * whole track and are its seek bar; the others move with the sound.
 */
export const VISUAL_STYLES = [
  "bars", "mirror", "area", "spectrum", "mirrorSpectrum", "oscilloscope", "ring",
] as const;
export type VisualStyle = (typeof VISUAL_STYLES)[number];

export const VISUAL_COLORS = ["theme", "cover", "white", "ice", "rainbow"] as const;
export type VisualColor = (typeof VISUAL_COLORS)[number];

export const DEFAULT_VISUAL_STYLE: VisualStyle = "mirror";
export const DEFAULT_VISUAL_COLOR: VisualColor = "theme";

const STYLE_STORAGE_KEY = "dla.library.audio-visual-style";
const COLOR_STORAGE_KEY = "dla.library.audio-visual-color";

export const VISUAL_STYLE_LABELS: Record<VisualStyle, MessageKey> = {
  bars: "media.visual.style.bars",
  mirror: "media.visual.style.mirror",
  area: "media.visual.style.area",
  spectrum: "media.visual.style.spectrum",
  mirrorSpectrum: "media.visual.style.mirrorSpectrum",
  oscilloscope: "media.visual.style.oscilloscope",
  ring: "media.visual.style.ring",
};

export const VISUAL_COLOR_LABELS: Record<VisualColor, MessageKey> = {
  theme: "media.visual.color.theme",
  cover: "media.visual.color.cover",
  white: "media.visual.color.white",
  ice: "media.visual.color.ice",
  rainbow: "media.visual.color.rainbow",
};

export function showsWholeTrack(style: VisualStyle): boolean {
  return style === "bars" || style === "mirror" || style === "area";
}

export function readVisualStyle(storage: PreferenceStorage | undefined = browserStorage()): VisualStyle {
  const stored = readStoredText(STYLE_STORAGE_KEY, storage);
  return VISUAL_STYLES.find((style) => style === stored) ?? DEFAULT_VISUAL_STYLE;
}

export function writeVisualStyle(style: VisualStyle, storage: PreferenceStorage | undefined = browserStorage()): void {
  writeStoredText(STYLE_STORAGE_KEY, style, storage);
}

export function readVisualColor(storage: PreferenceStorage | undefined = browserStorage()): VisualColor {
  const stored = readStoredText(COLOR_STORAGE_KEY, storage);
  return VISUAL_COLORS.find((color) => color === stored) ?? DEFAULT_VISUAL_COLOR;
}

export function writeVisualColor(color: VisualColor, storage: PreferenceStorage | undefined = browserStorage()): void {
  writeStoredText(COLOR_STORAGE_KEY, color, storage);
}

/** A colour and how solid it is drawn, for one mark of `count`. */
export type Ink = (index: number, count: number) => { color: string; alpha: number };

export interface VisualPaint {
  /** The part of the track already heard, and everything a live style draws. */
  played: Ink;
  /** The part of the track still to come. */
  rest: Ink;
}

const WHITE = "#ffffff";
const ICE = "#6fe3ff";
const REST_ALPHA = 0.38;

/**
 * The colours of a colour set. "From the cover" falls back to the theme's
 * while the cover has given none: no cover, or one in greys.
 */
export function visualPaint(
  color: VisualColor,
  theme: { primary: string; content: string },
  cover: string | null,
): VisualPaint {
  const one = (played: string, rest: string, restAlpha: number): VisualPaint => ({
    played: () => ({ color: played, alpha: 1 }),
    rest: () => ({ color: rest, alpha: restAlpha }),
  });
  switch (color) {
    case "white":
      return one(WHITE, WHITE, REST_ALPHA);
    case "ice":
      return one(ICE, ICE, REST_ALPHA);
    case "rainbow": {
      const hue = (index: number, count: number) =>
        `hsl(${Math.round((300 * index) / Math.max(1, count))} 85% 64%)`;
      return {
        played: (index, count) => ({ color: hue(index, count), alpha: 1 }),
        rest: (index, count) => ({ color: hue(index, count), alpha: REST_ALPHA }),
      };
    }
    case "cover":
      if (cover) return one(cover, cover, REST_ALPHA);
      return one(theme.primary, theme.content, 0.42);
    case "theme":
      return one(theme.primary, theme.content, 0.42);
  }
}

const HUE_BINS = 12;

/**
 * The cover's own colour: the hue most of its vivid pixels share, at a
 * lightness that reads on the darkened artwork. Null for a cover in greys.
 * `pixels` are RGBA bytes.
 */
export function coverColor(pixels: Uint8ClampedArray | number[]): string | null {
  const weight = new Float64Array(HUE_BINS);
  const hueX = new Float64Array(HUE_BINS);
  const hueY = new Float64Array(HUE_BINS);
  const saturation = new Float64Array(HUE_BINS);
  for (let at = 0; at + 3 < pixels.length; at += 4) {
    if ((pixels[at + 3] ?? 0) < 128) continue;
    const red = (pixels[at] ?? 0) / 255;
    const green = (pixels[at + 1] ?? 0) / 255;
    const blue = (pixels[at + 2] ?? 0) / 255;
    const high = Math.max(red, green, blue);
    const low = Math.min(red, green, blue);
    const spread = high - low;
    const light = (high + low) / 2;
    if (spread < 0.12 || light < 0.12 || light > 0.92) continue;
    const vivid = spread / (1 - Math.abs(2 * light - 1));
    let hue = high === red
      ? ((green - blue) / spread) % 6
      : high === green ? (blue - red) / spread + 2 : (red - green) / spread + 4;
    hue = (hue * 60 + 360) % 360;
    const bin = Math.min(HUE_BINS - 1, Math.floor((hue / 360) * HUE_BINS));
    const share = vivid * (1 - Math.abs(light - 0.5));
    const angle = (hue * Math.PI) / 180;
    weight[bin] = (weight[bin] ?? 0) + share;
    hueX[bin] = (hueX[bin] ?? 0) + Math.cos(angle) * share;
    hueY[bin] = (hueY[bin] ?? 0) + Math.sin(angle) * share;
    saturation[bin] = (saturation[bin] ?? 0) + vivid * share;
  }
  let best = -1;
  for (let bin = 0; bin < HUE_BINS; bin += 1) {
    if ((weight[bin] ?? 0) > (best < 0 ? 0 : weight[best] ?? 0)) best = bin;
  }
  if (best < 0) return null;
  const hue = (Math.atan2(hueY[best] ?? 0, hueX[best] ?? 0) * 180) / Math.PI;
  const vivid = (saturation[best] ?? 0) / (weight[best] ?? 1);
  const percent = Math.round(Math.max(55, Math.min(90, vivid * 100)));
  return `hsl(${Math.round((hue + 360) % 360)} ${percent}% 68%)`;
}

/** The height of each placeholder mark while a track is still being read: a pulse that travels along it. */
export function readingLevel(index: number, seconds: number): number {
  return 0.06 + 0.2 * Math.max(0, Math.sin(seconds * 4 - index * 0.22)) ** 6;
}
