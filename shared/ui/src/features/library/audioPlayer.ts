export function clampPlaybackPosition(positionSeconds: number, durationSeconds: number | null): number {
  const boundedPosition = Number.isFinite(positionSeconds) ? Math.max(0, positionSeconds) : 0;
  if (durationSeconds === null || !Number.isFinite(durationSeconds) || durationSeconds <= 0) {
    return boundedPosition;
  }
  return Math.min(boundedPosition, durationSeconds);
}

export function restorePlaybackPosition(
  positionSeconds: number,
  durationSeconds: number | null,
  completed = false,
): number {
  if (completed) return 0;
  const bounded = clampPlaybackPosition(positionSeconds, durationSeconds);
  if (durationSeconds === null || !Number.isFinite(durationSeconds) || durationSeconds <= 0) {
    return bounded;
  }
  if (bounded >= durationSeconds - 0.25) return 0;
  return bounded;
}

export function resumePlaybackPosition(
  positionSeconds: number,
  durationSeconds: number | null,
  ended = false,
): number {
  const bounded = clampPlaybackPosition(positionSeconds, durationSeconds);
  if (ended) return 0;
  if (durationSeconds !== null && Number.isFinite(durationSeconds) && durationSeconds > 0) {
    if (bounded >= durationSeconds - 0.05) return 0;
    return Math.min(bounded + 0.001, Math.max(0, durationSeconds - 0.001));
  }
  return bounded + 0.001;
}

export function formatPlaybackTime(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds) || seconds < 0) return "--:--";
  const totalSeconds = Math.floor(seconds);
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const remainingSeconds = totalSeconds % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(remainingSeconds).padStart(2, "0")}`;
  }
  return `${minutes}:${String(remainingSeconds).padStart(2, "0")}`;
}

export function clampPlaybackRate(value: number): number {
  if (!Number.isFinite(value)) return 1;
  return Math.round(Math.max(0.25, Math.min(4, value)) * 100) / 100;
}
