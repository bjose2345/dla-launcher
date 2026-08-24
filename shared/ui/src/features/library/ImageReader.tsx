import {
  AlertTriangle,
  ArrowLeft,
  ArrowLeftRight,
  BookOpen,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  GalleryHorizontal,
  Image as ImageIcon,
  Images,
  LoaderCircle,
  Maximize2,
  Move,

  RefreshCw,
  Rows3,
  Scan,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import {
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type ReactElement,
  type ReactNode,
  type RefObject,
  type WheelEvent as ReactWheelEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { useBoundKeys } from "../../preferences/KeyBindingsProvider";
import {
  adjacentReaderItems,
  clampImageReaderZoom,
  imageReaderChapters,
  imageReaderProfilePreferences,
  readImageReaderPreferences,
  isWidePage,
  readerClickStep,
  spreadDisplayOrder,
  spreadPageOrdinals,
  spreadStep,
  readerHorizontalStep,
  writeImageReaderPreferences,
  type ImageReaderDirection,
  type ImageReaderFit,
  type ImageReaderLayout,
  type ImageReaderPreferences,
  type ImageReaderProfile,
} from "./imageReaderModel";
import { mediaItemName } from "./mediaSession";
import type { LibraryGateway, MediaSession, MediaSessionItem } from "./types";

const CONTROLS_HIDE_DELAY_MS = 2_400;
const WHEEL_NAVIGATION_DELAY_MS = 320;
const ZOOM_STEP = 0.15;

interface ImageReaderProps {
  gateway: Pick<LibraryGateway, "mediaAssetUrl">;
  session: MediaSession;
  installationName: string;
  items: MediaSessionItem[];
  currentOrdinal: number;
  completed: boolean;
  saveError: string;
  closing: boolean;
  onChoose: (ordinal: number) => void;
  onComplete: () => void;
  onBack: () => void;
}

interface Point {
  x: number;
  y: number;
}

interface DragState {
  pointerId: number;
  start: Point;
  origin: Point;
}

export function ImageReader({
  gateway,
  session,
  installationName,
  items,
  currentOrdinal,
  completed,
  saveError,
  closing,
  onChoose,
  onComplete,
  onBack,
}: ImageReaderProps) {
  const { t } = usePresentation();
  const [preferences, setPreferences] = useState<ImageReaderPreferences>(() => (
    readImageReaderPreferences(session.installationId)
  ));
  const [controlsVisible, setControlsVisible] = useState(true);
  const [pan, setPan] = useState<Point>({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const [retryVersion, setRetryVersion] = useState<Record<number, number>>({});
  const hideTimerRef = useRef<number | null>(null);
  const wheelAtRef = useRef(0);
  const dragRef = useRef<DragState | null>(null);
  const singleStageRef = useRef<HTMLDivElement>(null);
  const continuousStageRef = useRef<HTMLDivElement>(null);
  const chapters = useMemo(() => imageReaderChapters(items), [items]);
  const currentIndex = Math.max(0, items.findIndex((item) => item.ordinal === currentOrdinal));
  const currentItem = items[currentIndex];
  const widePathsRef = useRef<Set<string>>(new Set());
  const [wideVersion, setWideVersion] = useState(0);
  const markWidePage = useCallback((relativePath: string, width: number, height: number) => {
    if (!isWidePage(width, height) || widePathsRef.current.has(relativePath)) return;
    widePathsRef.current.add(relativePath);
    setWideVersion((version) => version + 1);
  }, []);
  const visiblePages = useMemo(() => {
    if (preferences.layout !== "spread" || !currentItem) return currentItem ? [currentItem] : [];
    const ordinals = spreadDisplayOrder(
      spreadPageOrdinals(items, currentOrdinal, widePathsRef.current),
      preferences.direction,
    );
    return ordinals.flatMap((ordinal) => {
      const found = items.find((item) => item.ordinal === ordinal);
      return found ? [found] : [];
    });
  }, [currentItem, currentOrdinal, items, preferences.direction, preferences.layout, wideVersion]);
  const canPan = preferences.zoom > 1 || preferences.fit !== "height";

  const clearHideTimer = useCallback(() => {
    if (hideTimerRef.current !== null) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
  }, []);

  const revealControls = useCallback(() => {
    setControlsVisible(true);
    clearHideTimer();
    hideTimerRef.current = window.setTimeout(() => {
      hideTimerRef.current = null;
      if (readerControlsAreFocused()) return;
      setControlsVisible(false);
    }, CONTROLS_HIDE_DELAY_MS);
  }, [clearHideTimer]);

  useEffect(() => {
    writeImageReaderPreferences(session.installationId, preferences);
  }, [preferences, session.installationId]);

  useEffect(() => {
    revealControls();
    return clearHideTimer;
  }, [clearHideTimer, revealControls]);

  useEffect(() => {
    setPan({ x: 0, y: 0 });
    dragRef.current = null;
    setDragging(false);
  }, [currentOrdinal, preferences.fit, preferences.layout]);

  useEffect(() => {
    const preloaders = adjacentReaderItems(items, currentOrdinal).map((item) => {
      const image = new window.Image();
      image.decoding = "async";
      image.src = gateway.mediaAssetUrl(session.id, item.ordinal);
      return image;
    });
    return () => preloaders.forEach((image) => { image.src = ""; });
  }, [currentOrdinal, gateway, items, session.id]);

  const goByStep = useCallback((step: -1 | 1) => {
    if (preferences.layout === "spread") {
      const next = spreadStep(items, currentOrdinal, step, widePathsRef.current);
      if (next !== currentOrdinal) onChoose(next);
      return;
    }
    const index = items.findIndex((item) => item.ordinal === currentOrdinal);
    const next = items[index + step];
    if (next) onChoose(next.ordinal);
  }, [currentOrdinal, items, onChoose, preferences.layout]);

  const changeZoom = useCallback((change: number) => {
    setPreferences((current) => ({
      ...current,
      zoom: clampImageReaderZoom(current.zoom + change),
    }));
  }, []);

  const resetView = useCallback(() => {
    setPreferences((current) => ({ ...current, zoom: 1 }));
    setPan({ x: 0, y: 0 });
  }, []);

  const scrollContinuous = useCallback((direction: -1 | 1) => {
    const root = continuousStageRef.current;
    if (!root) return;
    root.scrollBy({
      top: direction * Math.max(160, root.clientHeight * 0.8),
      behavior: readerScrollBehavior(),
    });
  }, []);

  const readerHandlers = useMemo(() => ({
    readerNextPage: () => {
      revealControls();
      goByStep(readerHorizontalStep("ArrowRight", preferences.direction));
    },
    readerPreviousPage: () => {
      revealControls();
      goByStep(readerHorizontalStep("ArrowLeft", preferences.direction));
    },
    readerScrollBack: () => {
      revealControls();
      if (preferences.layout === "continuous") scrollContinuous(-1);
      else goByStep(-1);
    },
    readerScrollForward: () => {
      revealControls();
      if (preferences.layout === "continuous") scrollContinuous(1);
      else goByStep(1);
    },
    readerZoomIn: () => { revealControls(); changeZoom(ZOOM_STEP); },
    readerZoomOut: () => { revealControls(); changeZoom(-ZOOM_STEP); },
    readerResetZoom: () => { revealControls(); resetView(); },
  }), [
    changeZoom, goByStep, preferences.direction, preferences.layout, resetView, revealControls,
    scrollContinuous,
  ]);
  useBoundKeys("reader", readerHandlers);

  const clampPan = useCallback((next: Point): Point => {
    const stage = singleStageRef.current;
    const image = stage?.querySelector<HTMLImageElement>(".image-reader-asset img");
    if (!stage || !image) return next;
    const maxX = Math.max(0, (image.offsetWidth * preferences.zoom - stage.clientWidth) / 2) + 32;
    const maxY = Math.max(0, (image.offsetHeight * preferences.zoom - stage.clientHeight) / 2) + 32;
    return {
      x: Math.max(-maxX, Math.min(maxX, next.x)),
      y: Math.max(-maxY, Math.min(maxY, next.y)),
    };
  }, [preferences.zoom]);

  const beginPan = (event: ReactPointerEvent<HTMLDivElement>) => {
    revealControls();
    if (!canPan || event.button !== 0 || isInteractiveTarget(event.target)) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      start: { x: event.clientX, y: event.clientY },
      origin: pan,
    };
    setDragging(true);
  };

  const movePan = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    setPan(clampPan({
      x: drag.origin.x + event.clientX - drag.start.x,
      y: drag.origin.y + event.clientY - drag.start.y,
    }));
  };

  const endPan = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    dragRef.current = null;
    setDragging(false);
  };

  const handleWheel = (event: ReactWheelEvent<HTMLDivElement>) => {
    revealControls();
    if (preferences.layout !== "page") return;
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      changeZoom(event.deltaY < 0 ? ZOOM_STEP : -ZOOM_STEP);
      return;
    }
    if (canPan) {
      event.preventDefault();
      setPan((current) => clampPan({
        x: current.x - event.deltaX,
        y: current.y - event.deltaY,
      }));
      return;
    }
    const delta = Math.abs(event.deltaY) >= Math.abs(event.deltaX) ? event.deltaY : event.deltaX;
    if (Math.abs(delta) < 35 || Date.now() - wheelAtRef.current < WHEEL_NAVIGATION_DELAY_MS) return;
    event.preventDefault();
    wheelAtRef.current = Date.now();
    goByStep(delta > 0 ? 1 : -1);
  };

  const updateProfile = (profile: ImageReaderProfile) => {
    setPreferences(imageReaderProfilePreferences(profile));
    setPan({ x: 0, y: 0 });
  };
  const updateLayout = (layout: ImageReaderLayout) => {
    setPreferences((current) => ({ ...current, layout }));
  };
  const updateDirection = (direction: ImageReaderDirection) => {
    setPreferences((current) => ({ ...current, direction }));
  };
  const updateFit = (fit: ImageReaderFit) => {
    setPreferences((current) => ({ ...current, fit, zoom: 1 }));
    setPan({ x: 0, y: 0 });
  };
  const retry = (ordinal: number) => {
    setRetryVersion((current) => ({ ...current, [ordinal]: (current[ordinal] ?? 0) + 1 }));
  };

  return (
    <main
      className={`image-reader-page image-reader-${preferences.profile} image-reader-${preferences.layout}${controlsVisible ? " is-controls-visible" : " is-controls-hidden"}`}
      onMouseMove={revealControls}
      onFocusCapture={revealControls}
      onBlurCapture={revealControls}
      onWheel={handleWheel}
    >
      <header className="image-reader-topbar">
        <button className="image-reader-icon-button" type="button" disabled={closing} onClick={onBack}>
          {closing ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <ArrowLeft aria-hidden="true" />}
          <span>{t("library.back")}</span>
        </button>
        <div className="image-reader-title">
          <span>{preferences.profile === "manga" ? <BookOpen aria-hidden="true" /> : <Images aria-hidden="true" />}{t("media.player.images")}</span>
          <strong>{installationName}</strong>
          <small>{currentItem ? mediaItemName(currentItem) : t("media.noItems")}</small>
        </div>
        <div className="image-reader-page-status" aria-live="polite">
          <strong>{items.length > 0 ? currentIndex + 1 : 0}</strong>
          <span>/ {items.length}</span>
        </div>
        <button className="image-reader-finish" type="button" disabled={completed} onClick={onComplete}>
          <CheckCircle2 aria-hidden="true" /><span>{t(completed ? "media.completed" : "media.markFinished")}</span>
        </button>
      </header>

      {saveError ? (
        <div className="image-reader-save-error" role="alert">
          <AlertTriangle aria-hidden="true" />
          {t("common.requestFailed", { error: saveError })}
        </div>
      ) : null}

      {preferences.layout === "page" ? (
        <div
          className={`image-reader-single${dragging ? " is-dragging" : ""}${canPan ? " can-pan" : ""}`}
          ref={singleStageRef}
          onPointerDown={beginPan}
          onPointerMove={movePan}
          onPointerUp={endPan}
          onPointerCancel={endPan}
        >
          {visiblePages.length ? visiblePages.map((page) => (
            <ReaderAsset
              className={`is-fit-${preferences.fit}`}
              key={page.ordinal}
              source={gateway.mediaAssetUrl(session.id, page.ordinal)}
              item={page}
              retryVersion={retryVersion[page.ordinal] ?? 0}
              style={{
                "--reader-pan-x": `${pan.x}px`,
                "--reader-pan-y": `${pan.y}px`,
                "--reader-zoom": preferences.zoom,
              } as CSSProperties}
              onNaturalSize={(width, height) => markWidePage(page.relativePath, width, height)}
              onRetry={() => retry(page.ordinal)}
            />
          )) : <strong className="image-reader-empty">{t("media.noItems")}</strong>}
          <button
            className="image-reader-click-zone is-left"
            type="button"
            disabled={!items[currentIndex + readerClickStep("left", preferences.direction)]}
            aria-label={t(readerClickStep("left", preferences.direction) > 0 ? "media.next" : "media.previous")}
            onClick={() => goByStep(readerClickStep("left", preferences.direction))}
          >
            <ChevronLeft aria-hidden="true" />
          </button>
          <button
            className="image-reader-click-zone is-right"
            type="button"
            disabled={!items[currentIndex + readerClickStep("right", preferences.direction)]}
            aria-label={t(readerClickStep("right", preferences.direction) > 0 ? "media.next" : "media.previous")}
            onClick={() => goByStep(readerClickStep("right", preferences.direction))}
          >
            <ChevronRight aria-hidden="true" />
          </button>
        </div>
      ) : (
        <ContinuousReader
          gateway={gateway}
          sessionId={session.id}
          items={items}
          chapters={chapters}
          currentOrdinal={currentOrdinal}
          fit={preferences.fit}
          zoom={preferences.zoom}
          rootRef={continuousStageRef}
          retryVersion={retryVersion}
          onChoose={onChoose}
          onRetry={retry}
        />
      )}

      <footer className="image-reader-toolbar">
        <ReaderSegment label={t("media.reader.profile")}>
          <ReaderModeButton active={preferences.profile === "gallery"} label={t("media.reader.gallery")} onClick={() => updateProfile("gallery")}><GalleryHorizontal aria-hidden="true" /></ReaderModeButton>
          <ReaderModeButton active={preferences.profile === "manga"} label={t("media.reader.manga")} onClick={() => updateProfile("manga")}><BookOpen aria-hidden="true" /></ReaderModeButton>
        </ReaderSegment>
        <ReaderSegment label={t("media.reader.layout")}>
          <ReaderModeButton active={preferences.layout === "page"} label={t("media.reader.singlePage")} onClick={() => updateLayout("page")}><ImageIcon aria-hidden="true" /></ReaderModeButton>
          <ReaderModeButton active={preferences.layout === "continuous"} label={t("media.reader.continuous")} onClick={() => updateLayout("continuous")}><Rows3 aria-hidden="true" /></ReaderModeButton>
        </ReaderSegment>
        <ReaderSegment label={t("media.reader.fit")}>
          <ReaderModeButton active={preferences.fit === "width"} label={t("media.reader.fitWidth")} onClick={() => updateFit("width")}><Maximize2 aria-hidden="true" /></ReaderModeButton>
          <ReaderModeButton active={preferences.fit === "height"} label={t("media.reader.fitHeight")} onClick={() => updateFit("height")}><Scan aria-hidden="true" /></ReaderModeButton>
          <ReaderModeButton active={preferences.fit === "original"} label={t("media.reader.originalSize")} onClick={() => updateFit("original")}><Move aria-hidden="true" /></ReaderModeButton>
        </ReaderSegment>
        <ReaderSegment label={t("media.reader.zoom")} compact>
          <button type="button" aria-label={t("media.reader.zoomOut")} onClick={() => changeZoom(-ZOOM_STEP)}><ZoomOut aria-hidden="true" /></button>
          <output>{Math.round(preferences.zoom * 100)}%</output>
          <button type="button" aria-label={t("media.reader.resetView")} onClick={resetView}><RefreshCw aria-hidden="true" /></button>
          <button type="button" aria-label={t("media.reader.zoomIn")} onClick={() => changeZoom(ZOOM_STEP)}><ZoomIn aria-hidden="true" /></button>
        </ReaderSegment>
        <ReaderSegment label={t("media.reader.direction")}>
          <ReaderModeButton active={preferences.direction === "ltr"} label={t("media.reader.leftToRight")} onClick={() => updateDirection("ltr")}><ArrowLeftRight aria-hidden="true" /></ReaderModeButton>
          <ReaderModeButton active={preferences.direction === "rtl"} label={t("media.reader.rightToLeft")} onClick={() => updateDirection("rtl")}><ArrowLeftRight aria-hidden="true" /></ReaderModeButton>
        </ReaderSegment>
      </footer>

      <ReaderFilmstrip
        gateway={gateway}
        sessionId={session.id}
        items={items}
        chapters={chapters}
        currentOrdinals={visiblePages.map((page) => page.ordinal)}
        direction={preferences.direction}
        onChoose={onChoose}
      />
    </main>
  );
}

