import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Folder,
  Image as ImageIcon,
  Keyboard,
  LayoutGrid,
  LoaderCircle,
  Maximize,
  Minimize,
  Pause,
  Play,
  SlidersHorizontal,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import {
  Fragment,
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

import { useBoundKeys } from "../../preferences/KeyBindingsProvider";
import type { MessageKey } from "../../preferences/preferences";
import { usePresentation } from "../../preferences/PresentationProvider";
import { FolderChoices, ImageGallery } from "./ImageGallery";
import {
  SLIDESHOW_INTERVALS,
  clampViewerZoom,
  filmstripWindow,
  fitScale,
  formatFileSize,
  imageBox,
  imageFolders,
  minimapSize,
  readImageViewerPreferences,
  scrollToPoint,
  viewerSequence,
  visibleRegion,
  writeImageViewerPreferences,
  type GallerySize,
  type ImageFolder,
  type ImageRegion,
  type ImageSetViewer,
  type ImageViewerPreferences,
  type ViewerBackground,
  type ViewerFit,
} from "./imageViewerModel";
import { mediaItemName } from "./mediaSession";
import {
  ReaderShortcuts,
  guardKeys,
  scrollBehavior,
  useElementSize,
  useEscapeClosesPanel,
  useFullscreen,
} from "./readerControls";
import type { PageBox } from "./readerModel";
import { ImagePage } from "./ReaderPages";
import { useReader, type ReaderPosition } from "./ReaderProvider";
import type { LibraryGateway } from "./types";

const ZOOM_FACTOR = 1.25;
const WHEEL_TURN_DELAY_MS = 320;
const WHEEL_TURN_THRESHOLD = 35;
const DRAG_THRESHOLD_PX = 6;
const EDGE_REVEAL_TOP_PX = 90;
const EDGE_REVEAL_BOTTOM_PX = 170;
const RESUME_NOTICE_MS = 7_000;
const COMPACT_STAGE_PX = 640;
const VIEWER_ACTIONS: ReadonlySet<string> = new Set([
  "readerNextPage",
  "readerPreviousPage",
  "readerScrollBack",
  "readerScrollForward",
  "readerFirstPage",
  "readerLastPage",
  "readerPreviousFolder",
  "readerNextFolder",
  "readerGallery",
  "readerSlideshow",
  "readerActualSize",
  "readerZoomIn",
  "readerZoomOut",
  "readerResetZoom",
  "readerFullscreen",
  "readerClose",
]);

export type ImageViewerGateway = Pick<LibraryGateway, "mediaAssetUrl" | "mediaThumbnailUrl">;

interface DragState {
  pointerId: number;
  x: number;
  y: number;
  left: number;
  top: number;
  moved: boolean;
}

interface ZoomFocus {
  x: number;
  y: number;
  rx: number;
  ry: number;
}

interface Point {
  x: number;
  y: number;
}

/** The CG viewer: one image at a time, a gallery of the whole set, and slideshows. */
export function ImageViewer({
  gateway,
  onOpenIn,
}: {
  gateway: ImageViewerGateway;
  onOpenIn: (viewer: ImageSetViewer, position: ReaderPosition) => void;
}) {
  const reader = useReader();
  const session = reader.session!;
  const { items, start } = reader;
  const { locale, t } = usePresentation();
  const preferEnglish = locale !== "ja-JP";
  const title = reader.title(preferEnglish);
  const creator = reader.creator(preferEnglish);
  const count = items.length;
  const rootRef = useRef<HTMLElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const imageRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const draggedRef = useRef(false);
  const wheelAtRef = useRef(0);
  const focusRef = useRef<ZoomFocus | null>(null);

  const [preferences, setPreferences] = useState<ImageViewerPreferences>(() => (
    readImageViewerPreferences(session.installationId)
  ));
  const [index, setIndex] = useState(() => Math.min(Math.max(0, start.page), Math.max(0, count - 1)));
  const [scope, setScope] = useState<string | null>(null);
  const [view, setView] = useState<"viewer" | "gallery">("viewer");
  const [chrome, setChrome] = useState(true);
  const [panel, setPanel] = useState<"settings" | "shortcuts" | "folders" | null>(null);
  const [resumeNotice, setResumeNotice] = useState(() => reader.resumed && start.page > 0);
  const [endCard, setEndCard] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [zoom, setZoom] = useState<number | null>(null);
  const [sizes, setSizes] = useState<ReadonlyMap<number, PageBox>>(new Map());
  const stage = useElementSize(stageRef);
  const fullscreen = useFullscreen(rootRef);

  const folders = useMemo(() => imageFolders(items), [items]);
  const folderOf = useMemo(() => {
    const byIndex = new Map<number, ImageFolder>();
    for (const folder of folders) for (const at of folder.indexes) byIndex.set(at, folder);
    return byIndex;
  }, [folders]);
  const sequence = useMemo(() => viewerSequence(folders, count, scope), [count, folders, scope]);
  const at = Math.max(0, sequence.indexOf(index));
  const item = items[index];
  const natural = sizes.get(index);
  const box = imageBox(natural, stage, preferences.fit, zoom);
  const folderName = useCallback((folder: ImageFolder | undefined) => (
    folder?.name || t("viewer.topFolder")
  ), [t]);

  useEffect(() => {
    writeImageViewerPreferences(session.installationId, preferences);
  }, [preferences, session.installationId]);

  useEffect(() => {
    if (!resumeNotice) return;
    const timer = window.setTimeout(() => setResumeNotice(false), RESUME_NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [resumeNotice]);

  useEffect(() => {
    if (!count) return;
    const current = { ordinal: items[index]!.ordinal, page: index };
    if (index === count - 1 && !reader.finished) reader.finish(current, count);
    else reader.move(current, count);
    // Only a change of place is saved, not every render of the viewer.
  }, [count, index, items]);

  const rememberSize = useCallback((at: number, size: PageBox) => {
    setSizes((current) => {
      const known = current.get(at);
      if (known && known.width === size.width && known.height === size.height) return current;
      return new Map(current).set(at, size);
    });
  }, []);

  const show = useCallback((next: number, hide = false) => {
    setIndex(next);
    setZoom(null);
    setResumeNotice(false);
    setEndCard(false);
    if (hide && preferences.hideControls) {
      setChrome(false);
      setPanel(null);
    }
  }, [preferences.hideControls]);

  const step = useCallback((delta: -1 | 1, bySlideshow = false) => {
    const next = at + delta;
    if (next < 0) return;
    if (next >= sequence.length) {
      if (bySlideshow && preferences.loop) {
        show(sequence[0]!);
        return;
      }
      setPlaying(false);
      setEndCard(true);
      setChrome(true);
      return;
    }
    show(sequence[next]!, !bySlideshow);
  }, [at, preferences.loop, sequence, show]);

  const scrollOrStep = useCallback((delta: -1 | 1) => {
    const scroller = stageRef.current;
    if (scroller && scroller.scrollHeight > scroller.clientHeight + 1) {
      const room = delta > 0
        ? scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop
        : scroller.scrollTop;
      if (room > 1) {
        scroller.scrollBy({ top: delta * Math.max(160, scroller.clientHeight * 0.85), behavior: scrollBehavior() });
        return;
      }
    }
    step(delta);
  }, [step]);

  useEffect(() => {
    if (!playing || view !== "viewer" || endCard) return;
    const timer = window.setTimeout(() => step(1, true), preferences.interval * 1000);
    return () => window.clearTimeout(timer);
  }, [endCard, playing, preferences.interval, step, view]);

  const togglePlay = useCallback(() => {
    if (playing) {
      setPlaying(false);
      return;
    }
    if (at >= sequence.length - 1) show(sequence[0]!);
    setView("viewer");
    setPanel(null);
    setEndCard(false);
    setResumeNotice(false);
    if (preferences.hideControls) setChrome(false);
    setPlaying(true);
  }, [at, playing, preferences.hideControls, sequence, show]);

  const toggleGallery = useCallback(() => {
    setPlaying(false);
    setPanel(null);
    setEndCard(false);
    setResumeNotice(false);
    setChrome(true);
    setView((current) => (current === "viewer" ? "gallery" : "viewer"));
  }, []);

  const chooseScope = useCallback((key: string | null) => {
    setScope(key);
    setPanel(null);
    setEndCard(false);
    const next = viewerSequence(folders, count, key);
    if (!next.includes(index)) show(next[0]!);
  }, [count, folders, index, show]);

  const jumpFolder = useCallback((delta: -1 | 1) => {
    const place = folders.findIndex((folder) => folder === folderOf.get(index));
    const target = folders[place + delta];
    if (!target) return;
    if (scope !== null) setScope(target.key);
    show(target.indexes[0]!, true);
  }, [folderOf, folders, index, scope, show]);

  const zoomTo = useCallback((value: number | null, point?: Point) => {
    const stageElement = stageRef.current;
    const image = imageRef.current;
    if (stageElement && image) {
      const area = stageElement.getBoundingClientRect();
      const rect = image.getBoundingClientRect();
      const x = point?.x ?? area.left + area.width / 2;
      const y = point?.y ?? area.top + area.height / 2;
      focusRef.current = {
        x: x - area.left,
        y: y - area.top,
        rx: rect.width ? Math.min(1, Math.max(0, (x - rect.left) / rect.width)) : 0.5,
        ry: rect.height ? Math.min(1, Math.max(0, (y - rect.top) / rect.height)) : 0.5,
      };
    }
    setZoom(value === null ? null : clampViewerZoom(value));
  }, []);

  // A zoom keeps the spot under the pointer where it was.
  useLayoutEffect(() => {
    const focus = focusRef.current;
    const stageElement = stageRef.current;
    const image = imageRef.current;
    focusRef.current = null;
    if (!focus || !stageElement || !image) return;
    stageElement.scrollLeft = image.offsetLeft + focus.rx * image.offsetWidth - focus.x;
    stageElement.scrollTop = image.offsetTop + focus.ry * image.offsetHeight - focus.y;
  }, [zoom]);

  const changeZoom = useCallback((factor: number, point?: Point) => {
    if (!natural) return;
    zoomTo((zoom ?? fitScale(natural, stage, preferences.fit)) * factor, point);
  }, [natural, preferences.fit, stage, zoom, zoomTo]);

  const toggleActualSize = useCallback((point?: Point) => {
    if (zoom !== null) {
      zoomTo(null, point);
      return;
    }
    if (!natural) return;
    zoomTo(preferences.fit === "original" ? fitScale(natural, stage, "screen") : 1, point);
  }, [natural, preferences.fit, stage, zoom, zoomTo]);

  const update = useCallback(<K extends keyof ImageViewerPreferences>(key: K, value: ImageViewerPreferences[K]) => {
    setPreferences((current) => ({ ...current, [key]: value }));
    if (key === "fit") setZoom(null);
  }, []);

  const openIn = useCallback((viewer: ImageSetViewer) => {
    update("openIn", viewer);
    writeImageViewerPreferences(session.installationId, { ...preferences, openIn: viewer });
    onOpenIn(viewer, { ordinal: items[index]?.ordinal ?? 0, page: index });
  }, [index, items, onOpenIn, preferences, session.installationId, update]);

  // One listener for the viewer's life that always acts on the latest image.
  const keyActions = useRef({
    view, step, scrollOrStep, show, sequence, changeZoom, zoomTo, toggleActualSize, jumpFolder, toggleGallery, togglePlay, fullscreen,
  });
  keyActions.current = {
    view, step, scrollOrStep, show, sequence, changeZoom, zoomTo, toggleActualSize, jumpFolder, toggleGallery, togglePlay, fullscreen,
  };
  const viewerKeys = useMemo(() => {
    type Actions = typeof keyActions.current;
    const inViewer = (action: (actions: Actions) => void) => () => {
      if (keyActions.current.view === "viewer") action(keyActions.current);
    };
    return guardKeys({
      readerNextPage: inViewer((actions) => actions.step(1)),
      readerPreviousPage: inViewer((actions) => actions.step(-1)),
      readerScrollForward: inViewer((actions) => actions.scrollOrStep(1)),
      readerScrollBack: inViewer((actions) => actions.scrollOrStep(-1)),
      readerFirstPage: inViewer((actions) => actions.show(actions.sequence[0]!)),
      readerLastPage: inViewer((actions) => actions.show(actions.sequence.at(-1)!)),
      readerPreviousFolder: inViewer((actions) => actions.jumpFolder(-1)),
      readerNextFolder: inViewer((actions) => actions.jumpFolder(1)),
      readerZoomIn: inViewer((actions) => actions.changeZoom(ZOOM_FACTOR)),
      readerZoomOut: inViewer((actions) => actions.changeZoom(1 / ZOOM_FACTOR)),
      readerResetZoom: inViewer((actions) => actions.zoomTo(null)),
      readerActualSize: inViewer((actions) => actions.toggleActualSize()),
      readerGallery: () => keyActions.current.toggleGallery(),
      readerSlideshow: () => keyActions.current.togglePlay(),
      readerFullscreen: () => keyActions.current.fullscreen.toggle(),
    });
  }, []);
  useBoundKeys("reader", viewerKeys, { ignoreInteractive: false });

  const closePanel = useCallback(() => setPanel(null), []);
  useEscapeClosesPanel(panel !== null, closePanel);

  const onWheel = (event: ReactWheelEvent<HTMLElement>) => {
    if (view !== "viewer") return;
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      changeZoom(event.deltaY < 0 ? ZOOM_FACTOR : 1 / ZOOM_FACTOR, { x: event.clientX, y: event.clientY });
      return;
    }
    const scroller = stageRef.current;
    if (scroller && (scroller.scrollHeight > scroller.clientHeight + 1 || scroller.scrollWidth > scroller.clientWidth + 1)) {
      event.preventDefault();
      scroller.scrollBy({ left: event.deltaX, top: event.deltaY });
      return;
    }
    const delta = Math.abs(event.deltaY) >= Math.abs(event.deltaX) ? event.deltaY : event.deltaX;
    if (Math.abs(delta) < WHEEL_TURN_THRESHOLD || Date.now() - wheelAtRef.current < WHEEL_TURN_DELAY_MS) return;
    wheelAtRef.current = Date.now();
    step(delta > 0 ? 1 : -1);
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    draggedRef.current = false;
    const scroller = stageRef.current;
    if (view !== "viewer" || event.button !== 0 || !scroller) return;
    if ((event.target as Element).closest(".reader-bar, .reader-panel, .reader-card, .image-viewer-minimap")) return;
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
    if (view === "viewer" && (event.clientY < EDGE_REVEAL_TOP_PX || event.clientY > window.innerHeight - EDGE_REVEAL_BOTTOM_PX)) {
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
  // Clicks on the image leave focus on the viewer, so the arrow keys keep
  // working and no focus ring lights up on a click area.
  const keepFocus = (event: ReactMouseEvent) => event.preventDefault();

  const neighbours = useMemo(
    () => [at + 1, at - 1, at + 2].flatMap((place) => (sequence[place] === undefined ? [] : [sequence[place]!])),
    [at, sequence],
  );
  usePreloadedImages(neighbours, items, gateway, session.id, rememberSize);

  const multipleFolders = folders.length > 1;
  const currentFolder = folderOf.get(index);
  const scopeLabel = scope === null ? t("viewer.allImages") : folderName(folders.find((folder) => folder.key === scope));
  const positionLabel = count ? t("reader.position", { page: at + 1, total: sequence.length }) : "";
  const countLabel = count === 1 ? t("viewer.imageCountOne") : t("viewer.imageCount", { count });
  const zoomLabel = zoom !== null
    ? `${Math.round(zoom * 100)}%`
    : t(preferences.fit === "width" ? "media.reader.fitWidth" : preferences.fit === "original" ? "media.reader.originalSize" : "viewer.fitScreen");
  const strip = filmstripWindow(at, sequence.length);
  const thumbnail = item ? gateway.mediaThumbnailUrl(session.id, item.ordinal) : "";
  const folderFinished = scope !== null && multipleFolders;
  const fileDetails = [
    natural ? `${natural.width} × ${natural.height}` : null,
    item && item.sizeBytes !== null ? formatFileSize(item.sizeBytes, locale) : null,
  ].filter(Boolean).join(" · ");
  const stageReady = stage.width > 0 && stage.height > 0;
  const overflowing = Boolean(natural) && (box.width > stage.width + 1 || box.height > stage.height + 1);

  const titleBlock = (
    <div className="reader-title">
      <strong title={title}>{title}</strong>
      <span>
        {reader.workCode ? <code>{reader.workCode}</code> : null}
        {creator && creator !== reader.workCode ? <span>{creator}</span> : null}
        <span>{t("reader.kindImages")}</span>
        {count ? <span>{countLabel}</span> : null}
        {multipleFolders ? <span>{t("viewer.folderCount", { count: folders.length })}</span> : null}
      </span>
    </div>
  );
  const libraryButton = (
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
  );

  return (
    <main
      className={`reader image-viewer is-${view}${chrome ? " has-chrome" : ""}`}
      data-background={preferences.background}
      ref={rootRef}
      tabIndex={-1}
      onWheel={onWheel}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    >
      {view === "gallery" ? (
        <div className="image-gallery-view">
          <header className="image-gallery-top">
            {libraryButton}
            {titleBlock}
            <GallerySizes size={preferences.gallerySize} onChange={(size) => update("gallerySize", size)} />
            <button className="reader-chip-button" type="button" onClick={togglePlay}>
              <Play aria-hidden="true" />{t("viewer.slideshow")}
            </button>
            <button className="reader-chip-button is-primary" type="button" onClick={toggleGallery}>
              <ImageIcon aria-hidden="true" />{t("viewer.viewer")}
            </button>
          </header>
          <ImageGallery
            items={items}
            folders={folders}
            scope={scope}
            current={index}
            size={preferences.gallerySize}
            folderName={folderName}
            thumbnailUrl={(ordinal) => gateway.mediaThumbnailUrl(session.id, ordinal)}
            onScope={chooseScope}
            onOpen={(next) => {
              show(next);
              setView("viewer");
            }}
          />
        </div>
      ) : (
        <>
          {preferences.background === "ambient" && thumbnail ? (
            <div className="image-viewer-ambient" aria-hidden="true" style={{ backgroundImage: `url("${thumbnail}")` }} />
          ) : null}

          <div className="reader-stage" ref={stageRef}>
            {!count ? (
              <p className="reader-message">{t("media.noItems")}</p>
            ) : !stageReady || !item ? null : (
              <div className="image-viewer-canvas">
                <div className="image-viewer-image" ref={imageRef} key={index} style={{ width: box.width, height: box.height }}>
                  <ImagePage
                    source={gateway.mediaAssetUrl(session.id, item.ordinal)}
                    label={mediaItemName(item)}
                    box={box}
                    onNaturalSize={(size) => rememberSize(index, size)}
                  />
                </div>
              </div>
            )}
          </div>

          {count > 0 ? (
            <div className="reader-zones image-viewer-zones">
              <button type="button" tabIndex={-1} aria-label={t("viewer.previousImage")} onMouseDown={keepFocus} onClick={zone(() => step(-1))} />
              <button
                type="button"
                tabIndex={-1}
                aria-label={t("viewer.toggleControls")}
                onMouseDown={keepFocus}
                onClick={zone(() => setChrome((value) => !value))}
                onDoubleClick={(event) => toggleActualSize({ x: event.clientX, y: event.clientY })}
              />
              <button type="button" tabIndex={-1} aria-label={t("viewer.nextImage")} onMouseDown={keepFocus} onClick={zone(() => step(1))} />
            </div>
          ) : null}

          {chrome && sequence.length > 1 ? (
            <>
              <button
                className="reader-side-arrow is-left"
                type="button"
                aria-label={t("viewer.previousImage")}
                title={t("viewer.previousImage")}
                disabled={at === 0}
                onMouseDown={keepFocus}
                onClick={() => step(-1)}
              >
                <ChevronLeft aria-hidden="true" />
              </button>
              <button
                className="reader-side-arrow is-right"
                type="button"
                aria-label={t("viewer.nextImage")}
                title={t("viewer.nextImage")}
                onMouseDown={keepFocus}
                onClick={() => step(1)}
              >
                <ChevronRight aria-hidden="true" />
              </button>
            </>
          ) : null}

          {overflowing && natural ? (
            <ViewerMinimap
              stageRef={stageRef}
              imageRef={imageRef}
              natural={natural}
              box={box}
              thumbnail={thumbnail}
              compact={stage.width < COMPACT_STAGE_PX}
              label={t("detail.galleryMinimap")}
            />
          ) : null}

          {playing ? (
            <div className="image-viewer-progress" aria-hidden="true">
              <i key={index} style={{ animationDuration: `${preferences.interval}s` }} />
            </div>
          ) : null}

          {chrome ? (
            <header className="reader-bar reader-top">
              {libraryButton}
              {titleBlock}
              {reader.saveError ? (
                <span className="reader-save-error" role="alert" title={t("common.technicalDetail", { detail: reader.saveError })}>
                  <AlertTriangle aria-hidden="true" />{t("reader.saveFailed")}
                </span>
              ) : null}
              {multipleFolders ? (
                <div className="image-viewer-scope-anchor">
                  <button
                    className={`reader-chip-button image-viewer-scope${panel === "folders" ? " is-open" : ""}`}
                    type="button"
                    aria-label={t("viewer.chooseFolder")}
                    aria-expanded={panel === "folders"}
                    title={t("viewer.chooseFolder")}
                    onClick={() => setPanel((current) => (current === "folders" ? null : "folders"))}
                  >
                    <Folder aria-hidden="true" />
                    <span>{scopeLabel}</span>
                    <ChevronDown aria-hidden="true" />
                  </button>
                  {panel === "folders" ? (
                    <section className="reader-panel image-viewer-folders" aria-label={t("viewer.folders")}>
                      <span>{t("viewer.show")}</span>
                      <FolderChoices folders={folders} scope={scope} count={count} folderName={folderName} onChoose={chooseScope} />
                    </section>
                  ) : null}
                </div>
              ) : null}
              {positionLabel ? <span className="reader-position" aria-live="polite">{positionLabel}</span> : null}
              <button className="reader-icon-button" type="button" aria-label={t("viewer.gallery")} title={t("viewer.gallery")} onClick={toggleGallery}>
                <LayoutGrid aria-hidden="true" />
              </button>
              <button
                className={`reader-icon-button${playing ? " is-playing" : ""}`}
                type="button"
                aria-label={t(playing ? "viewer.slideshowPause" : "viewer.slideshowPlay")}
                aria-pressed={playing}
                title={t(playing ? "viewer.slideshowPause" : "viewer.slideshowPlay")}
                onClick={togglePlay}
              >
                {playing ? <Pause aria-hidden="true" /> : <Play aria-hidden="true" />}
              </button>
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
                <SlidersHorizontal aria-hidden="true" />{t("viewer.settings")}
              </button>
            </header>
          ) : null}

          {chrome && count > 0 ? (
            <footer className="reader-bar reader-bottom">
              {sequence.length > 1 ? (
                <div className="image-viewer-strip">
                  {strip.map((place, offset) => {
                    const stripIndex = sequence[place]!;
                    const stripItem = items[stripIndex]!;
                    const folder = folderOf.get(stripIndex);
                    const before = offset > 0 ? folderOf.get(sequence[strip[offset - 1]!]!) : folder;
                    return (
                      <Fragment key={stripIndex}>
                        {folder !== before ? <span className="image-viewer-strip-folder">{folderName(folder)}</span> : null}
                        <button
                          className={`image-viewer-thumb${stripIndex === index ? " is-current" : ""}`}
                          type="button"
                          aria-label={t("viewer.imageLabel", { number: place + 1, name: mediaItemName(stripItem) })}
                          aria-current={stripIndex === index ? "true" : undefined}
                          onClick={() => show(stripIndex)}
                        >
                          <img src={gateway.mediaThumbnailUrl(session.id, stripItem.ordinal)} alt="" loading="lazy" decoding="async" draggable={false} />
                        </button>
                      </Fragment>
                    );
                  })}
                </div>
              ) : null}
              <div className="image-viewer-details">
                <div className="image-viewer-file">
                  <span>
                    {multipleFolders ? <span>{folderName(currentFolder)} /</span> : null}
                    <code title={item ? mediaItemName(item) : undefined}>{item ? mediaItemName(item) : ""}</code>
                  </span>
                  <small>{fileDetails}</small>
                </div>
                {sequence.length > 1 ? (
                  <div className="reader-slider">
                    <span>1</span>
                    <input
                      type="range"
                      min={1}
                      max={sequence.length}
                      value={at + 1}
                      aria-label={t("viewer.images")}
                      aria-valuetext={positionLabel}
                      onChange={(event) => show(sequence[Number(event.currentTarget.value) - 1]!)}
                    />
                    <span>{sequence.length}</span>
                  </div>
                ) : <span />}
                <div className="image-viewer-zoom" role="group" aria-label={t("media.reader.zoom")}>
                  <button
                    className="reader-icon-button"
                    type="button"
                    aria-label={t("media.reader.zoomOut")}
                    title={t("media.reader.zoomOut")}
                    disabled={!natural}
                    onClick={() => changeZoom(1 / ZOOM_FACTOR)}
                  >
                    <ZoomOut aria-hidden="true" />
                  </button>
                  <button
                    className="reader-chip-button"
                    type="button"
                    aria-label={t("viewer.zoomToggle")}
                    title={t("viewer.zoomToggle")}
                    onClick={() => toggleActualSize()}
                  >
                    {zoomLabel}
                  </button>
                  <button
                    className="reader-icon-button"
                    type="button"
                    aria-label={t("media.reader.zoomIn")}
                    title={t("media.reader.zoomIn")}
                    disabled={!natural}
                    onClick={() => changeZoom(ZOOM_FACTOR)}
                  >
                    <ZoomIn aria-hidden="true" />
                  </button>
                </div>
              </div>
            </footer>
          ) : null}

          {panel === "settings" ? (
            <ViewerSettings preferences={preferences} onChange={update} onOpenInReader={() => openIn("reader")} />
          ) : null}
          {panel === "shortcuts" ? <ReaderShortcuts actions={VIEWER_ACTIONS} /> : null}

          {resumeNotice && !endCard ? (
            <div className="reader-toast" role="status">
              <span>{t("viewer.resumed", { image: at + 1 })}</span>
              <button className="reader-chip-button is-outline" type="button" onClick={() => show(sequence[0]!)}>{t("reader.startOver")}</button>
              <button className="reader-icon-button is-plain" type="button" aria-label={t("notice.close")} onClick={() => setResumeNotice(false)}>
                <X aria-hidden="true" />
              </button>
            </div>
          ) : null}

          {endCard ? (
            <div className="reader-veil">
              <section className="reader-card reader-finished" aria-labelledby="image-viewer-finished-title">
                <CheckCircle2 aria-hidden="true" />
                <h2 id="image-viewer-finished-title">{t(folderFinished ? "viewer.folderFinishedTitle" : "viewer.finishedTitle")}</h2>
                <p>{t(folderFinished ? "viewer.folderFinishedHelp" : "viewer.finishedHelp")}</p>
                <div>
                  <button className="reader-chip-button is-primary" type="button" onClick={() => void reader.close()}>
                    {t("library.back")}
                  </button>
                  <button className="reader-chip-button" type="button" onClick={toggleGallery}>
                    {t("viewer.openGallery")}
                  </button>
                  <button className="reader-chip-button" type="button" onClick={() => show(sequence[0]!)}>
                    {t("viewer.viewAgain")}
                  </button>
                </div>
              </section>
            </div>
          ) : null}
        </>
      )}
    </main>
  );
}

const GALLERY_SIZES: ReadonlyArray<readonly [GallerySize, string, MessageKey]> = [
  ["small", "S", "viewer.sizeSmall"],
  ["medium", "M", "viewer.sizeMedium"],
  ["large", "L", "viewer.sizeLarge"],
];

function GallerySizes({ size, onChange }: { size: GallerySize; onChange: (size: GallerySize) => void }) {
  const { t } = usePresentation();
  return (
    <div className="image-gallery-sizes" role="group" aria-label={t("viewer.thumbnailSize")}>
      {GALLERY_SIZES.map(([value, short, label]) => (
        <button
          className={`reader-choice${size === value ? " is-active" : ""}`}
          type="button"
          key={value}
          aria-label={t(label)}
          aria-pressed={size === value}
          title={t(label)}
          onClick={() => onChange(value)}
        >
          {short}
        </button>
      ))}
    </div>
  );
}

const FIT_OPTIONS: ReadonlyArray<readonly [ViewerFit, MessageKey]> = [
  ["screen", "viewer.fitScreen"],
  ["width", "media.reader.fitWidth"],
  ["original", "media.reader.originalSize"],
];
const BACKGROUND_OPTIONS: ReadonlyArray<readonly [ViewerBackground, MessageKey]> = [
  ["black", "reader.backgroundBlack"],
  ["gray", "reader.backgroundGray"],
  ["ambient", "viewer.backgroundAmbient"],
];

function ViewerSettings({
  preferences,
  onChange,
  onOpenInReader,
}: {
  preferences: ImageViewerPreferences;
  onChange: <K extends keyof ImageViewerPreferences>(key: K, value: ImageViewerPreferences[K]) => void;
  onOpenInReader: () => void;
}) {
  const { t } = usePresentation();
  const group = <K extends "fit" | "background">(
    key: K,
    title: MessageKey,
    options: ReadonlyArray<readonly [ImageViewerPreferences[K], MessageKey]>,
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
    <section className="reader-panel reader-settings" aria-label={t("viewer.settingsTitle")}>
      {group("fit", "media.reader.fit", FIT_OPTIONS)}
      {group("background", "reader.background", BACKGROUND_OPTIONS)}
      <div className="reader-setting" role="group" aria-label={t("viewer.slideshow")}>
        <span>{t("viewer.slideshow")}</span>
        <div>
          {SLIDESHOW_INTERVALS.map((seconds) => (
            <button
              className={`reader-choice${preferences.interval === seconds ? " is-active" : ""}`}
              type="button"
              key={seconds}
              aria-pressed={preferences.interval === seconds}
              onClick={() => onChange("interval", seconds)}
            >
              {t("viewer.interval", { seconds })}
            </button>
          ))}
        </div>
      </div>
      <div className="reader-setting" role="group" aria-label={t("viewer.openIn")}>
        <span>{t("viewer.openIn")}</span>
        <div>
          <button className="reader-choice is-active" type="button" aria-pressed>
            {t("viewer.openInViewer")}
          </button>
          <button className="reader-choice" type="button" aria-pressed={false} onClick={onOpenInReader}>
            {t("viewer.openInReader")}
          </button>
        </div>
      </div>
      <label className="reader-check">
        <input type="checkbox" checked={preferences.loop} onChange={(event) => onChange("loop", event.currentTarget.checked)} />
        {t("viewer.loop")}
      </label>
      <label className="reader-check">
        <input
          type="checkbox"
          checked={preferences.hideControls}
          onChange={(event) => onChange("hideControls", event.currentTarget.checked)}
        />
        {t("viewer.hideControls")}
      </label>
    </section>
  );
}

/**
 * The whole image in a corner with the shown part framed; a click or a drag
 * in it moves the zoomed image there.
 */
function ViewerMinimap({
  stageRef,
  imageRef,
  natural,
  box,
  thumbnail,
  compact,
  label,
}: {
  stageRef: RefObject<HTMLDivElement | null>;
  imageRef: RefObject<HTMLDivElement | null>;
  natural: PageBox;
  box: PageBox;
  thumbnail: string;
  compact: boolean;
  label: string;
}) {
  const size = minimapSize(natural, compact);
  const [region, setRegion] = useState<ImageRegion | null>(null);
  const dragging = useRef(false);

  useLayoutEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const measure = () => {
      const image = imageRef.current;
      if (!image) return;
      setRegion(visibleRegion(
        { left: stage.scrollLeft, top: stage.scrollTop },
        { width: stage.clientWidth, height: stage.clientHeight },
        { left: image.offsetLeft, top: image.offsetTop, width: image.offsetWidth, height: image.offsetHeight },
      ));
    };
    measure();
    stage.addEventListener("scroll", measure, { passive: true });
    return () => stage.removeEventListener("scroll", measure);
  }, [box.height, box.width, imageRef, stageRef]);

  const moveTo = (event: ReactPointerEvent<HTMLDivElement>) => {
    const stage = stageRef.current;
    const image = imageRef.current;
    if (!stage || !image) return;
    const area = event.currentTarget.getBoundingClientRect();
    const scroll = scrollToPoint(
      { x: (event.clientX - area.left) / area.width, y: (event.clientY - area.top) / area.height },
      { width: stage.clientWidth, height: stage.clientHeight },
      { left: image.offsetLeft, top: image.offsetTop, width: image.offsetWidth, height: image.offsetHeight },
    );
    stage.scrollLeft = scroll.left;
    stage.scrollTop = scroll.top;
  };

  return (
    <div className="image-viewer-minimap">
      <div
        role="img"
        aria-label={label}
        style={{ width: size.width, height: size.height }}
        onPointerDown={(event) => {
          event.stopPropagation();
          dragging.current = true;
          event.currentTarget.setPointerCapture?.(event.pointerId);
          moveTo(event);
        }}
        onPointerMove={(event) => {
          if (dragging.current) moveTo(event);
        }}
        onPointerUp={() => {
          dragging.current = false;
        }}
        onPointerCancel={() => {
          dragging.current = false;
        }}
      >
        <img src={thumbnail} alt="" draggable={false} />
        {region ? (
          <span
            aria-hidden="true"
            style={{
              left: region.left * size.width,
              top: region.top * size.height,
              width: region.width * size.width,
              height: region.height * size.height,
            }}
          />
        ) : null}
      </div>
    </div>
  );
}

/** The next images load ahead, and their sizes are known before they are shown. */
function usePreloadedImages(
  indexes: number[],
  items: { ordinal: number }[],
  gateway: ImageViewerGateway,
  sessionId: string,
  onSize: (index: number, size: PageBox) => void,
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
      image.onload = () => latest.current(index, { width: image.naturalWidth, height: image.naturalHeight });
      image.src = gateway.mediaAssetUrl(sessionId, item.ordinal);
      return [image];
    });
    return () => preloaders.forEach((image) => {
      image.onload = null;
      image.src = "";
    });
  }, [gateway, items, key, sessionId]);
}
