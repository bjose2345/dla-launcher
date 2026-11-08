import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
  type WheelEvent as ReactWheelEvent,
} from "react";

import { useKeyBindings } from "../../preferences/KeyBindingsProvider";
import { actionsInScope, bindingsForAction, formatCombo } from "../../preferences/keyBindings";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { PageBox } from "./readerModel";
import type { LibraryGateway } from "./types";

const WHEEL_TURN_DELAY_MS = 320;
const WHEEL_TURN_THRESHOLD = 35;
const DRAG_THRESHOLD_PX = 6;
const EDGE_REVEAL_TOP_PX = 90;
const RESUME_NOTICE_MS = 7_000;

interface DragState {
  pointerId: number;
  x: number;
  y: number;
  left: number;
  top: number;
  moved: boolean;
}

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

/**
 * What the pointer and the wheel do over a reader's stage: Ctrl+wheel zooms,
 * the wheel scrolls an overflowing page and otherwise turns it, a drag pans
 * the page, and the pointer near the top or bottom edge brings the controls
 * back. `zone` wraps a click action so a drag that ends on it does nothing.
 */
export function useStageGestures({
  stageRef,
  enabled,
  ignore,
  revealBottomPx,
  onZoom,
  onTurn,
  onReveal,
}: {
  stageRef: RefObject<HTMLElement | null>;
  enabled: boolean;
  /** Controls over the stage where a press must not start a drag. */
  ignore: string;
  revealBottomPx: number;
  onZoom: (direction: 1 | -1, point: { x: number; y: number }) => void;
  onTurn: (step: 1 | -1) => void;
  onReveal?: () => void;
}) {
  const dragRef = useRef<DragState | null>(null);
  const draggedRef = useRef(false);
  const wheelAtRef = useRef(0);

  const onWheel = (event: ReactWheelEvent<HTMLElement>) => {
    if (!enabled) return;
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      onZoom(event.deltaY < 0 ? 1 : -1, { x: event.clientX, y: event.clientY });
      return;
    }
    const scroller = stageRef.current;
    if (scroller && (scroller.scrollHeight > scroller.clientHeight + 1 || scroller.scrollWidth > scroller.clientWidth + 1)) {
      // The click zones sit over the page, so a zoomed page is scrolled here.
      event.preventDefault();
      scroller.scrollBy({ left: event.deltaX, top: event.deltaY });
      return;
    }
    const delta = Math.abs(event.deltaY) >= Math.abs(event.deltaX) ? event.deltaY : event.deltaX;
    if (Math.abs(delta) < WHEEL_TURN_THRESHOLD || Date.now() - wheelAtRef.current < WHEEL_TURN_DELAY_MS) return;
    wheelAtRef.current = Date.now();
    onTurn(delta > 0 ? 1 : -1);
  };
  const onPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    draggedRef.current = false;
    const scroller = stageRef.current;
    if (!enabled || event.button !== 0 || !scroller) return;
    if ((event.target as Element).closest(ignore)) return;
    dragRef.current = {
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      left: scroller.scrollLeft,
      top: scroller.scrollTop,
      moved: false,
    };
  };
  const onPointerMove = (event: ReactPointerEvent<HTMLElement>) => {
    if (onReveal && (event.clientY < EDGE_REVEAL_TOP_PX || event.clientY > window.innerHeight - revealBottomPx)) {
      onReveal();
    }
    const drag = dragRef.current;
    const scroller = stageRef.current;
    if (!drag || drag.pointerId !== event.pointerId || !scroller) return;
    const dx = event.clientX - drag.x;
    const dy = event.clientY - drag.y;
    if (!drag.moved && Math.hypot(dx, dy) < DRAG_THRESHOLD_PX) return;
    drag.moved = true;
    draggedRef.current = true;
    scroller.scrollLeft = drag.left - dx;
    scroller.scrollTop = drag.top - dy;
  };
  const onPointerUp = () => {
    dragRef.current = null;
  };
  const zone = (action: () => void) => () => {
    if (draggedRef.current) return;
    action();
  };
  // Clicks on the page leave focus on the reader, so the arrow keys keep
  // working and no focus ring lights up on a click area.
  const keepFocus = (event: ReactMouseEvent) => event.preventDefault();
  return { onWheel, onPointerDown, onPointerMove, onPointerUp, zone, keepFocus };
}

/** The "resumed where you left off" notice, shown for a while unless dismissed. */
export function useResumeNotice(initial: boolean): [boolean, () => void] {
  const [shown, setShown] = useState(initial);
  useEffect(() => {
    if (!shown) return;
    const timer = window.setTimeout(() => setShown(false), RESUME_NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [shown]);
  const dismiss = useCallback(() => setShown(false), []);
  return [shown, dismiss];
}

/** Fetches the images at `indexes` ahead of time; `onSize` learns each one's size. */
export function usePreloadedImages(
  indexes: number[],
  items: { ordinal: number }[],
  gateway: Pick<LibraryGateway, "mediaAssetUrl">,
  sessionId: string,
  onSize?: (index: number, size: PageBox) => void,
) {
  const latest = useRef(onSize);
  latest.current = onSize;
  const key = indexes.join(",");
  useEffect(() => {
    if (!key) return;
    const preloaders = key.split(",").map(Number).flatMap((index) => {
      const item = items[index];
      if (!item) return [];
      const image = new window.Image();
      image.decoding = "async";
      image.onload = () => latest.current?.(index, { width: image.naturalWidth, height: image.naturalHeight });
      image.src = gateway.mediaAssetUrl(sessionId, item.ordinal);
      return [image];
    });
    return () => preloaders.forEach((image) => {
      image.onload = null;
      image.src = "";
    });
  }, [gateway, items, key, sessionId]);
}
