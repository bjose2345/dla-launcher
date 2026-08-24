import { ChevronLeft, ChevronRight, X } from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { ArchiveImagePlaceholder } from "./ArchiveImagePlaceholder";
import { balanceGalleryColumns, defaultGalleryRatio, galleryColumnCount } from "./galleryLayout";

const softEase = [0.16, 1, 0.3, 1] as const;
const fanStep = 12;
const visibleCards = 2;
const fanPivot = "50% 235%";
const fanEase = "cubic-bezier(0.24, 1.1, 0.32, 1)";
const defaultModalRatio = 3 / 4;

type GalleryTileState = {
  sourceIndex: number;
  ratio: number;
  loaded: boolean;
  failed: boolean;
};

export function WorkGallery({ images, onOpen }: { images: string[][]; onOpen: (index: number) => void }) {
  const { t } = usePresentation();
  const rootRef = useRef<HTMLDivElement>(null);
  const imageKey = useMemo(() => images.map((chain) => chain.join("|")).join("||"), [images]);
  const [columnCount, setColumnCount] = useState<1 | 2 | 3>(() =>
    galleryColumnCount(typeof window === "undefined" ? 1024 : window.innerWidth),
  );
  const [states, setStates] = useState<GalleryTileState[]>(() => createTileStates(images));

  useEffect(() => setStates(createTileStates(images)), [imageKey]);

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const measure = () => setColumnCount(galleryColumnCount(root.getBoundingClientRect().width));
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    return () => observer.disconnect();
  }, []);

  const columns = useMemo(
    () => balanceGalleryColumns(images.map((_, index) => states[index]?.ratio ?? defaultGalleryRatio), columnCount),
    [columnCount, images, states],
  );

  if (!images.length) return <p className="work-detail-empty">{t("detail.galleryEmpty")}</p>;

  return (
    <div className={`work-gallery-grid columns-${columnCount}`} ref={rootRef}>
      {columns.map((indices, columnIndex) => (
        <div className="work-gallery-column" key={`column:${columnIndex}`}>
          {indices.map((index) => {
            const chain = images[index] ?? [];
            const state = states[index] ?? createTileState(chain);
            return (
              <GalleryTile
                chain={chain}
                index={index}
                state={state}
                onOpen={onOpen}
                onLoad={(ratio) => setStates((current) => updateTileState(current, index, {
                  ratio,
                  loaded: true,
                  failed: false,
                }))}
                onError={() => setStates((current) => advanceTileSource(current, images, index))}
                openLabel={t("detail.galleryOpenImage", { number: index + 1 })}
                unavailableLabel={t("detail.galleryImageUnavailable")}
                key={`${index}:${chain.join("|")}`}
              />
            );
          })}
        </div>
      ))}
    </div>
  );
}

function GalleryTile({
  chain,
  index,
  state,
  onOpen,
  onLoad,
  onError,
  openLabel,
  unavailableLabel,
}: {
  chain: string[];
  index: number;
  state: GalleryTileState;
  onOpen: (index: number) => void;
  onLoad: (ratio: number) => void;
  onError: () => void;
  openLabel: string;
  unavailableLabel: string;
}) {
  const source = chain[state.sourceIndex];
  const ready = state.loaded || state.failed;

  return (
    <button
      type="button"
      className={`work-gallery-tile cover-hover-trigger cover-hover-frame ${ready ? "is-loaded" : ""}`}
      style={{ animationDelay: `${(index % 5) * 0.06}s` }}
      onClick={() => onOpen(index)}
      aria-label={openLabel}
    >
      <span className="work-gallery-frame">
        <span className="work-gallery-image cover-hover-media">
          {state.failed || !source ? (
            <ArchiveImagePlaceholder className="work-gallery-placeholder" label={unavailableLabel} />
          ) : (
            <img
              src={source}
              alt=""
              loading={index < 4 ? "eager" : "lazy"}
              decoding="async"
              onLoad={(event) => {
                const image = event.currentTarget;
                onLoad(image.naturalWidth && image.naturalHeight
                  ? image.naturalWidth / image.naturalHeight
                  : defaultGalleryRatio);
              }}
              onError={onError}
            />
          )}
        </span>
      </span>
    </button>
  );
}

