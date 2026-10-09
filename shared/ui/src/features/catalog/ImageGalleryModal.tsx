import {
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  Keyboard,
  LayoutGrid,
  Maximize,
  Pause,
  Play,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { ArchiveImagePlaceholder } from "./ArchiveImagePlaceholder";

type Point = { x: number; y: number };
type Box = { width: number; height: number };
type ZoomState = { zoom: number; pan: Point };

const softEase = [0.16, 1, 0.3, 1] as const;
const softEaseCss = "cubic-bezier(0.16, 1, 0.3, 1)";
const fanStep = 12;
const visibleCards = 2;
const fanPivot = "50% 235%";
const fanEase = "cubic-bezier(0.24, 1.1, 0.32, 1)";
const swipePixels = 60;
const defaultModalRatio = 3 / 4;

// Zoom: the center card grows to fill the stage and the image scales inside
// it. Buttons and keys walk zoomSteps; the wheel and pinch zoom continuously
// up to maxZoom. A second tap within doubleTapMs toggles 2x there.
const zoomSteps = [1, 1.5, 2, 3, 4];
const maxZoom = 4;
const doubleTapMs = 300;
const panKeyPixels = 80;
const slideMs = 4500;
const legendSeenKey = "dla-launcher:gallery-legend:v1";
const legendMs = 6000;

const clamp = (value: number, minimum: number, maximum: number) =>
  Math.max(minimum, Math.min(maximum, value));

/** The largest box of `ratio` inside `bounds` (object-fit: contain). */
const containIn = (ratio: number, bounds: Box): Box => {
  let width = bounds.width;
  let height = width / ratio;
  if (height > bounds.height) {
    height = bounds.height;
    width = height * ratio;
  }
  return { width, height };
};

/** Keeps a zoomed image covering its box: no empty edge can be dragged in. */
const clampPan = (pan: Point, zoom: number, box: Box): Point => {
  const maxX = ((zoom - 1) * box.width) / 2;
  const maxY = ((zoom - 1) * box.height) / 2;
  return { x: clamp(pan.x, -maxX, maxX), y: clamp(pan.y, -maxY, maxY) };
};

export function ImageGalleryModal({
  images,
  openIndex,
  title,
  onClose,
  onOpenOriginal,
}: {
  images: string[][];
  openIndex: number | null;
  title: string;
  onClose: () => void;
  /** Opens the image the center card shows, at full size, outside the app. */
  onOpenOriginal?: (url: string) => void | Promise<void>;
}) {
  const { t } = usePresentation();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const railRef = useRef<HTMLDivElement>(null);
  const thumbRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const dragStart = useRef<number | null>(null);
  const dragMoved = useRef(false);
  const pointers = useRef(new Map<number, Point>());
  const pinch = useRef<{ distance: number; from: ZoomState } | null>(null);
  const panStart = useRef<{ at: Point; from: Point } | null>(null);
  const lastTap = useRef<{ time: number; at: Point } | null>(null);
  const reduceMotion = useReducedMotion() ?? false;
  const open = openIndex !== null;
  const total = images.length;
  const multiple = total > 1;
  const [current, setCurrent] = useState(0);
  const [dragOffset, setDragOffset] = useState(0);
  const [dragging, setDragging] = useState(false);
  const [dealt, setDealt] = useState(false);
  const [dealSettled, setDealSettled] = useState(false);
  const [ratios, setRatios] = useState<Record<number, number>>({});
  const [resolved, setResolved] = useState<Record<number, string>>({});
  const [viewport, setViewport] = useState(() => ({
    width: typeof window === "undefined" ? 1280 : window.innerWidth,
    height: typeof window === "undefined" ? 800 : window.innerHeight,
  }));
  const [stage, setStage] = useState<Box>({ width: 1280, height: 600 });
  // Zoom (1 is the fan) and the pan offset of the zoomed image, in pixels
  // from its box centre. `gesturing` drops the easing while a pointer drives
  // the image directly.
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState<Point>({ x: 0, y: 0 });
  const [gesturing, setGesturing] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [pageHidden, setPageHidden] = useState(false);
  const [gridOpen, setGridOpen] = useState(false);
  const [legendOpen, setLegendOpen] = useState(false);
  const zoomed = zoom > 1.001;
  const finePointer = useMemo(
    () => typeof window !== "undefined"
      && window.matchMedia?.("(hover: hover) and (pointer: fine)").matches === true,
    [],
  );

  const liveOpen = useRef(open);
  liveOpen.current = open;
  const liveClose = useRef(onClose);
  liveClose.current = onClose;

  useEffect(() => {
    if (openIndex === null) return;
    setCurrent(Math.max(0, Math.min(openIndex, total - 1)));
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) {
      dialog.showModal();
      dialog.focus();
    }
  }, [openIndex, total]);

  // A dialog the platform closed on its own still closes the gallery.
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return undefined;
    const closed = () => {
      if (liveOpen.current) liveClose.current();
    };
    dialog.addEventListener("close", closed);
    return () => dialog.removeEventListener("close", closed);
  }, []);

  useEffect(() => {
    if (!open) {
      setDealt(false);
      setDealSettled(false);
      return;
    }
    if (reduceMotion) {
      setDealt(true);
      setDealSettled(true);
      return;
    }
    const dealTimer = window.setTimeout(() => setDealt(true), 520);
    const settleTimer = window.setTimeout(() => setDealSettled(true), 1350);
    return () => {
      window.clearTimeout(dealTimer);
      window.clearTimeout(settleTimer);
    };
  }, [open, reduceMotion]);

  useEffect(() => {
    if (!open) return;
    const resize = () => setViewport({ width: window.innerWidth, height: window.innerHeight });
    resize();
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [open]);

  // The zoomed card fills the stage, so the stage box itself is tracked.
  useEffect(() => {
    if (!open) return undefined;
    const element = stageRef.current;
    if (!element || typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setStage({ width: entry.contentRect.width, height: entry.contentRect.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [open]);

  // Every image opens at fit; closing ends the slideshow and the overview.
  useEffect(() => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
  }, [current, open]);
  useEffect(() => {
    if (open) return;
    setPlaying(false);
    setGridOpen(false);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const rail = railRef.current;
    const thumb = thumbRefs.current[current];
    if (!rail || !thumb) return;
    rail.scrollTo?.({
      left: thumb.offsetLeft - rail.clientWidth / 2 + thumb.clientWidth / 2,
      behavior: reduceMotion ? "auto" : "smooth",
    });
  }, [current, open, reduceMotion]);

  // Slideshow: one step every slideMs. Moving by hand restarts the clock;
  // zooming, the overview and a hidden window hold it.
  useEffect(() => {
    if (!open) return undefined;
    const visibility = () => setPageHidden(document.hidden);
    document.addEventListener("visibilitychange", visibility);
    return () => document.removeEventListener("visibilitychange", visibility);
  }, [open]);
  const slideRunning = open && playing && multiple && !zoomed && !gridOpen && !pageHidden;
  useEffect(() => {
    if (!slideRunning) return undefined;
    const timer = window.setTimeout(() => setCurrent((value) => (value + 1) % total), slideMs);
    return () => window.clearTimeout(timer);
  }, [current, slideRunning, total]);

  // The keyboard legend shows once with a mouse the first time the gallery
  // opens, then on demand. Blocked storage counts as seen.
  useEffect(() => {
    if (!open || !finePointer) {
      setLegendOpen(false);
      return undefined;
    }
    let seen = true;
    try {
      seen = window.localStorage.getItem(legendSeenKey) === "1";
      window.localStorage.setItem(legendSeenKey, "1");
    } catch {
      seen = true;
    }
    if (seen) return undefined;
    setLegendOpen(true);
    const timer = window.setTimeout(() => setLegendOpen(false), legendMs);
    return () => window.clearTimeout(timer);
  }, [finePointer, open]);

  // Equal area per card: a wide banner and a tall pillar carry the same
  // weight, so the fan's hierarchy never inverts.
  const sizeFor = (ratio: number) => {
    const base = Math.min(viewport.height * 0.62, 620);
    const area = base * base * 0.78;
    const maxWidth = Math.min(viewport.width * 0.8, 760);
    const maxHeight = Math.min(viewport.height * 0.7, 740);
    let width = Math.sqrt(area * ratio);
    let height = Math.sqrt(area / ratio);
    if (width > maxWidth) {
      width = maxWidth;
      height = width / ratio;
    }
    if (height > maxHeight) {
      height = maxHeight;
      width = height * ratio;
    }
    return { width, height };
  };

  const currentRatio = ratios[current] ?? defaultModalRatio;
  const zoomBox = containIn(currentRatio, { width: stage.width * 0.94, height: stage.height * 0.94 });

  // Zooms to `next`, keeping the image point under `at` (screen pixels) in
  // place. `from` is the state a pinch started from, or the current one.
  const zoomAt = (next: number, at?: Point, from: ZoomState = { zoom, pan }) => {
    const value = clamp(next, 1, maxZoom);
    if (value <= 1.001) {
      setZoom(1);
      setPan({ x: 0, y: 0 });
      return;
    }
    setPlaying(false);
    const box = stageRef.current?.getBoundingClientRect();
    const point = at && box
      ? { x: at.x - (box.left + box.width / 2), y: at.y - (box.top + box.height / 2) }
      : { x: 0, y: 0 };
    const scale = value / from.zoom;
    setZoom(value);
    setPan(clampPan(
      { x: point.x - scale * (point.x - from.pan.x), y: point.y - scale * (point.y - from.pan.y) },
      value,
      zoomBox,
    ));
  };
  const zoomStep = (direction: 1 | -1) => {
    const next = direction > 0
      ? zoomSteps.find((step) => step > zoom + 0.01)
      : [...zoomSteps].reverse().find((step) => step < zoom - 0.01);
    zoomAt(next ?? (direction > 0 ? maxZoom : 1));
  };
  const resetZoom = () => zoomAt(1);
  const toggleZoom = (at?: Point) => (zoomed ? resetZoom() : zoomAt(2, at));
  const panBy = (x: number, y: number) =>
    setPan((value) => clampPan({ x: value.x + x, y: value.y + y }, zoom, zoomBox));

  // The wheel and key listeners are bound once per open and read the live
  // state through refs.
  const live = useRef({ zoom, pan, zoomAt });
  live.current = { zoom, pan, zoomAt };
  const keyHandler = useRef<(event: KeyboardEvent) => void>(() => undefined);

  // Shortcuts listen on the document, so they keep working when a focused
  // control unmounts and focus falls back to the body.
  useEffect(() => {
    if (!open) return undefined;
    const key = (event: KeyboardEvent) => keyHandler.current(event);
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [open]);

  // The wheel turns the fan (throttled); once zoomed, or with Ctrl (a
  // trackpad pinch), it zooms towards the pointer.
  useEffect(() => {
    if (!open) return undefined;
    const element = stageRef.current;
    if (!element) return undefined;
    let lastTurn = 0;
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const state = live.current;
      if (event.ctrlKey || state.zoom > 1.001) {
        const factor = Math.exp(-event.deltaY * (event.ctrlKey ? 0.01 : 0.0015));
        state.zoomAt(state.zoom * factor, { x: event.clientX, y: event.clientY }, state);
        return;
      }
      if (!multiple) return;
      const now = performance.now();
      if (now - lastTurn < 320) return;
      lastTurn = now;
      setCurrent((value) => event.deltaY > 0 || event.deltaX > 0
        ? (value + 1) % total
        : (value - 1 + total) % total);
    };
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, [multiple, open, total]);

  if (!total) return null;

  const goPrevious = () => setCurrent((value) => multiple ? (value - 1 + total) % total : value);
  const goNext = () => setCurrent((value) => multiple ? (value + 1) % total : value);
  const centerWidth = sizeFor(currentRatio).width;

  // A tap on the center card; two in quick succession toggle zoom there. The
  // stage captures the pointer, so the card is hit-tested by position.
  const tap = (event: ReactPointerEvent) => {
    const card = stageRef.current?.querySelector("[data-center-card]")?.getBoundingClientRect();
    const at = { x: event.clientX, y: event.clientY };
    if (!card || at.x < card.left || at.x > card.right || at.y < card.top || at.y > card.bottom) return;
    const now = performance.now();
    const last = lastTap.current;
    if (last && now - last.time < doubleTapMs && Math.hypot(at.x - last.at.x, at.y - last.at.y) < 30) {
      lastTap.current = null;
      toggleZoom(at);
    } else {
      lastTap.current = { time: now, at };
    }
  };

  const pointerDown = (event: ReactPointerEvent) => {
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    stageRef.current?.setPointerCapture?.(event.pointerId);
    // A second finger turns any drag in flight into a pinch.
    if (pointers.current.size === 2) {
      const [first, second] = [...pointers.current.values()];
      if (first && second) {
        pinch.current = {
          distance: Math.hypot(first.x - second.x, first.y - second.y),
          from: { zoom, pan },
        };
      }
      dragStart.current = null;
      panStart.current = null;
      setDragging(false);
      setDragOffset(0);
      setGesturing(true);
      return;
    }
    if (pointers.current.size > 1) return;
    dragMoved.current = false;
    if (zoomed) {
      panStart.current = { at: { x: event.clientX, y: event.clientY }, from: pan };
      setGesturing(true);
      return;
    }
    if (!multiple) return;
    dragStart.current = event.clientX;
    setDragging(true);
  };
  const pointerMove = (event: ReactPointerEvent) => {
    if (!pointers.current.has(event.pointerId)) return;
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    const pinching = pinch.current;
    if (pinching && pointers.current.size >= 2) {
      const [first, second] = [...pointers.current.values()];
      if (!first || !second) return;
      const distance = Math.hypot(first.x - second.x, first.y - second.y);
      zoomAt(
        (pinching.from.zoom * distance) / pinching.distance,
        { x: (first.x + second.x) / 2, y: (first.y + second.y) / 2 },
        pinching.from,
      );
      return;
    }
    const panning = panStart.current;
    if (panning) {
      const x = event.clientX - panning.at.x;
      const y = event.clientY - panning.at.y;
      if (Math.hypot(x, y) > 6) dragMoved.current = true;
      setPan(clampPan({ x: panning.from.x + x, y: panning.from.y + y }, zoom, zoomBox));
      return;
    }
    if (dragStart.current === null) return;
    const distance = event.clientX - dragStart.current;
    if (Math.abs(distance) > 6) dragMoved.current = true;
    setDragOffset(-distance / centerWidth);
  };
  const pointerUp = (event: ReactPointerEvent) => {
    if (!pointers.current.delete(event.pointerId)) return;
    if (pinch.current) {
      if (pointers.current.size < 2) {
        pinch.current = null;
        setGesturing(false);
        if (zoom < 1.05) resetZoom();
      }
      return;
    }
    if (panStart.current) {
      panStart.current = null;
      setGesturing(false);
      if (!dragMoved.current) tap(event);
      return;
    }
    if (dragStart.current === null) {
      tap(event);
      return;
    }
    const distance = event.clientX - dragStart.current;
    dragStart.current = null;
    setDragging(false);
    setDragOffset(0);
    if (distance < -swipePixels) goNext();
    else if (distance > swipePixels) goPrevious();
    else if (!dragMoved.current) tap(event);
    window.setTimeout(() => {
      dragMoved.current = false;
    }, 50);
  };

  keyHandler.current = (event: KeyboardEvent) => {
    // Space still presses a focused control; it only drives the slideshow
    // from the dialog itself.
    const onControl = (event.target as Element | null)?.closest?.("button, a, input");
    switch (event.key) {
      case "ArrowLeft":
        if (zoomed) panBy(panKeyPixels, 0);
        else goPrevious();
        break;
      case "ArrowRight":
        if (zoomed) panBy(-panKeyPixels, 0);
        else goNext();
        break;
      case "ArrowUp":
        if (zoomed) {
          event.preventDefault();
          panBy(0, panKeyPixels);
        }
        break;
      case "ArrowDown":
        if (zoomed) {
          event.preventDefault();
          panBy(0, -panKeyPixels);
        }
        break;
      case "+":
      case "=":
        zoomStep(1);
        break;
      case "-":
      case "_":
        zoomStep(-1);
        break;
      case "0":
        resetZoom();
        break;
      case "z":
      case "Z":
        toggleZoom();
        break;
      case " ":
        if (!onControl && multiple) {
          event.preventDefault();
          setPlaying((value) => !value);
        }
        break;
      case "g":
      case "G":
        if (multiple) setGridOpen((value) => !value);
        break;
      case "?":
        setLegendOpen((value) => !value);
        break;
      case "Escape":
        // One layer at a time: the overview, then zoom, then the gallery.
        if (gridOpen) {
          event.preventDefault();
          setGridOpen(false);
        } else if (zoomed) {
          event.preventDefault();
          resetZoom();
        }
        break;
      default:
        break;
    }
  };

  const backdropChain = images[current] ?? [];
  // "Full size" opens the source the center card actually loaded.
  const originalSource = resolved[current] ?? images[current]?.[0];
  const legend: Array<[string, string]> = [
    ["← →", t("detail.galleryKeyBrowse")],
    ["Z", t("detail.galleryKeyZoom")],
    [t("detail.galleryKeySpace"), t("detail.galleryKeySlideshow")],
    ["G", t("detail.galleryKeyGrid")],
    ["Esc", t("detail.galleryKeyClose")],
  ];
  const slideshowLabel = playing ? t("detail.galleryPause") : t("detail.galleryPlay");

  return (
    <dialog
      ref={dialogRef}
      className="gallery-modal"
      aria-label={title}
      tabIndex={-1}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <AnimatePresence onExitComplete={() => dialogRef.current?.close()}>
        {open && (
          <motion.div
            className="gallery-modal-sheet"
            initial={reduceMotion ? { opacity: 0 } : { y: "100%", opacity: 1 }}
            animate={reduceMotion ? { opacity: 1 } : { y: 0 }}
            exit={reduceMotion ? { opacity: 0 } : { y: "110%" }}
            transition={{ duration: reduceMotion ? 0.01 : 0.45, ease: softEase }}
          >
            <div className="gallery-modal-backdrop" aria-hidden="true">
              <AnimatePresence initial={false}>
                {backdropChain.length > 0 && (
                  <GalleryBackdrop
                    chain={backdropChain}
                    reduceMotion={reduceMotion}
                    key={`${current}:${backdropChain.join("|")}`}
                  />
                )}
              </AnimatePresence>
              <i />
            </div>
            <header className="gallery-modal-header">
              <span>{current + 1} / {total}</span>
              <strong>{title}</strong>
              <div className="gallery-modal-controls">
                <div className="gallery-zoom-controls">
                  <button
                    type="button"
                    onClick={() => zoomStep(-1)}
                    disabled={!zoomed}
                    aria-label={t("detail.galleryZoomOut")}
                    title={t("detail.galleryZoomOut")}
                  ><ZoomOut aria-hidden="true" /></button>
                  <span>{zoomed ? `${Math.round(zoom * 100)}%` : t("detail.galleryZoomFitShort")}</span>
                  <button
                    type="button"
                    onClick={() => zoomStep(1)}
                    disabled={zoom >= maxZoom - 0.001}
                    aria-label={t("detail.galleryZoomIn")}
                    title={t("detail.galleryZoomIn")}
                  ><ZoomIn aria-hidden="true" /></button>
                  <button
                    type="button"
                    onClick={resetZoom}
                    disabled={!zoomed}
                    aria-label={t("detail.galleryZoomFit")}
                    title={t("detail.galleryZoomFit")}
                  ><Maximize aria-hidden="true" /></button>
                </div>
                {multiple && (
                  <button
                    className="gallery-round-button"
                    type="button"
                    onClick={() => setPlaying((value) => !value)}
                    aria-label={slideshowLabel}
                    title={slideshowLabel}
                  >
                    {playing ? <Pause aria-hidden="true" /> : <Play aria-hidden="true" />}
                    {slideRunning && (
                      // Time left on this slide; restarts with every step.
                      <svg className="gallery-slide-progress" viewBox="0 0 42 42" aria-hidden="true" key={current}>
                        <circle cx="21" cy="21" r="20" pathLength={100} style={{ animationDuration: `${slideMs}ms` }} />
                      </svg>
                    )}
                  </button>
                )}
                {multiple && (
                  <button
                    className="gallery-round-button"
                    type="button"
                    onClick={() => setGridOpen((value) => !value)}
                    aria-pressed={gridOpen}
                    aria-label={t("detail.galleryShowAll")}
                    title={t("detail.galleryShowAll")}
                  ><LayoutGrid aria-hidden="true" /></button>
                )}
                {onOpenOriginal && originalSource && (
                  <button
                    className="gallery-round-button gallery-full-size"
                    type="button"
                    onClick={() => void onOpenOriginal(originalSource)}
                    aria-label={t("detail.galleryOpenOriginal")}
                    title={t("detail.galleryOpenOriginal")}
                  >
                    <ExternalLink aria-hidden="true" />
                    <span>{t("detail.galleryFullSize")}</span>
                  </button>
                )}
                {finePointer && (
                  <button
                    className="gallery-round-button"
                    type="button"
                    onClick={() => setLegendOpen((value) => !value)}
                    aria-pressed={legendOpen}
                    aria-label={t("detail.galleryShortcuts")}
                    title={t("detail.galleryShortcuts")}
                  ><Keyboard aria-hidden="true" /></button>
                )}
                <button
                  className="gallery-round-button"
                  type="button"
                  onClick={onClose}
                  aria-label={t("detail.galleryClose")}
                  title={t("detail.galleryClose")}
                ><X aria-hidden="true" /></button>
              </div>
            </header>
            <div
              ref={stageRef}
              className={`gallery-fan-stage${zoomed ? " is-zoomed" : ""}${dragging || gesturing ? " is-dragging" : ""}`}
              onPointerDown={pointerDown}
              onPointerMove={pointerMove}
              onPointerUp={pointerUp}
              onPointerCancel={pointerUp}
            >
              {images.map((chain, index) => {
                const distance = index - current - dragOffset;
                const clamped = Math.max(-(visibleCards + 1), Math.min(visibleCards + 1, distance));
                const center = Math.round(distance) === 0;
                // Zoomed, the center card has the stage to itself.
                const hidden = Math.abs(distance) > visibleCards + 0.9 || (zoomed && !center);
                const absoluteDistance = Math.abs(clamped);
                const expanded = zoomed && center;
                const size = expanded ? zoomBox : sizeFor(ratios[index] ?? defaultModalRatio);
                const delay = !reduceMotion && dealt && !dealSettled ? `${Math.min(absoluteDistance, 3) * 0.06}s` : "0s";
                const dimOpacity = Math.min(absoluteDistance * 0.28, 0.6);
                return (
                  <div
                    className="gallery-fan-card"
                    key={`${chain[0]}:${index}`}
                    aria-hidden={!center || undefined}
                    data-center-card={center || undefined}
                    onClick={() => {
                      if (!center && !dragMoved.current) setCurrent(index);
                    }}
                    style={{
                      top: expanded ? "50%" : undefined,
                      width: size.width,
                      height: size.height,
                      marginLeft: -size.width / 2,
                      marginTop: -size.height / 2,
                      transform: `rotate(${dealt ? clamped * fanStep : 0}deg)`,
                      transformOrigin: fanPivot,
                      zIndex: 40 - Math.round(absoluteDistance * 10),
                      opacity: hidden ? 0 : 1,
                      pointerEvents: hidden ? "none" : "auto",
                      transition: dragging || reduceMotion
                        ? "opacity 0.4s ease"
                        : `transform 0.55s ${fanEase}, opacity 0.4s ease, width 0.45s ${softEaseCss}, height 0.45s ${softEaseCss}, margin 0.45s ${softEaseCss}, top 0.45s ${softEaseCss}`,
                      transitionDelay: delay,
                      cursor: center ? undefined : "pointer",
                    }}
                  >
                    <div style={{
                      transform: `scale(${1 - Math.min(absoluteDistance, 2.4) * 0.12})`,
                      transition: dragging || reduceMotion ? "none" : `transform 0.55s ${fanEase}`,
                      transitionDelay: delay,
                    }}>
                      <div
                        className="gallery-fan-card-image"
                        style={{
                          transform: expanded ? `translate(${pan.x}px, ${pan.y}px) scale(${zoom})` : undefined,
                          transition: gesturing || reduceMotion ? "none" : `transform 0.35s ${softEaseCss}`,
                        }}
                      >
                        <GalleryCardImage
                          chain={chain}
                          alt={center ? title : ""}
                          unavailableLabel={center ? t("detail.galleryNoImageFor", { title }) : ""}
                          onRatio={(ratio) => setRatios((values) => Math.abs((values[index] ?? 0) - ratio) < 0.001 ? values : { ...values, [index]: ratio })}
                          onResolved={(source) => setResolved((values) => values[index] === source ? values : { ...values, [index]: source })}
                        />
                      </div>
                      <i
                        className="gallery-fan-card-dimmer"
                        style={{
                          opacity: dimOpacity,
                          transition: dragging || reduceMotion ? "none" : "opacity 0.4s ease",
                          transitionDelay: delay,
                        }}
                      />
                    </div>
                  </div>
                );
              })}
              {zoomed && (
                <GalleryMinimap
                  chain={images[current] ?? []}
                  ratio={currentRatio}
                  zoom={zoom}
                  pan={pan}
                  box={zoomBox}
                  compact={stage.width < 640}
                  label={t("detail.galleryMinimap")}
                  onPan={(value) => setPan(clampPan(value, zoom, zoomBox))}
                />
              )}
            </div>
            {multiple && !zoomed && !gridOpen && (
              <>
                <button className="gallery-side-arrow previous" type="button" onClick={goPrevious} aria-label={t("detail.galleryPrevious")}><ChevronLeft /></button>
                <button className="gallery-side-arrow next" type="button" onClick={goNext} aria-label={t("detail.galleryNext")}><ChevronRight /></button>
              </>
            )}
            <AnimatePresence>
              {legendOpen && (
                <motion.div
                  className={`gallery-legend${multiple ? " above-filmstrip" : ""}`}
                  key="legend"
                  initial={{ opacity: 0, y: 8 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: 8 }}
                  transition={{ duration: reduceMotion ? 0 : 0.25 }}
                >
                  {legend.map(([key, label]) => (
                    <span key={key}><kbd>{key}</kbd>{label}</span>
                  ))}
                </motion.div>
              )}
            </AnimatePresence>
            {multiple && (
              <div className="gallery-filmstrip" ref={railRef}>
                {images.map((chain, index) => (
                  <button
                    type="button"
                    className={index === current ? "is-current" : ""}
                    key={`${chain[0]}:thumb`}
                    ref={(element) => { thumbRefs.current[index] = element; }}
                    onClick={() => setCurrent(index)}
                    aria-label={t("detail.galleryGoTo", { number: index + 1 })}
                    aria-current={index === current ? "true" : undefined}
                    style={{ width: 48 * (ratios[index] ?? defaultModalRatio) }}
                  >
                    <GalleryCardImage chain={chain} alt="" unavailableLabel="" />
                  </button>
                ))}
              </div>
            )}
            <AnimatePresence>
              {gridOpen && (
                // Every image at once; picking one returns to the fan on it.
                <motion.div
                  className="gallery-overview"
                  key="overview"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: reduceMotion ? 0 : 0.2 }}
                >
                  <div>
                    {images.map((chain, index) => (
                      <motion.button
                        type="button"
                        className={index === current ? "is-current" : ""}
                        key={`${chain[0]}:overview`}
                        initial={reduceMotion ? false : { opacity: 0, y: 10 }}
                        animate={{ opacity: 1, y: 0 }}
                        transition={{
                          duration: 0.3,
                          delay: reduceMotion ? 0 : Math.min(index, 12) * 0.025,
                          ease: softEase,
                        }}
                        onClick={() => {
                          setCurrent(index);
                          setGridOpen(false);
                          dialogRef.current?.focus({ preventScroll: true });
                        }}
                        aria-label={t("detail.galleryGoTo", { number: index + 1 })}
                        aria-current={index === current ? "true" : undefined}
                      >
                        <GalleryCardImage chain={chain} alt="" unavailableLabel={t("detail.galleryImageUnavailable")} />
                        <span>{index + 1}</span>
                      </motion.button>
                    ))}
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
            <span className="screen-reader-only" aria-live="polite">
              {t("detail.galleryPosition", { current: current + 1, total })}
            </span>
          </motion.div>
        )}
      </AnimatePresence>
    </dialog>
  );
}

/**
 * The zoom navigator: the whole image with a frame around the part on
 * screen. Pressing or dragging anywhere on it centres the view there.
 */
function GalleryMinimap({
  chain,
  ratio,
  zoom,
  pan,
  box,
  compact,
  label,
  onPan,
}: {
  chain: string[];
  ratio: number;
  zoom: number;
  pan: Point;
  box: Box;
  compact: boolean;
  label: string;
  onPan: (pan: Point) => void;
}) {
  const size = containIn(ratio, compact ? { width: 112, height: 88 } : { width: 168, height: 126 });
  const frameWidth = size.width / zoom;
  const frameHeight = size.height / zoom;
  const centreX = size.width / 2 - (pan.x / (zoom * box.width)) * size.width;
  const centreY = size.height / 2 - (pan.y / (zoom * box.height)) * size.height;
  const dragging = useRef(false);
  const panTo = (event: ReactPointerEvent<HTMLDivElement>) => {
    const area = event.currentTarget.getBoundingClientRect();
    const x = event.clientX - area.left;
    const y = event.clientY - area.top;
    onPan({
      x: ((size.width / 2 - x) / size.width) * zoom * box.width,
      y: ((size.height / 2 - y) / size.height) * zoom * box.height,
    });
  };
  return (
    <div className="gallery-minimap">
      <div
        role="img"
        aria-label={label}
        style={{ width: size.width, height: size.height }}
        onPointerDown={(event) => {
          event.stopPropagation();
          dragging.current = true;
          event.currentTarget.setPointerCapture?.(event.pointerId);
          panTo(event);
        }}
        onPointerMove={(event) => {
          if (dragging.current) panTo(event);
        }}
        onPointerUp={() => {
          dragging.current = false;
        }}
        onPointerCancel={() => {
          dragging.current = false;
        }}
      >
        <GalleryCardImage chain={chain} alt="" unavailableLabel="" />
        <span
          aria-hidden="true"
          style={{
            width: frameWidth,
            height: frameHeight,
            left: centreX - frameWidth / 2,
            top: centreY - frameHeight / 2,
          }}
        />
      </div>
    </div>
  );
}

function GalleryBackdrop({ chain, reduceMotion }: { chain: string[]; reduceMotion: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [sourceIndex, setSourceIndex] = useState(0);
  const [ready, setReady] = useState(false);
  const source = chain[sourceIndex];

  useEffect(() => {
    setSourceIndex(0);
    setReady(false);
  }, [chain]);

  useEffect(() => {
    if (!source) return;
    let active = true;
    const image = new Image();
    image.decoding = "async";
    image.onload = () => {
      if (!active) return;
      const canvas = canvasRef.current;
      const context = canvas?.getContext("2d");
      if (!canvas || !context || !image.naturalWidth || !image.naturalHeight) return;
      const width = 640;
      const height = 360;
      const sampleWidth = 96;
      const sampleHeight = 54;
      const sample = document.createElement("canvas");
      sample.width = sampleWidth;
      sample.height = sampleHeight;
      const sampleContext = sample.getContext("2d");
      if (!sampleContext) return;
      const sampleScale = Math.max(sampleWidth / image.naturalWidth, sampleHeight / image.naturalHeight);
      const sampleDrawWidth = image.naturalWidth * sampleScale;
      const sampleDrawHeight = image.naturalHeight * sampleScale;
      sampleContext.imageSmoothingEnabled = true;
      sampleContext.imageSmoothingQuality = "high";
      sampleContext.drawImage(
        image,
        (sampleWidth - sampleDrawWidth) / 2,
        (sampleHeight - sampleDrawHeight) / 2,
        sampleDrawWidth,
        sampleDrawHeight,
      );
      canvas.width = width;
      canvas.height = height;
      context.imageSmoothingEnabled = true;
      context.imageSmoothingQuality = "high";
      context.clearRect(0, 0, width, height);
      context.globalAlpha = 1 / 16;
      const spread = 22;
      for (let row = 0; row < 4; row += 1) {
        for (let column = 0; column < 4; column += 1) {
          const offsetX = ((column - 1.5) / 1.5) * spread;
          const offsetY = ((row - 1.5) / 1.5) * spread;
          context.drawImage(
            sample,
            -spread + offsetX,
            -spread + offsetY,
            width + spread * 2,
            height + spread * 2,
          );
        }
      }
      context.globalAlpha = 1;
      setReady(true);
    };
    image.onerror = () => {
      if (!active) return;
      setSourceIndex((value) => {
        const next = value + 1;
        return next < chain.length ? next : value;
      });
    };
    image.src = source;
    return () => {
      active = false;
      image.onload = null;
      image.onerror = null;
    };
  }, [chain.length, source]);

  return (
    <motion.canvas
      ref={canvasRef}
      className="gallery-modal-backdrop-art"
      initial={{ opacity: 0 }}
      animate={{ opacity: ready ? 0.52 : 0 }}
      exit={{ opacity: 0 }}
      transition={{ duration: reduceMotion ? 0 : 0.6 }}
    />
  );
}

function GalleryCardImage({
  chain,
  alt,
  unavailableLabel,
  onRatio,
  onResolved,
}: {
  chain: string[];
  alt: string;
  unavailableLabel: string;
  onRatio?: (ratio: number) => void;
  /** The chain entry that actually loaded. */
  onResolved?: (source: string) => void;
}) {
  const [sourceIndex, setSourceIndex] = useState(0);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    setSourceIndex(0);
    setFailed(false);
  }, [chain]);
  const source = chain[sourceIndex];
  if (failed || !source) {
    return <ArchiveImagePlaceholder className="gallery-image-unavailable" label={unavailableLabel} />;
  }
  return (
    <img
      src={source}
      alt={alt}
      loading="lazy"
      decoding="async"
      draggable={false}
      onLoad={(event) => {
        const image = event.currentTarget;
        if (image.naturalWidth && image.naturalHeight) onRatio?.(image.naturalWidth / image.naturalHeight);
        onResolved?.(source);
      }}
      onError={() => setSourceIndex((value) => {
        const next = value + 1;
        if (next < chain.length) return next;
        setFailed(true);
        return value;
      })}
    />
  );
}
