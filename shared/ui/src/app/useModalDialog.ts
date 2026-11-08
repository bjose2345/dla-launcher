import { useEffect, useRef, type RefObject } from "react";

import { useDocumentScrollLock } from "./useDocumentScrollLock";

// Open dialogs, oldest first; Escape and Tab belong to the last one.
const openDialogs: symbol[] = [];

const focusableSelector = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

/**
 * What a modal dialog does while it is open: the page behind it does not
 * scroll, Tab stays inside it, Escape closes it when it is the dialog on top,
 * and the element that had focus gets it back when it closes. `initialFocus`
 * names what to focus on opening; without it the dialog element itself is
 * focused, so give that element `tabIndex={-1}`. Without `onClose` Escape is
 * left to the dialog's own keys.
 */
export function useModalDialog({
  open = true,
  dialog,
  onClose,
  initialFocus,
}: {
  open?: boolean;
  dialog: RefObject<HTMLElement | null>;
  onClose?: () => void;
  initialFocus?: () => HTMLElement | null | undefined;
}): void {
  useDocumentScrollLock(open);
  const onCloseRef = useRef(onClose);
  const initialFocusRef = useRef(initialFocus);
  onCloseRef.current = onClose;
  initialFocusRef.current = initialFocus;

  useEffect(() => {
    if (!open) return undefined;
    const token = Symbol("modal dialog");
    openDialogs.push(token);
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const frame = requestAnimationFrame(() => {
      (initialFocusRef.current?.() ?? dialog.current)?.focus({ preventScroll: true });
    });
    const onKeyDown = (event: KeyboardEvent) => {
      if (openDialogs[openDialogs.length - 1] !== token) return;
      if (event.key === "Escape") {
        if (!onCloseRef.current) return;
        event.preventDefault();
        onCloseRef.current();
      } else if (event.key === "Tab" && dialog.current) {
        keepTabInside(event, dialog.current);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("keydown", onKeyDown);
      openDialogs.splice(openDialogs.indexOf(token), 1);
      if (previousFocus?.isConnected) previousFocus.focus({ preventScroll: true });
    };
  }, [dialog, open]);
}

export function focusableElements(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>(focusableSelector)]
    .filter((element) => !element.hidden && element.tabIndex >= 0 && element.getAttribute("aria-hidden") !== "true");
}

function keepTabInside(event: KeyboardEvent, container: HTMLElement) {
  const focusable = focusableElements(container);
  if (!focusable.length) {
    event.preventDefault();
    container.focus();
    return;
  }
  const active = document.activeElement;
  const inside = container.contains(active);
  const first = focusable[0]!;
  const last = focusable[focusable.length - 1]!;
  if (event.shiftKey && (active === first || !inside)) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && (active === last || !inside)) {
    event.preventDefault();
    first.focus();
  }
}
