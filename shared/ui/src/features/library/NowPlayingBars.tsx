import { useRef, useState } from "react";

import { useResizeObserver } from "../../app/useResizeObserver";
import { usePresentation } from "../../preferences/PresentationProvider";
import { useMediaPlayback } from "./MediaPlaybackProvider";

const BAR_PITCH_PX = 7;
const MIN_BARS = 12;
const MAX_BARS = 128;

export function useIsPlaying(installationId: string): boolean {
  const playback = useMediaPlayback();
  return playback.playing
    && playback.session?.action === "play_audio"
    && playback.session.installationId === installationId;
}

export function spectrumBarCount(width: number): number {
  if (!Number.isFinite(width) || width <= 0) return MIN_BARS;
  return Math.max(MIN_BARS, Math.min(MAX_BARS, Math.round(width / BAR_PITCH_PX)));
}

export function spectrumBarHeights(frequencies: Uint8Array, barCount: number): number[] {
  if (barCount <= 0) return [];
  const usable = Math.max(1, Math.floor(frequencies.length * 0.72));
  const heights: number[] = [];
  for (let bar = 0; bar < barCount; bar += 1) {
    const start = Math.floor((bar / barCount) * usable);
    const end = Math.max(start + 1, Math.floor(((bar + 1) / barCount) * usable));
    let peak = 0;
    for (let index = start; index < end && index < frequencies.length; index += 1) {
      peak = Math.max(peak, frequencies[index] ?? 0);
    }
    heights.push(Math.min(1, (peak / 255) ** 0.75));
  }
  return heights;
}

export function envelopeLevel(previous: number, target: number, release: number): number {
  return target >= previous ? target : previous * release + target * (1 - release);
}

export function NowPlayingBars({
  installationId,
  alwaysVisible = false,
}: {
  installationId: string;
  alwaysVisible?: boolean;
}) {
  const { t } = usePresentation();
  const playback = useMediaPlayback();
  const containerRef = useRef<HTMLSpanElement>(null);
  const [barCount, setBarCount] = useState(MIN_BARS);
  const current = playback.session?.action === "play_audio"
    && playback.session.installationId === installationId;
  const playing = current && playback.playing;

  useResizeObserver(containerRef, (container, entry) => {
    setBarCount(spectrumBarCount(entry?.contentRect.width ?? container.clientWidth));
  }, [alwaysVisible, current]);

  if (!current && !alwaysVisible) return null;

  return (
    <span
      className={`now-playing-eq${playing ? " is-playing" : " is-paused"}`}
      ref={containerRef}
      role={playing ? "img" : undefined}
      aria-label={playing ? t("media.nowPlaying") : undefined}
      aria-hidden={playing ? undefined : true}
    >
      {Array.from({ length: barCount }, (_, index) => <i key={index} />)}
    </span>
  );
}
