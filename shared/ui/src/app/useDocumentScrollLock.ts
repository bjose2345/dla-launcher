import { useEffect } from "react";

let activeLocks = 0;
let previousBodyOverflow: string | null = null;
let previousRootOverflow: string | null = null;

export function useDocumentScrollLock(active: boolean): void {
  useEffect(() => {
    if (!active) return;
    return acquireDocumentScrollLock();
  }, [active]);
}

function acquireDocumentScrollLock(): () => void {
  if (activeLocks === 0) {
    previousBodyOverflow = document.body.style.overflow;
    previousRootOverflow = document.documentElement.style.overflow;
    document.body.style.overflow = "hidden";
    document.documentElement.style.overflow = "hidden";
  }
  activeLocks += 1;

  let released = false;
  return () => {
    if (released) return;
    released = true;
    activeLocks -= 1;
    if (activeLocks !== 0) return;
    document.body.style.overflow = previousBodyOverflow ?? "";
    document.documentElement.style.overflow = previousRootOverflow ?? "";
    previousBodyOverflow = null;
    previousRootOverflow = null;
  };
}
