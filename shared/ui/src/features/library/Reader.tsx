import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  Keyboard,
  LoaderCircle,
  Maximize,
  Minimize,
  Minus,
  Plus,
  RefreshCw,
  SlidersHorizontal,
  X,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type WheelEvent as ReactWheelEvent,
} from "react";

import { useBoundKeys } from "../../preferences/KeyBindingsProvider";
import type { MessageKey } from "../../preferences/preferences";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { ImageSetViewer } from "./imageViewerModel";
import { mediaItemName } from "./mediaSession";
import { openPdfDocument, type PdfDocument } from "./pdfDocument";
import {
  ReaderShortcuts,
  guardKeys,
  scrollBehavior,
  useElementSize,
  useEscapeClosesPanel,
  useFullscreen,
} from "./readerControls";
import { useReader, type ReaderPosition } from "./ReaderProvider";
import { ImagePage, PdfPage, PdfThumbnail, StripView } from "./ReaderPages";
import {
  clampReaderZoom,
  defaultReaderPreferences,
  horizontalStep,
  isWidePage,
  nearbyPages,
  nextReaderLayout,
  pageBox,
  readReaderPreferences,
  screenOrder,
  spreadPages,
  stepPage,
  writeReaderPreferences,
  type PageBox,
  type ReaderBackground,
  type ReaderDirection,
  type ReaderFit,
  type ReaderLayout,
  type ReaderPreferences,
} from "./readerModel";
import type { LibraryGateway } from "./types";

const WHEEL_TURN_DELAY_MS = 320;
const WHEEL_TURN_THRESHOLD = 35;
const ZOOM_STEP = 0.15;
const DRAG_THRESHOLD_PX = 6;
const EDGE_REVEAL_TOP_PX = 90;
const EDGE_REVEAL_BOTTOM_PX = 150;
const RESUME_NOTICE_MS = 7_000;
const READER_ACTIONS: ReadonlySet<string> = new Set([
  "readerNextPage",
  "readerPreviousPage",
  "readerScrollBack",
  "readerScrollForward",
  "readerFirstPage",
  "readerLastPage",
  "readerLayout",
  "readerFullscreen",
  "readerZoomIn",
  "readerZoomOut",
  "readerResetZoom",
  "readerClose",
]);

interface DragState {
  pointerId: number;
  x: number;
  y: number;
  left: number;
  top: number;
  moved: boolean;
}

