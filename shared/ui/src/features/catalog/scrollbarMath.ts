export type ScrollMetrics = {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
};

export interface ScrollbarGeometry {
  thumbHeight: number;
  thumbOffset: number;
  maxScroll: number;
  maxThumbOffset: number;
}

export function calculateScrollbarGeometry(
  metrics: ScrollMetrics,
  trackHeight: number,
  minimumThumbHeight = 36,
): ScrollbarGeometry {
  const safeTrackHeight = Math.max(0, trackHeight);
  const maxScroll = Math.max(0, metrics.scrollHeight - metrics.clientHeight);
  if (safeTrackHeight === 0 || maxScroll === 0) {
    return {
      thumbHeight: safeTrackHeight,
      thumbOffset: 0,
      maxScroll,
      maxThumbOffset: 0,
    };
  }

  const proportionalHeight = safeTrackHeight * metrics.clientHeight / metrics.scrollHeight;
  const thumbHeight = Math.min(safeTrackHeight, Math.max(minimumThumbHeight, proportionalHeight));
  const maxThumbOffset = safeTrackHeight - thumbHeight;
  const scrollRatio = clamp(metrics.scrollTop / maxScroll, 0, 1);

  return {
    thumbHeight,
    thumbOffset: maxThumbOffset * scrollRatio,
    maxScroll,
    maxThumbOffset,
  };
}

export function scrollTopForThumbOffset(
  thumbOffset: number,
  geometry: ScrollbarGeometry,
): number {
  if (geometry.maxThumbOffset === 0) return 0;
  const ratio = clamp(thumbOffset / geometry.maxThumbOffset, 0, 1);
  return ratio * geometry.maxScroll;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
