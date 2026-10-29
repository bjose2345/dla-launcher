import { useCallback, useEffect, useLayoutEffect, useMemo, useState, type RefObject } from "react";

import { useKeyBindings } from "../../preferences/KeyBindingsProvider";
import { actionsInScope, bindingsForAction, formatCombo } from "../../preferences/keyBindings";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { PageBox } from "./readerModel";

/**
 * Page keys work while a reader button has focus, so a click does not turn
 * the keyboard off; Space and Enter still press a focused button, and the
 * slider keeps its own arrow keys.
 */
export function guardKeys<T extends Record<string, () => void>>(handlers: T): Record<keyof T, (event: KeyboardEvent) => void> {
  const guarded = {} as Record<keyof T, (event: KeyboardEvent) => void>;
  for (const [id, handler] of Object.entries(handlers) as Array<[keyof T, () => void]>) {
    guarded[id] = (event) => {
      if (!(event.target instanceof Element)) {
        handler();
        return;
      }
      const field = event.target.closest("input, select, textarea, [contenteditable]:not([contenteditable='false'])");
      const button = event.target.closest("button, a[href]");
      if (field || (button && (event.key === " " || event.key === "Enter"))) return;
      handler();
    };
  }
  return guarded;
}

/** The keys of the reader scope this viewer answers to, as the user bound them. */
export function ReaderShortcuts({ actions }: { actions: ReadonlySet<string> }) {
  const { t } = usePresentation();
  const { bindings } = useKeyBindings();
  return (
    <section className="reader-panel reader-shortcuts" aria-label={t("reader.shortcuts")}>
      <dl>
        {actionsInScope("reader").map((action) => {
          const combos = bindingsForAction(bindings, action.id);
          if (!combos.length || !actions.has(action.id)) return null;
          return (
            <div key={action.id}>
              <dt>{combos.map(formatCombo).join(" · ")}</dt>
              <dd>{t(action.labelKey)}</dd>
            </div>
          );
        })}
      </dl>
    </section>
  );
}

export function useElementSize(ref: RefObject<HTMLElement | null>): PageBox {
  const [size, setSize] = useState<PageBox>({ width: 0, height: 0 });
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const measure = () => {
      const next = { width: element.clientWidth, height: element.clientHeight };
      setSize((current) => (current.width === next.width && current.height === next.height ? current : next));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);
  return size;
}

export function useFullscreen(ref: RefObject<HTMLElement | null>) {
  const [active, setActive] = useState(() => Boolean(document.fullscreenElement));
  useEffect(() => {
    const onChange = () => setActive(Boolean(document.fullscreenElement));
    document.addEventListener("fullscreenchange", onChange);
    return () => document.removeEventListener("fullscreenchange", onChange);
  }, []);
  const toggle = useCallback(() => {
    if (document.fullscreenElement) void document.exitFullscreen().catch(() => undefined);
    else void ref.current?.closest<HTMLElement>(".reader-overlay")?.requestFullscreen().catch(() => undefined);
  }, [ref]);
  return useMemo(() => ({ supported: Boolean(document.fullscreenEnabled), active, toggle }), [active, toggle]);
}

/** Escape closes an open panel before it closes the reader. */
export function useEscapeClosesPanel(open: boolean, close: () => void) {
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      close();
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [close, open]);
}

export function scrollBehavior(): ScrollBehavior {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
}