function ReaderSegment({
  label,
  compact = false,
  children,
}: {
  label: string;
  compact?: boolean;
  children: ReactNode;
}) {
  return (
    <div className={`image-reader-tool-group${compact ? " is-compact" : ""}`} role="group" aria-label={label}>
      <span>{label}</span>
      <div>{children}</div>
    </div>
  );
}

function ReaderModeButton({
  active,
  label,
  onClick,
  children,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
  children: ReactElement;
}) {
  return (
    <button className={active ? "is-active" : undefined} type="button" aria-pressed={active} onClick={onClick}>
      {children}<span>{label}</span>
    </button>
  );
}

function ReaderAsset({
  className,
  source,
  item,
  retryVersion,
  loading = "eager",
  style,
  onNaturalSize,
  onRetry,
}: {
  className: string;
  source: string;
  item: MediaSessionItem;
  retryVersion: number;
  loading?: "eager" | "lazy";
  style?: CSSProperties;
  onNaturalSize?: (width: number, height: number) => void;
  onRetry: () => void;
}) {
  const { t } = usePresentation();
  const [failed, setFailed] = useState(false);

  useEffect(() => setFailed(false), [retryVersion, source]);

  return (
    <div className={`image-reader-asset ${className}${failed ? " has-error" : ""}`} style={style}>
      {failed ? (
        <div className="image-reader-asset-error" role="alert">
          <AlertTriangle aria-hidden="true" />
          <strong>{t("media.assetUnavailable")}</strong>
          <span>{mediaItemName(item)}</span>
          <button type="button" onClick={onRetry}><RefreshCw aria-hidden="true" />{t("media.reader.retryPage")}</button>
        </div>
      ) : (
        <img
          key={retryVersion}
          src={source}
          alt={mediaItemName(item)}
          draggable={false}
          decoding="async"
          loading={loading}
          onLoad={(event) => onNaturalSize?.(
            event.currentTarget.naturalWidth,
            event.currentTarget.naturalHeight,
          )}
          onError={() => setFailed(true)}
        />
      )}
    </div>
  );
}