export function Reader({
  gateway,
  onOpenIn,
}: {
  gateway: Pick<LibraryGateway, "mediaAssetUrl" | "mediaThumbnailUrl">;
  /** Offered for image sets, which open in the CG viewer unless moved here. */
  onOpenIn?: (viewer: ImageSetViewer, position: ReaderPosition) => void;
}) {
  const reader = useReader();
  const session = reader.session!;
  const { items, start } = reader;
  const { locale, t } = usePresentation();
  const preferEnglish = locale !== "ja-JP";
  const title = reader.title(preferEnglish);
  const creator = reader.creator(preferEnglish);
  const isPdf = session.action === "open_document";
  const rootRef = useRef<HTMLElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const draggedRef = useRef(false);
  const wheelAtRef = useRef(0);

  const [preferences, setPreferences] = useState<ReaderPreferences>(() => (
    readReaderPreferences(session.installationId, defaultReaderPreferences(reader.manga))
  ));
  const [position, setPosition] = useState<ReaderPosition>(start);
  const [chrome, setChrome] = useState(true);
  const [panel, setPanel] = useState<"settings" | "shortcuts" | null>(null);
  const [resumeNotice, setResumeNotice] = useState(() => (
    reader.resumed && (start.page > 0 || (isPdf && start.ordinal !== items[0]?.ordinal))
  ));
  const [endCard, setEndCard] = useState(false);
  const [sizes, setSizes] = useState<ReadonlyMap<number, PageBox>>(new Map());
  const stage = useElementSize(stageRef);
  const fullscreen = useFullscreen(rootRef);

  const itemIndex = Math.max(0, items.findIndex((item) => item.ordinal === position.ordinal));
  const pdfItem = isPdf ? items[itemIndex] : undefined;
  const pdfUrl = pdfItem ? gateway.mediaAssetUrl(session.id, pdfItem.ordinal) : null;
  const pdfState = usePdfDocument(pdfUrl);
  const pdf = pdfState.pdf;
  const count = isPdf ? (pdf?.pageCount ?? 0) : items.length;
  const page = count ? Math.min(position.page, count - 1) : 0;
  const { layout, direction, fit, zoom } = preferences;

  useEffect(() => {
    writeReaderPreferences(session.installationId, preferences);
  }, [preferences, session.installationId]);

  useEffect(() => {
    if (!resumeNotice) return;
    const timer = window.setTimeout(() => setResumeNotice(false), RESUME_NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [resumeNotice]);

  // A new PDF of a work with several brings its own page sizes.
  useEffect(() => setSizes(new Map()), [pdf]);

  const wide = useMemo(() => new Set(
    [...sizes].flatMap(([index, size]) => (isWidePage(size.width, size.height) ? [index] : [])),
  ), [sizes]);
  const shown = useMemo(() => {
    if (!count) return [];
    return layout === "double" ? spreadPages(page, count, wide) : [page];
  }, [count, layout, page, wide]);

  const rememberSize = useCallback((index: number, size: PageBox) => {
    setSizes((current) => {
      const known = current.get(index);
      if (known && known.width === size.width && known.height === size.height) return current;
      return new Map(current).set(index, size);
    });
  }, []);

  // PDF page sizes are read ahead of drawing, so the page box is right at once.
  useEffect(() => {
    if (!pdf) return;
    let current = true;
    const wanted = layout === "strip" ? [0, ...shown] : [...shown, page + 1, page + 2];
    for (const index of new Set(wanted)) {
      if (index < 0 || index >= pdf.pageCount || sizes.has(index)) continue;
      pdf.pageSize(index).then((size) => current && rememberSize(index, size), () => undefined);
    }
    return () => {
      current = false;
    };
  }, [layout, page, pdf, rememberSize, shown, sizes]);

  const pdfOrdinal = pdfItem?.ordinal ?? 0;
  const ordinalFor = useCallback((index: number) => (
    isPdf ? pdfOrdinal : (items[index]?.ordinal ?? 0)
  ), [isPdf, items, pdfOrdinal]);

  const lastItem = itemIndex === items.length - 1;
  const atEnd = count > 0 && lastItem && (layout === "strip" ? page === count - 1 : shown.includes(count - 1));

  useEffect(() => {
    if (!count) return;
    const current = { ordinal: ordinalFor(page), page };
    if (atEnd && !reader.finished) reader.finish(current, count);
    else reader.move(current, count);
    // Only a change of place is saved, not every render of the reader.
  }, [atEnd, count, ordinalFor, page]);

  const goTo = useCallback((index: number, turning = false) => {
    if (!count) return;
    const next = Math.max(0, Math.min(count - 1, index));
    setPosition({ ordinal: ordinalFor(next), page: next });
    setResumeNotice(false);
    setEndCard(false);
    if (turning && preferences.hideControls) {
      setChrome(false);
      setPanel(null);
    }
  }, [count, ordinalFor, preferences.hideControls]);

  const scrollStage = useCallback((step: -1 | 1) => {
    const root = stageRef.current?.querySelector<HTMLElement>(".reader-strip") ?? stageRef.current;
    root?.scrollBy({ top: step * Math.max(160, root.clientHeight * 0.85), behavior: scrollBehavior() });
  }, []);

  const turn = useCallback((step: -1 | 1) => {
    if (!count) return;
    if (layout === "strip") {
      scrollStage(step);
      return;
    }
    const next = stepPage(page, count, step, layout, wide);
    if (next !== page) {
      goTo(next, true);
      return;
    }
    const neighbour = items[itemIndex + step];
    if (isPdf && neighbour) {
      setPosition({ ordinal: neighbour.ordinal, page: 0 });
      return;
    }
    if (step === 1) {
      setEndCard(true);
      setChrome(true);
    }
  }, [count, goTo, isPdf, itemIndex, items, layout, page, scrollStage, wide]);

  const turnSide = useCallback((side: "left" | "right") => turn(horizontalStep(side, direction)), [direction, turn]);

  const update = useCallback(<K extends keyof ReaderPreferences>(key: K, value: ReaderPreferences[K]) => {
    setPreferences((current) => ({ ...current, [key]: value }));
  }, []);
  const changeZoom = useCallback((change: number) => {
    setPreferences((current) => ({ ...current, zoom: clampReaderZoom(current.zoom + change) }));
  }, []);

  // One listener for the reader's life that always acts on the latest page,
  // so a key pressed right after a render never reaches a stale handler.
  const keyActions = useRef({ turnSide, turn, goTo, count, changeZoom, update, layout, fullscreen });
  keyActions.current = { turnSide, turn, goTo, count, changeZoom, update, layout, fullscreen };
  const readerKeys = useMemo(() => guardKeys({
    readerNextPage: () => keyActions.current.turnSide("right"),
    readerPreviousPage: () => keyActions.current.turnSide("left"),
    readerScrollForward: () => keyActions.current.turn(1),
    readerScrollBack: () => keyActions.current.turn(-1),
    readerFirstPage: () => keyActions.current.goTo(0),
    readerLastPage: () => keyActions.current.goTo(keyActions.current.count - 1),
    readerZoomIn: () => keyActions.current.changeZoom(ZOOM_STEP),
    readerZoomOut: () => keyActions.current.changeZoom(-ZOOM_STEP),
    readerResetZoom: () => keyActions.current.update("zoom", 1),
    readerLayout: () => keyActions.current.update("layout", nextReaderLayout(keyActions.current.layout)),
    readerFullscreen: () => keyActions.current.fullscreen.toggle(),
  }), []);
  useBoundKeys("reader", readerKeys, { ignoreInteractive: false });

  const closePanel = useCallback(() => setPanel(null), []);
  useEscapeClosesPanel(panel !== null, closePanel);

  const onWheel = (event: ReactWheelEvent<HTMLElement>) => {
    if (layout === "strip") return;
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      changeZoom(event.deltaY < 0 ? ZOOM_STEP : -ZOOM_STEP);
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
    turn(delta > 0 ? 1 : -1);
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    draggedRef.current = false;
    const scroller = stageRef.current;
    if (layout === "strip" || event.button !== 0 || !scroller) return;
    if ((event.target as Element).closest(".reader-bar, .reader-panel, .reader-card")) return;
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
    if (event.clientY < EDGE_REVEAL_TOP_PX || event.clientY > window.innerHeight - EDGE_REVEAL_BOTTOM_PX) {
      setChrome(true);
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
  const sideLabel = (side: "left" | "right") => (
    t(horizontalStep(side, direction) > 0 ? "settings.action.nextPage" : "settings.action.previousPage")
  );
  const atStart = page === 0 && itemIndex === 0;

  const kindLabel = isPdf ? t("reader.kindPdf") : t("reader.kindImages");
  const positionLabel = !count
    ? ""
    : shown.length > 1
      ? t("reader.positionSpread", { first: shown[0]! + 1, last: shown.at(-1)! + 1, total: count })
      : t("reader.position", { page: page + 1, total: count });
  const pageCountLabel = count === 1 ? t("reader.pageCountOne") : t("reader.pageCount", { count });
  const thumbnails = count ? nearbyPages(page, count) : [];

  const boxFor = useCallback((index: number, together: number) => pageBox(
    sizes.get(index) ?? sizes.get(0) ?? { width: 0, height: 0 },
    stage,
    fit,
    together,
    zoom,
    isPdf,
  ), [fit, isPdf, sizes, stage, zoom]);

  const renderPage = useCallback((index: number, box: PageBox) => (
    isPdf
      ? pdf ? <PdfPage pdf={pdf} index={index} label={t("reader.thumbnail", { page: index + 1 })} box={box} /> : null
      : (
          <ImagePage
            source={gateway.mediaAssetUrl(session.id, items[index]!.ordinal)}
            label={mediaItemName(items[index]!)}
            box={box}
            onNaturalSize={(size) => rememberSize(index, size)}
          />
        )
  ), [gateway, isPdf, items, pdf, rememberSize, session.id, t]);

  const onStripPage = useCallback((index: number) => (
    setPosition({ ordinal: ordinalFor(index), page: index })
  ), [ordinalFor]);
  const together = layout === "double" ? 2 : 1;
  const ahead = layout === "strip" || !shown.length
    ? []
    : [shown.at(-1)! + 1, shown.at(-1)! + together, shown[0]! - 1, shown[0]! - together];

  usePreloadedImages(isPdf ? [] : items, page, gateway, session.id);
  usePrerenderedPages(pdf, ahead, (index) => boxFor(index, together));

  const stageReady = stage.width > 0 && stage.height > 0;
  const opening = isPdf && !pdf && !pdfState.error;

  return (
    <main
      className={`reader is-${layout}${chrome ? " has-chrome" : ""}`}
      data-background={preferences.background}
      ref={rootRef}
      tabIndex={-1}
      onWheel={onWheel}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    >
      <div className="reader-stage" ref={stageRef}>
        {pdfState.error ? (
          <div className="reader-card reader-message" role="alert">
            <AlertTriangle aria-hidden="true" />
            <strong>{t("reader.openFailed")}</strong>
            <span>{t("common.technicalDetail", { detail: pdfState.error })}</span>
            <button className="reader-chip-button is-primary" type="button" onClick={() => void reader.close()}>
              <ArrowLeft aria-hidden="true" />{t("library.back")}
            </button>
          </div>
        ) : opening ? (
          <div className="reader-message" role="status">
            <LoaderCircle className="library-spin" aria-hidden="true" />
            <span>{t("reader.opening")}</span>
          </div>
        ) : !count ? (
          <p className="reader-message">{t("media.noItems")}</p>
        ) : !stageReady ? null : layout === "strip" ? (
          <StripView
            count={count}
            current={page}
            pageBoxFor={(index) => boxFor(index, 1)}
            renderPage={renderPage}
            onCurrent={onStripPage}
          />
        ) : (
          <div className="reader-spread">
            {screenOrder(shown, direction).map((index) => (
              <div className="reader-spread-page" key={`${position.ordinal}:${index}`}>
                {renderPage(index, boxFor(index, shown.length))}
              </div>
            ))}
          </div>
        )}
      </div>

      {layout !== "strip" && count > 0 ? (
        <div className="reader-zones">
          <button type="button" tabIndex={-1} aria-label={sideLabel("left")} onMouseDown={keepFocus} onClick={zone(() => turnSide("left"))} />
          <button type="button" tabIndex={-1} aria-label={t("reader.toggleControls")} onMouseDown={keepFocus} onClick={zone(() => setChrome((value) => !value))} />
          <button type="button" tabIndex={-1} aria-label={sideLabel("right")} onMouseDown={keepFocus} onClick={zone(() => turnSide("right"))} />
        </div>
      ) : null}

      {chrome && layout !== "strip" && count > 0 ? (
        <>
          <button
            className="reader-side-arrow is-left"
            type="button"
            aria-label={sideLabel("left")}
            title={sideLabel("left")}
            disabled={atStart && horizontalStep("left", direction) < 0}
            onMouseDown={keepFocus}
            onClick={() => turnSide("left")}
          >
            <ChevronLeft aria-hidden="true" />
          </button>
          <button
            className="reader-side-arrow is-right"
            type="button"
            aria-label={sideLabel("right")}
            title={sideLabel("right")}
            disabled={atStart && horizontalStep("right", direction) < 0}
            onMouseDown={keepFocus}
            onClick={() => turnSide("right")}
          >
            <ChevronRight aria-hidden="true" />
          </button>
        </>
      ) : null}

      {chrome ? (
        <header className="reader-bar reader-top">
          <button
            className="reader-icon-button reader-back"
            type="button"
            aria-label={t("library.back")}
            title={t("library.back")}
            disabled={reader.closing}
            onClick={() => void reader.close()}
          >
            {reader.closing ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <ArrowLeft aria-hidden="true" />}
          </button>
          <div className="reader-title">
            <strong title={title}>{title}</strong>
            <span>
              {reader.workCode ? <code>{reader.workCode}</code> : null}
              {creator && creator !== reader.workCode ? <span>{creator}</span> : null}
              <span>{kindLabel}</span>
              {count ? <span>{pageCountLabel}</span> : null}
              {isPdf && items.length > 1 ? (
                <span>{t("reader.volume", { number: itemIndex + 1, total: items.length })}</span>
              ) : null}
            </span>
          </div>
          {reader.saveError ? (
            <span className="reader-save-error" role="alert" title={t("common.technicalDetail", { detail: reader.saveError })}>
              <AlertTriangle aria-hidden="true" />{t("reader.saveFailed")}
            </span>
          ) : null}
          {positionLabel ? <span className="reader-position" aria-live="polite">{positionLabel}</span> : null}
          <button
            className={`reader-icon-button${panel === "shortcuts" ? " is-active" : ""}`}
            type="button"
            aria-label={t("reader.shortcuts")}
            aria-expanded={panel === "shortcuts"}
            title={t("reader.shortcuts")}
            onClick={() => setPanel((current) => (current === "shortcuts" ? null : "shortcuts"))}
          >
            <Keyboard aria-hidden="true" />
          </button>
          {fullscreen.supported ? (
            <button
              className="reader-icon-button"
              type="button"
              aria-label={t(fullscreen.active ? "media.video.exitFullscreen" : "settings.action.fullscreen")}
              title={t(fullscreen.active ? "media.video.exitFullscreen" : "settings.action.fullscreen")}
              onClick={fullscreen.toggle}
            >
              {fullscreen.active ? <Minimize aria-hidden="true" /> : <Maximize aria-hidden="true" />}
            </button>
          ) : null}
          <button
            className={`reader-chip-button${panel === "settings" ? " is-primary" : ""}`}
            type="button"
            aria-expanded={panel === "settings"}
            onClick={() => setPanel((current) => (current === "settings" ? null : "settings"))}
          >
            <SlidersHorizontal aria-hidden="true" />{t("reader.settings")}
          </button>
        </header>
      ) : null}

      {chrome && count > 1 ? (
        <footer className="reader-bar reader-bottom">
          <div className={`reader-thumbnails is-${direction}`}>
            {thumbnails.map((index) => (
              <button
                className={`reader-thumbnail${shown.includes(index) ? " is-current" : ""}`}
                type="button"
                key={index}
                aria-label={t("reader.thumbnail", { page: index + 1 })}
                aria-current={shown.includes(index) ? "page" : undefined}
                onClick={() => goTo(index)}
              >
                {isPdf ? (
                  pdf ? <PdfThumbnail pdf={pdf} index={index} /> : null
                ) : (
                  <img
                    className="reader-thumbnail-art"
                    src={gateway.mediaThumbnailUrl(session.id, items[index]!.ordinal)}
                    alt=""
                    loading="lazy"
                    decoding="async"
                  />
                )}
                <b>{index + 1}</b>
              </button>
            ))}
          </div>
          <div className="reader-slider" dir={direction}>
            <span>1</span>
            <input
              type="range"
              min={1}
              max={count}
              value={page + 1}
              aria-label={t("media.reader.pages")}
              aria-valuetext={positionLabel}
              onChange={(event) => goTo(Number(event.currentTarget.value) - 1)}
            />
            <span>{count}</span>
          </div>
        </footer>
      ) : null}

      {panel === "settings" ? (
        <ReaderSettings
          preferences={preferences}
          onChange={update}
          onZoom={changeZoom}
          onOpenInViewer={onOpenIn && !isPdf ? () => onOpenIn("viewer", { ordinal: ordinalFor(page), page }) : undefined}
        />
      ) : null}
      {panel === "shortcuts" ? <ReaderShortcuts actions={READER_ACTIONS} /> : null}

      {resumeNotice && !endCard ? (
        <div className="reader-toast" role="status">
          <span>{t("reader.resumed", { page: page + 1 })}</span>
          <button className="reader-chip-button is-outline" type="button" onClick={() => goTo(0)}>{t("reader.startOver")}</button>
          <button className="reader-icon-button is-plain" type="button" aria-label={t("notice.close")} onClick={() => setResumeNotice(false)}>
            <X aria-hidden="true" />
          </button>
        </div>
      ) : null}

      {endCard ? (
        <div className="reader-veil">
          <section className="reader-card reader-finished" aria-labelledby="reader-finished-title">
            <CheckCircle2 aria-hidden="true" />
            <h2 id="reader-finished-title">{t("reader.finishedTitle")}</h2>
            <p>{t("reader.finishedHelp")}</p>
            <div>
              <button className="reader-chip-button is-primary" type="button" onClick={() => void reader.close()}>
                {t("library.back")}
              </button>
              <button
                className="reader-chip-button"
                type="button"
                onClick={() => {
                  setEndCard(false);
                  if (isPdf && items[0] && items[0].ordinal !== position.ordinal) setPosition({ ordinal: items[0].ordinal, page: 0 });
                  else goTo(0);
                }}
              >
                {t("reader.readAgain")}
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </main>
  );
}

const LAYOUT_OPTIONS: ReadonlyArray<readonly [ReaderLayout, MessageKey]> = [
  ["single", "media.reader.singlePage"],
  ["double", "reader.layoutDouble"],
  ["strip", "reader.layoutStrip"],
];
const DIRECTION_OPTIONS: ReadonlyArray<readonly [ReaderDirection, MessageKey]> = [
  ["rtl", "media.reader.rightToLeft"],
  ["ltr", "media.reader.leftToRight"],
];
const FIT_OPTIONS: ReadonlyArray<readonly [ReaderFit, MessageKey]> = [
  ["height", "media.reader.fitHeight"],
  ["width", "media.reader.fitWidth"],
  ["original", "media.reader.originalSize"],
];
const BACKGROUND_OPTIONS: ReadonlyArray<readonly [ReaderBackground, MessageKey]> = [
  ["black", "reader.backgroundBlack"],
  ["gray", "reader.backgroundGray"],
  ["paper", "reader.backgroundPaper"],
];

function ReaderSettings({
  preferences,
  onChange,
  onZoom,
  onOpenInViewer,
}: {
  preferences: ReaderPreferences;
  onChange: <K extends keyof ReaderPreferences>(key: K, value: ReaderPreferences[K]) => void;
  onZoom: (change: number) => void;
  onOpenInViewer?: () => void;
}) {
  const { t } = usePresentation();
  const group = <K extends "layout" | "direction" | "fit" | "background">(
    key: K,
    title: MessageKey,
    options: ReadonlyArray<readonly [ReaderPreferences[K], MessageKey]>,
  ) => (
    <div className="reader-setting" role="group" aria-label={t(title)}>
      <span>{t(title)}</span>
      <div>
        {options.map(([value, label]) => (
          <button
            className={`reader-choice${preferences[key] === value ? " is-active" : ""}`}
            type="button"
            key={value}
            aria-pressed={preferences[key] === value}
            onClick={() => onChange(key, value)}
          >
            {t(label)}
          </button>
        ))}
      </div>
    </div>
  );
  return (
    <section className="reader-panel reader-settings" aria-label={t("reader.settingsTitle")}>
      {group("layout", "reader.layout", LAYOUT_OPTIONS)}
      {group("direction", "media.reader.direction", DIRECTION_OPTIONS)}
      {group("fit", "media.reader.fit", FIT_OPTIONS)}
      {preferences.layout !== "strip" ? (
        <div className="reader-setting" role="group" aria-label={t("media.reader.zoom")}>
          <span>{t("media.reader.zoom")}</span>
          <div className="reader-zoom">
            <button className="reader-choice" type="button" aria-label={t("media.reader.zoomOut")} onClick={() => onZoom(-ZOOM_STEP)}><Minus aria-hidden="true" /></button>
            <output>{Math.round(preferences.zoom * 100)}%</output>
            <button className="reader-choice" type="button" aria-label={t("media.reader.zoomIn")} onClick={() => onZoom(ZOOM_STEP)}><Plus aria-hidden="true" /></button>
            <button className="reader-choice" type="button" aria-label={t("media.reader.resetView")} onClick={() => onChange("zoom", 1)}><RefreshCw aria-hidden="true" /></button>
          </div>
        </div>
      ) : null}
      {group("background", "reader.background", BACKGROUND_OPTIONS)}
      <label className="reader-check">
        <input
          type="checkbox"
          checked={preferences.hideControls}
          onChange={(event) => onChange("hideControls", event.currentTarget.checked)}
        />
        {t("reader.hideControls")}
      </label>
      {onOpenInViewer ? (
        <div className="reader-setting" role="group" aria-label={t("viewer.openIn")}>
          <span>{t("viewer.openIn")}</span>
          <div>
            <button className="reader-choice" type="button" aria-pressed={false} onClick={onOpenInViewer}>
              {t("viewer.openInViewer")}
            </button>
            <button className="reader-choice is-active" type="button" aria-pressed>
              {t("viewer.openInReader")}
            </button>
          </div>
        </div>
      ) : null}
    </section>
  );
}

function usePdfDocument(url: string | null): { pdf: PdfDocument | null; error: string } {
  const [state, setState] = useState<{ url: string | null; pdf: PdfDocument | null; error: string }>({
    url: null,
    pdf: null,
    error: "",
  });
  useEffect(() => {
    if (!url) return;
    let current = true;
    let opened: PdfDocument | null = null;
    setState({ url, pdf: null, error: "" });
    openPdfDocument(url).then(
      (pdf) => {
        if (!current) {
          pdf.destroy();
          return;
        }
        opened = pdf;
        setState({ url, pdf, error: "" });
      },
      (cause: unknown) => {
        if (current) setState({ url, pdf: null, error: cause instanceof Error ? cause.message : String(cause) });
      },
    );
    return () => {
      current = false;
      opened?.destroy();
    };
  }, [url]);
  return state.url === url ? state : { pdf: null, error: "" };
}

function usePreloadedImages(
  items: { ordinal: number }[],
  page: number,
  gateway: Pick<LibraryGateway, "mediaAssetUrl">,
  sessionId: string,
) {
  useEffect(() => {
    const preloaders = [page - 1, page + 1, page + 2].flatMap((index) => {
      const item = items[index];
      if (!item) return [];
      const image = new window.Image();
      image.decoding = "async";
      image.src = gateway.mediaAssetUrl(sessionId, item.ordinal);
      return [image];
    });
    return () => preloaders.forEach((image) => { image.src = ""; });
  }, [gateway, items, page, sessionId]);
}

function usePrerenderedPages(
  pdf: PdfDocument | null,
  pages: number[],
  boxFor: (index: number) => PageBox,
) {
  const latest = useRef(boxFor);
  latest.current = boxFor;
  const key = [...new Set(pages)].join(",");
  useEffect(() => {
    if (!pdf || !key) return;
    // After the shown pages have their turn, the neighbours are drawn ahead.
    const timer = window.setTimeout(() => {
      for (const index of key.split(",").map(Number)) {
        if (index >= 0 && index < pdf.pageCount) {
          void pdf.render(index, latest.current(index).width).catch(() => undefined);
        }
      }
    }, 250);
    return () => window.clearTimeout(timer);
  }, [pdf, key]);
}
