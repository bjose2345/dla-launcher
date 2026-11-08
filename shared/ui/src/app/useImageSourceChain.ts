import { useCallback, useState } from "react";

/**
 * Walks a list of image URLs: the current one to show, and `advance` to move
 * on when it fails to load. A new list, or a new `resetKey`, starts over at
 * the first URL in the same render, so no stale image shows in between. Once
 * every URL has failed `source` is undefined.
 */
export function useImageSourceChain(
  urls: readonly string[],
  resetKey: unknown = urls,
): { source: string | undefined; advance: () => void } {
  const [tracked, setTracked] = useState({ urls, resetKey, index: 0 });
  const current = tracked.urls === urls && tracked.resetKey === resetKey ? tracked : { urls, resetKey, index: 0 };
  if (current !== tracked) setTracked(current);
  const advance = useCallback(() => {
    setTracked((state) => ({ ...state, index: state.index + 1 }));
  }, []);
  return { source: current.urls[current.index], advance };
}
