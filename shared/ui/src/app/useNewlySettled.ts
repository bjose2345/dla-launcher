import { useCallback, useEffect, useRef, useState } from "react";

/**
 * The first item that reached one of the `settled` statuses since the list
 * was last seen, until dismissed. Items already settled when the list first
 * arrived are not news; one created since is, even when it settled before
 * the list first showed it.
 */
export function useNewlySettled<T, S extends string>(
  items: readonly T[] | undefined,
  id: (item: T) => string,
  status: (item: T) => S,
  settled: readonly S[],
): [T | null, () => void] {
  const [found, setFound] = useState<T | null>(null);
  const seen = useRef<Map<string, S> | null>(null);
  const read = useRef({ id, status, settled });
  read.current = { id, status, settled };

  useEffect(() => {
    if (!items) return;
    const { id, status, settled } = read.current;
    const previous = seen.current;
    seen.current = new Map(items.map((item) => [id(item), status(item)]));
    if (!previous) return;
    const next = items.find((item) => (
      settled.includes(status(item)) && previous.get(id(item)) !== status(item)
    ));
    if (next) setFound(next);
  }, [items]);

  const dismiss = useCallback(() => setFound(null), []);
  return [found, dismiss];
}