function createTileStates(images: string[][]): GalleryTileState[] {
  return images.map(createTileState);
}

function createTileState(chain: string[]): GalleryTileState {
  return {
    sourceIndex: 0,
    ratio: defaultGalleryRatio,
    loaded: false,
    failed: chain.length === 0,
  };
}

function updateTileState(
  states: GalleryTileState[],
  index: number,
  updates: Partial<GalleryTileState>,
): GalleryTileState[] {
  return states.map((state, stateIndex) => stateIndex === index ? { ...state, ...updates } : state);
}

function advanceTileSource(states: GalleryTileState[], images: string[][], index: number): GalleryTileState[] {
  const current = states[index];
  const chain = images[index] ?? [];
  if (!current) return states;
  const nextSource = current.sourceIndex + 1;
  return updateTileState(states, index, nextSource < chain.length
    ? { sourceIndex: nextSource, ratio: defaultGalleryRatio, loaded: false, failed: false }
    : { ratio: defaultGalleryRatio, loaded: false, failed: true });
}

export function ImageGalleryModal({
  images,
  openIndex,
  title,
  onClose,
}: {
  images: string[][];
  openIndex: number | null;
  title: string;
  onClose: () => void;
}) {
  const { t } = usePresentation();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const railRef = useRef<HTMLDivElement>(null);
  const thumbRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const dragStart = useRef<number | null>(null);
  const dragMoved = useRef(false);
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
  const [viewport, setViewport] = useState(() => ({
    width: typeof window === "undefined" ? 1280 : window.innerWidth,
    height: typeof window === "undefined" ? 800 : window.innerHeight,
  }));

  useEffect(() => {
    if (openIndex === null) return;
    setCurrent(Math.max(0, Math.min(openIndex, total - 1)));
    const dialog = dialogRef.current;
    if (dialog && !dialog.open) {
      dialog.showModal();
      dialog.focus();
    }
  }, [openIndex, total]);

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

  useEffect(() => {
    if (!open) return;
    const rail = railRef.current;
    const thumb = thumbRefs.current[current];
    if (!rail || !thumb) return;
    rail.scrollTo({
      left: thumb.offsetLeft - rail.clientWidth / 2 + thumb.clientWidth / 2,
      behavior: reduceMotion ? "auto" : "smooth",
    });
  }, [current, open, reduceMotion]);

  const goPrevious = () => setCurrent((value) => multiple ? (value - 1 + total) % total : value);
  const goNext = () => setCurrent((value) => multiple ? (value + 1) % total : value);

  useEffect(() => {
    if (!open || !multiple) return;
    const stage = stageRef.current;
    if (!stage) return;
    let lastTurn = 0;
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const now = performance.now();
      if (now - lastTurn < 320) return;
      lastTurn = now;
      setCurrent((value) => event.deltaY > 0 || event.deltaX > 0
        ? (value + 1) % total
        : (value - 1 + total) % total);
    };
    stage.addEventListener("wheel", wheel, { passive: false });
    return () => stage.removeEventListener("wheel", wheel);
  }, [multiple, open, total]);

  if (!total) return null;

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

  const centerWidth = sizeFor(ratios[current] ?? defaultModalRatio).width;
  const backdropChain = images[current] ?? [];

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
      onKeyDown={(event) => {
        if (event.key === "ArrowLeft") goPrevious();
        if (event.key === "ArrowRight") goNext();
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
              <button type="button" onClick={onClose} aria-label={t("detail.galleryClose")}><X aria-hidden="true" /></button>
            </header>
            <div
              ref={stageRef}
              className={`gallery-fan-stage ${dragging ? "is-dragging" : ""}`}
              onPointerDown={(event) => {
                if (!multiple) return;
                dragStart.current = event.clientX;
                dragMoved.current = false;
                setDragging(true);
                stageRef.current?.setPointerCapture(event.pointerId);
              }}
              onPointerMove={(event) => {
                if (dragStart.current === null) return;
                const distance = event.clientX - dragStart.current;
                if (Math.abs(distance) > 6) dragMoved.current = true;
                setDragOffset(-distance / centerWidth);
              }}
              onPointerUp={(event) => {
                if (dragStart.current === null) return;
                const distance = event.clientX - dragStart.current;
                dragStart.current = null;
                setDragging(false);
                setDragOffset(0);
                if (distance < -60) goNext();
                else if (distance > 60) goPrevious();
                window.setTimeout(() => { dragMoved.current = false; }, 50);
              }}
              onPointerCancel={() => {
                dragStart.current = null;
                setDragging(false);
                setDragOffset(0);
              }}
            >
              {images.map((chain, index) => {
                const distance = index - current - dragOffset;
                const clamped = Math.max(-(visibleCards + 1), Math.min(visibleCards + 1, distance));
                const hidden = Math.abs(distance) > visibleCards + 0.9;
                const absoluteDistance = Math.abs(clamped);
                const center = Math.round(distance) === 0;
                const size = sizeFor(ratios[index] ?? defaultModalRatio);
                const delay = !reduceMotion && dealt && !dealSettled ? `${Math.min(absoluteDistance, 3) * 0.06}s` : "0s";
                const dimOpacity = Math.min(absoluteDistance * 0.28, 0.6);
                return (
                  <div
                    className="gallery-fan-card"
                    key={`${chain[0]}:${index}`}
                    aria-hidden={!center || undefined}
                    onClick={() => {
                      if (!center && !dragMoved.current) setCurrent(index);
                    }}
                    style={{
                      width: size.width,
                      height: size.height,
                      marginLeft: -size.width / 2,
                      marginTop: -size.height / 2,
                      transform: `rotate(${dealt ? clamped * fanStep : 0}deg)`,
                      transformOrigin: fanPivot,
                      zIndex: 40 - Math.round(absoluteDistance * 10),
                      opacity: hidden ? 0 : 1,
                      pointerEvents: hidden ? "none" : "auto",
                      transition: dragging || reduceMotion ? "opacity 0.4s ease" : `transform 0.55s ${fanEase}, opacity 0.4s ease`,
                      transitionDelay: delay,
                      cursor: center ? undefined : "pointer",
                    }}
                  >
                    <div style={{
                      transform: `scale(${1 - Math.min(absoluteDistance, 2.4) * 0.12})`,
                      transition: dragging || reduceMotion ? "none" : `transform 0.55s ${fanEase}`,
                      transitionDelay: delay,
                    }}>
                      <GalleryCardImage
                        chain={chain}
                        alt={center ? title : ""}
                        unavailableLabel={center ? t("detail.galleryNoImageFor", { title }) : ""}
                        onRatio={(ratio) => setRatios((values) => Math.abs((values[index] ?? 0) - ratio) < 0.001 ? values : { ...values, [index]: ratio })}
                      />
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
            </div>
            {multiple && (
              <>
                <button className="gallery-side-arrow previous" type="button" onClick={goPrevious} aria-label={t("detail.galleryPrevious")}><ChevronLeft /></button>
                <button className="gallery-side-arrow next" type="button" onClick={goNext} aria-label={t("detail.galleryNext")}><ChevronRight /></button>
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
              </>
            )}
            <span className="screen-reader-only" aria-live="polite">
              {t("detail.galleryPosition", { current: current + 1, total })}
            </span>
          </motion.div>
        )}
      </AnimatePresence>
    </dialog>
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
}: {
  chain: string[];
  alt: string;
  unavailableLabel: string;
  onRatio?: (ratio: number) => void;
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
