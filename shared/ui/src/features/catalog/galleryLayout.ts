export const defaultGalleryRatio = 4 / 3;

export function galleryColumnCount(width: number): 1 | 2 | 3 {
  if (width >= 1024) return 3;
  if (width > 520) return 2;
  return 1;
}

export function balanceGalleryColumns(ratios: readonly number[], requestedColumns: number): number[][] {
  if (!ratios.length) return [];
  const columnCount = Math.max(1, Math.min(3, Math.floor(requestedColumns), ratios.length));
  if (columnCount === 1) return [ratios.map((_, index) => index)];

  const heights = ratios.map((ratio) => Number.isFinite(ratio) && ratio > 0 ? 1 / ratio : 1 / defaultGalleryRatio);
  const prefix = [0];
  for (const height of heights) prefix.push((prefix.at(-1) ?? 0) + height);

  let bestCuts: number[] | null = null;
  let bestRange = Number.POSITIVE_INFINITY;
  let bestPeak = Number.POSITIVE_INFINITY;

  const evaluate = (cuts: number[]) => {
    const boundaries = [0, ...cuts, ratios.length];
    const columnHeights = boundaries.slice(0, -1).map((start, index) => {
      const end = boundaries[index + 1] ?? start;
      return (prefix[end] ?? 0) - (prefix[start] ?? 0);
    });
    const peak = Math.max(...columnHeights);
    const range = peak - Math.min(...columnHeights);
    const betterBalance = range < bestRange - Number.EPSILON;
    const equalBalance = Math.abs(range - bestRange) <= Number.EPSILON;
    const lowerPeak = peak < bestPeak - Number.EPSILON;
    const laterCuts = equalBalance && Math.abs(peak - bestPeak) <= Number.EPSILON && compareCuts(cuts, bestCuts) > 0;
    if (betterBalance || (equalBalance && lowerPeak) || laterCuts) {
      bestCuts = [...cuts];
      bestRange = range;
      bestPeak = peak;
    }
  };

  const chooseCuts = (cuts: number[], next: number) => {
    if (cuts.length === columnCount - 1) {
      evaluate(cuts);
      return;
    }
    const cutsRemaining = columnCount - cuts.length - 1;
    const last = ratios.length - cutsRemaining;
    for (let cut = next; cut <= last; cut += 1) chooseCuts([...cuts, cut], cut + 1);
  };

  chooseCuts([], 1);
  const boundaries = [0, ...(bestCuts ?? []), ratios.length];
  return boundaries.slice(0, -1).map((start, index) => {
    const end = boundaries[index + 1] ?? start;
    return Array.from({ length: end - start }, (_, offset) => start + offset);
  });
}

function compareCuts(left: readonly number[], right: readonly number[] | null): number {
  if (!right) return 1;
  for (let index = 0; index < left.length; index += 1) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference !== 0) return difference;
  }
  return 0;
}
