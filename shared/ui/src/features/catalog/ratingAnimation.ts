export const ratingCountDuration = 1400;
export const ratingCountDelay = 450;

export function ratingCountValue(target: number, elapsed: number): number {
  const progress = Math.min(1, Math.max(0, (elapsed - ratingCountDelay) / ratingCountDuration));
  return target * (1 - Math.pow(1 - progress, 3));
}