function ContinuousReader({
  gateway,
  sessionId,
  items,
  chapters,
  currentOrdinal,
  fit,
  zoom,
  rootRef,
  retryVersion,
  onChoose,
  onRetry,
}: {
  gateway: Pick<LibraryGateway, "mediaAssetUrl">;
  sessionId: string;
  items: MediaSessionItem[];
  chapters: ReturnType<typeof imageReaderChapters>;
  currentOrdinal: number;
  fit: ImageReaderFit;
  zoom: number;
  rootRef: RefObject<HTMLDivElement | null>;
  retryVersion: Record<number, number>;
  onChoose: (ordinal: number) => void;
  onRetry: (ordinal: number) => void;
}) {
  const { t } = usePresentation();
  const reportedOrdinalRef = useRef<number | null>(null);
  const programmaticOrdinalRef = useRef<number | null>(null);
  const initialScrollRef = useRef(true);
  const chapterStarts = useMemo(() => new Map(
    chapters.map((chapter) => [chapter.items[0]?.ordinal, chapter.path]),
  ), [chapters]);
  const currentIndex = useMemo(
    () => items.findIndex((item) => item.ordinal === currentOrdinal),
    [currentOrdinal, items],
  );

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const observer = new IntersectionObserver((entries) => {
      const programmaticOrdinal = programmaticOrdinalRef.current;
      if (programmaticOrdinal !== null) {
        const reachedTarget = entries.some((entry) => (
          entry.isIntersecting
          && Number((entry.target as HTMLElement).dataset.readerOrdinal) === programmaticOrdinal
        ));
        if (reachedTarget) programmaticOrdinalRef.current = null;
        return;
      }
      const visible = entries
        .filter((entry) => entry.isIntersecting)
        .sort((left, right) => right.intersectionRatio - left.intersectionRatio)[0];
      const ordinal = Number((visible?.target as HTMLElement | undefined)?.dataset.readerOrdinal);
      if (!Number.isInteger(ordinal) || ordinal === currentOrdinal) return;
      reportedOrdinalRef.current = ordinal;
      onChoose(ordinal);
    }, { root, rootMargin: "-18% 0px -42%", threshold: [0.15, 0.35, 0.6] });
    root.querySelectorAll<HTMLElement>("[data-reader-ordinal]").forEach((page) => observer.observe(page));
    return () => observer.disconnect();
  }, [currentOrdinal, items, onChoose]);

  useEffect(() => {
    if (reportedOrdinalRef.current === currentOrdinal) {
      reportedOrdinalRef.current = null;
      return;
    }
    programmaticOrdinalRef.current = currentOrdinal;
    const behavior = initialScrollRef.current ? "auto" : readerScrollBehavior();
    const frame = window.requestAnimationFrame(() => {
      rootRef.current
        ?.querySelector<HTMLElement>(`[data-reader-ordinal='${currentOrdinal}']`)
        ?.scrollIntoView({
          block: "start",
          behavior,
        });
      initialScrollRef.current = false;
    });
    const release = window.setTimeout(() => {
      if (programmaticOrdinalRef.current === currentOrdinal) {
        programmaticOrdinalRef.current = null;
      }
    }, behavior === "smooth" ? 700 : 100);
    return () => {
      window.cancelAnimationFrame(frame);
      window.clearTimeout(release);
    };
  }, [currentOrdinal]);

  return (
    <div className={`image-reader-continuous is-fit-${fit}`} ref={rootRef}>
      {items.map((item, index) => {
        const chapter = chapterStarts.get(item.ordinal);
        return (
          <div className="image-reader-continuous-entry" data-reader-ordinal={item.ordinal} key={item.ordinal}>
            {chapter !== undefined ? (
              <div className="image-reader-chapter-divider">
                <BookOpen aria-hidden="true" />
                <span>{chapter || t("media.reader.rootChapter")}</span>
              </div>
            ) : null}
            <ReaderAsset
              className={`is-fit-${fit}`}
              source={gateway.mediaAssetUrl(sessionId, item.ordinal)}
              item={item}
              retryVersion={retryVersion[item.ordinal] ?? 0}
              loading={Math.abs(index - currentIndex) <= 2 ? "eager" : "lazy"}
              style={continuousAssetStyle(fit, zoom)}
              onRetry={() => onRetry(item.ordinal)}
            />
            <span className="image-reader-continuous-number">{index + 1}</span>
          </div>
        );
      })}
    </div>
  );
}

