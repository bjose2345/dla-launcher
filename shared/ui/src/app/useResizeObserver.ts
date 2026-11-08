import { useEffect, type DependencyList, type RefObject } from "react";

/**
 * Calls `onResize` with the element once it is there and again whenever its
 * size changes; `deps` say when to look for the element again, as for an
 * effect. The entry is null on the first call and where ResizeObserver is
 * missing, so measure the element itself when it matters.
 */
export function useResizeObserver<T extends Element>(
  ref: RefObject<T | null>,
  onResize: (element: T, entry: ResizeObserverEntry | null) => void,
  deps: DependencyList = [],
): void {
  useEffect(() => {
    const element = ref.current;
    if (!element) return undefined;
    onResize(element, null);
    if (typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(([entry]) => onResize(element, entry ?? null));
    observer.observe(element);
    return () => observer.disconnect();
  }, deps);
}
