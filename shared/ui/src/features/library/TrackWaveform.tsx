import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState, type PointerEvent } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { formatPlaybackTime } from "./audioPlayer";
import { useMediaPlayback } from "./MediaPlaybackProvider";
import type { LibraryGateway, MediaResume } from "./types";

const BAR_PITCH_PX = 7;
const MIN_BUCKETS = 32;
const MAX_BUCKETS = 256;
const SOURCE_BUCKETS = 256;

export function waveformBucketCount(width: number): number {
  if (!Number.isFinite(width) || width <= 0) return MIN_BUCKETS;
  return Math.max(MIN_BUCKETS, Math.min(MAX_BUCKETS, Math.round(width / BAR_PITCH_PX)));
}

export function seekPositionFromPointer(
  clientX: number,
  left: number,
  width: number,
  durationSeconds: number,
): number {
  if (!Number.isFinite(width) || width <= 0 || !Number.isFinite(durationSeconds)) return 0;
  const fraction = Math.max(0, Math.min(1, (clientX - left) / width));
  return fraction * Math.max(0, durationSeconds);
}

export function resampleWaveformPeaks(peaks: number[], targetCount: number): number[] {
  if (targetCount <= 0) return [];
  if (peaks.length === 0) return Array.from({ length: targetCount }, () => 0);
  return Array.from({ length: targetCount }, (_, bucket) => {
    const start = Math.floor((bucket / targetCount) * peaks.length);
    const end = Math.max(start + 1, Math.ceil(((bucket + 1) / targetCount) * peaks.length));
    return Math.max(...peaks.slice(start, end));
  });
}

export function TrackWaveform({
  gateway,
  installationId,
  ordinal,
  resume,
  onStart,
  onSeekPreview,
  onDuration,
}: {
  gateway: Pick<LibraryGateway, "readAudioWaveform">;
  installationId: string;
  ordinal: number;
  resume: MediaResume | null;
  onStart: (ordinal: number, positionSeconds: number) => Promise<void>;
  onSeekPreview?: (positionSeconds: number | null) => void;
  onDuration?: (durationSeconds: number | null) => void;
}) {
  const { t } = usePresentation();
  const playback = useMediaPlayback();
  const rootRef = useRef<HTMLDivElement>(null);
  const [bucketCount, setBucketCount] = useState(MIN_BUCKETS);
  const [dragPosition, setDragPosition] = useState<number | null>(null);
  const current = playback.session?.installationId === installationId
    && playback.item?.ordinal === ordinal;
  const waveform = useQuery({
    queryKey: ["library", "waveform", installationId, ordinal, SOURCE_BUCKETS],
    queryFn: () => gateway.readAudioWaveform(installationId, ordinal, SOURCE_BUCKETS),
    staleTime: Infinity,
    gcTime: 30 * 60_000,
  });
  const duration = current
    ? playback.durationSeconds ?? (waveform.data?.durationMs ?? 0) / 1_000
    : (waveform.data?.durationMs ?? resume?.durationMs ?? 0) / 1_000;
  const position = dragPosition ?? (current
    ? playback.positionSeconds
    : resume?.positionMs ? resume.positionMs / 1_000 : 0);
  const progress = duration > 0 ? Math.max(0, Math.min(1, position / duration)) : 0;
  const peaks = resampleWaveformPeaks(waveform.data?.peaks ?? [], bucketCount);

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const observer = new ResizeObserver(([entry]) => {
      setBucketCount(waveformBucketCount(entry?.contentRect.width ?? root.clientWidth));
    });
    observer.observe(root);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    onDuration?.(waveform.data ? waveform.data.durationMs / 1_000 : null);
  }, [onDuration, waveform.data]);

  const pointerPosition = (event: PointerEvent<HTMLDivElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    return seekPositionFromPointer(event.clientX, bounds.left, bounds.width, duration);
  };
  const preview = (next: number | null) => {
    setDragPosition(next);
    onSeekPreview?.(next);
  };
  const commit = (next: number) => {
    preview(null);
    if (current) playback.seek(next);
    else void onStart(ordinal, next);
  };

  return (
    <div
      className={`track-waveform${waveform.isPending ? " is-loading" : ""}`}
      ref={rootRef}
      role="slider"
      tabIndex={duration > 0 ? 0 : -1}
      aria-label={t("media.seek")}
      aria-valuemin={0}
      aria-valuemax={Math.round(duration)}
      aria-valuenow={Math.round(position)}
      aria-valuetext={formatPlaybackTime(position)}
      onPointerDown={(event) => {
        if (duration <= 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        const next = pointerPosition(event);
        preview(next);
      }}
      onPointerMove={(event) => {
        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
          const next = pointerPosition(event);
          preview(next);
        }
      }}
      onPointerUp={(event) => {
        if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
        event.currentTarget.releasePointerCapture(event.pointerId);
        commit(pointerPosition(event));
      }}
      onPointerCancel={() => preview(null)}
      onKeyDown={(event) => {
        if (duration <= 0) return;
        const step = event.shiftKey ? 30 : 5;
        if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
          event.preventDefault();
          commit(Math.max(0, position - step));
        } else if (event.key === "ArrowRight" || event.key === "ArrowUp") {
          event.preventDefault();
          commit(Math.min(duration, position + step));
        } else if (event.key === "Home") {
          event.preventDefault();
          commit(0);
        } else if (event.key === "End") {
          event.preventDefault();
          commit(duration);
        }
      }}
    >
      <span className="track-waveform-bars" aria-hidden="true">
        {(waveform.data ? peaks : Array.from({ length: bucketCount }, () => 0.04)).map((peak, index) => (
          <i
            key={index}
            className={(index + 1) / bucketCount <= progress ? "is-played" : undefined}
            style={{ height: `${Math.max(4, Math.min(100, peak * 100))}%` }}
          />
        ))}
      </span>
      <span className="track-waveform-times" aria-hidden="true">
        <span>{formatPlaybackTime(position)}</span>
        <span>{formatPlaybackTime(duration > 0 ? duration : null)}</span>
      </span>
    </div>
  );
}