function ReaderFilmstrip({
  gateway,
  sessionId,
  items,
  chapters,
  currentOrdinals,
  direction,
  onChoose,
}: {
  gateway: Pick<LibraryGateway, "mediaAssetUrl">;
  sessionId: string;
  items: MediaSessionItem[];
  chapters: ReturnType<typeof imageReaderChapters>;
  currentOrdinals: number[];
  direction: ImageReaderDirection;
  onChoose: (ordinal: number) => void;
}) {
  const { t } = usePresentation();
  const stripRef = useRef<HTMLDivElement>(null);
  const pageNumbers = useMemo(() => new Map(
    chapters.flatMap((chapter) => chapter.items).map((item, index) => [item.ordinal, index + 1]),
  ), [chapters]);
  const anchor = currentOrdinals[0] ?? items[0]?.ordinal;
  const position = anchor === undefined ? 0 : (pageNumbers.get(anchor) ?? 1);
  const percent = items.length ? (position / items.length) * 100 : 0;

  useEffect(() => {
    stripRef.current
      ?.querySelector<HTMLElement>("[aria-current='page']")
      ?.scrollIntoView({ block: "nearest", inline: "center", behavior: readerScrollBehavior() });
  }, [anchor]);

  const seek = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!items.length) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    let ratio = (event.clientX - bounds.left) / bounds.width;
    if (direction === "rtl") ratio = 1 - ratio;
    const index = Math.min(items.length - 1, Math.max(0, Math.round(ratio * (items.length - 1))));
    const target = items[index];
    if (target) onChoose(target.ordinal);
  };

  return (
    <div className={`image-reader-filmstrip is-${direction}`}>
      <div className="image-reader-filmstrip-scrub">
        <span>{position} / {items.length}</span>
        <div
          className="image-reader-filmstrip-track"
          role="slider"
          tabIndex={0}
          aria-label={t("media.reader.pages")}
          aria-valuemin={1}
          aria-valuemax={items.length}
          aria-valuenow={position}
          onClick={seek}
        >
          <i style={{ width: `${percent}%` }} />
        </div>
        <span className="is-right">{Math.max(0, items.length - position)}</span>
      </div>
      <div className="image-reader-filmstrip-track-list" ref={stripRef}>
        {items.map((item) => {
          const number = pageNumbers.get(item.ordinal) ?? item.ordinal + 1;
          const active = currentOrdinals.includes(item.ordinal);
          return (
            <button
              className={active ? "is-active" : undefined}
              type="button"
              key={item.ordinal}
              aria-current={active ? "page" : undefined}
              aria-label={t("media.goToItem", { number })}
              title={mediaItemName(item)}
              onClick={() => onChoose(item.ordinal)}
            >
              <span><ImageIcon aria-hidden="true" /></span>
              <img
                src={gateway.mediaAssetUrl(sessionId, item.ordinal)}
                alt=""
                loading="lazy"
                decoding="async"
                onError={(event) => { event.currentTarget.hidden = true; }}
              />
              <b>{number}</b>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function continuousAssetStyle(fit: ImageReaderFit, zoom: number): CSSProperties {
  if (fit === "width") return { "--reader-continuous-width": `${zoom * 100}%` } as CSSProperties;
  if (fit === "height") return { "--reader-continuous-height": `${Math.round(88 * zoom)}dvh` } as CSSProperties;
  return { "--reader-zoom": zoom } as CSSProperties;
}

function isInteractiveTarget(target: EventTarget | null): boolean {
  return target instanceof Element && Boolean(target.closest("button, input, select, textarea, a"));
}

function readerScrollBehavior(): ScrollBehavior {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
}

function readerControlsAreFocused(): boolean {
  const active = document.activeElement;
  return active instanceof Element && Boolean(active.closest(
    ".image-reader-topbar, .image-reader-toolbar, .image-reader-drawer",
  ));
}
