import { useQuery } from "@tanstack/react-query";
import {
  ArrowLeft,
  BookOpen,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  FileText,
  Headphones,
  Image as ImageIcon,
  LoaderCircle,
  Repeat2,
  Shuffle,
  Video,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import type { CatalogDetailGateway } from "../catalog";
import { visualImageUrls } from "../catalog/workImages";
import { effectiveIdentity, installationTitle } from "./libraryPresentation";
import { useMediaPlayback } from "./MediaPlaybackProvider";
import { isReaderAction, useImageReader } from "./ImageReaderProvider";
import {
  mediaItemName,
  mediaProgressPercent,
  mediaSessionTitleMessageKey,
  mediaStatusMessageKey,
  orderedSessionItems,
} from "./mediaSession";
import type {
  LibraryGateway,
  MediaRepeatMode,
  MediaSession,
  MediaSessionItem,
  UpdateMediaQueueSettingsRequest,
  UpdateMediaProgressRequest,
} from "./types";
import { VideoPlayer } from "./VideoPlayer";

interface MediaSessionPageProps {
  installationId: string;
  gateway: LibraryGateway;
  catalogGateway?: Pick<CatalogDetailGateway, "read">;
  onBack: () => void | Promise<void>;
}

type WritableMediaProgress = Omit<UpdateMediaProgressRequest, "sessionId">;

export function MediaSessionPage({
  installationId,
  gateway,
  catalogGateway,
  onBack,
}: MediaSessionPageProps) {
  const { t } = usePresentation();
  const playback = useMediaPlayback();
  const reader = useImageReader();
  const installation = useQuery({
    queryKey: ["library", "installation", installationId],
    queryFn: () => gateway.readInstallation(installationId),
  });
  const session = useQuery({
    queryKey: ["library", "media", "open", installationId],
    queryFn: () => gateway.openMediaSession(installationId),
  });
  const catalogCode = installation.data ? effectiveIdentity(installation.data) : null;
  const catalogWork = useQuery({
    queryKey: ["catalog", "work", catalogCode],
    queryFn: () => catalogGateway!.read(catalogCode!),
    enabled: Boolean(catalogGateway && catalogCode),
    retry: false,
  });
  const posterUrls = useMemo(
    () => catalogWork.data ? visualImageUrls(catalogWork.data) : [],
    [catalogWork.data],
  );
  const audioSession = session.data?.action === "play_audio" ? session.data : null;
  const readerSession = session.data && isReaderAction(session.data.action) ? session.data : null;

  useEffect(() => {
    if (!audioSession) return;
    void playback.openWork(audioSession.installationId);
    void onBack();
  }, [audioSession, onBack, playback]);

  useEffect(() => {
    if (!readerSession) return;
    void reader.open(readerSession.installationId);
    void onBack();
  }, [onBack, reader, readerSession]);

  if (installation.isPending || session.isPending) {
    return (
      <main className="media-session-page media-session-state" aria-live="polite">
        <LoaderCircle className="library-spin" aria-hidden="true" />
        <strong>{t("media.opening")}</strong>
      </main>
    );
  }

  const error = installation.error ?? session.error;
  if (error) {
    return (
      <main className="media-session-page media-session-state" role="alert">
        <strong>{t("media.openFailed")}</strong>
        <span>{t("common.technicalDetail", { detail: error instanceof Error ? error.message : String(error) })}</span>
        <button className="library-back-button" type="button" onClick={() => void onBack()}>
          <ArrowLeft aria-hidden="true" />{t("library.back")}
        </button>
      </main>
    );
  }

  if (!installation.data || !session.data) {
    return (
      <main className="media-session-page media-session-state" role="alert">
        <strong>{t("media.openFailed")}</strong>
        <button className="library-back-button" type="button" onClick={() => void onBack()}>
          <ArrowLeft aria-hidden="true" />{t("library.back")}
        </button>
      </main>
    );
  }

  return (
    <MediaSessionView
      key={session.data.id}
      gateway={gateway}
      initialSession={session.data}
      installationName={installationTitle(installation.data)}
      posterUrls={posterUrls}
      onBack={onBack}
    />
  );
}

function MediaSessionView({
  gateway,
  initialSession,
  installationName,
  posterUrls,
  onBack,
}: {
  gateway: LibraryGateway;
  initialSession: MediaSession;
  installationName: string;
  posterUrls: string[];
  onBack: () => void | Promise<void>;
}) {
  const { t } = usePresentation();
  const initialProgress = useMemo(() => writableProgress(initialSession), [initialSession]);
  const [progress, setProgress] = useState(initialProgress);
  const [queueSettings, setQueueSettings] = useState({
    repeatMode: initialSession.repeatMode,
    shuffle: initialSession.shuffle,
  });
  const [settingsPending, setSettingsPending] = useState(false);
  const [autoPlayOrdinal, setAutoPlayOrdinal] = useState<number | null>(() => (
    initialSession.action === "play_audio" || initialSession.action === "play_video"
      ? initialProgress.itemOrdinal
      : null
  ));
  const [saveError, setSaveError] = useState("");
  const [closing, setClosing] = useState(false);
  const latestRef = useRef(initialProgress);
  const latestRevisionRef = useRef(0);
  const queuedRevisionRef = useRef(0);
  const timerRef = useRef<number | null>(null);
  const queueRef = useRef<Promise<void>>(Promise.resolve());
  const mountedRef = useRef(true);
  const orderedItems = useMemo(
    () => orderedSessionItems(
      initialSession,
      initialSession.action === "read_images" ? false : queueSettings.shuffle,
    ),
    [initialSession, queueSettings.shuffle],
  );
  const currentItem = orderedItems.find((item) => item.ordinal === progress.itemOrdinal)
    ?? orderedItems[0];

  const enqueue = useCallback((value: WritableMediaProgress, revision: number) => {
    queuedRevisionRef.current = Math.max(queuedRevisionRef.current, revision);
    const operation = queueRef.current
      .catch(() => undefined)
      .then(async () => {
        await gateway.updateMediaProgress({ sessionId: initialSession.id, ...value });
        if (mountedRef.current) setSaveError("");
      })
      .catch((error: unknown) => {
        if (mountedRef.current) {
          setSaveError(error instanceof Error ? error.message : String(error));
        }
        throw error;
      });
    queueRef.current = operation.catch(() => undefined);
    return operation;
  }, [gateway, initialSession.id]);

  const clearTimer = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const persistNow = useCallback(async (value: WritableMediaProgress) => {
    clearTimer();
    const revision = latestRevisionRef.current + 1;
    latestRevisionRef.current = revision;
    latestRef.current = value;
    setProgress(value);
    await enqueue(value, revision);
  }, [clearTimer, enqueue]);

  const schedulePersist = useCallback((value: WritableMediaProgress) => {
    latestRevisionRef.current += 1;
    latestRef.current = value;
    setProgress(value);
    if (timerRef.current !== null) return;
    timerRef.current = window.setTimeout(() => {
      timerRef.current = null;
      void enqueue(latestRef.current, latestRevisionRef.current).catch(() => undefined);
    }, 2_000);
  }, [enqueue]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      clearTimer();
      if (latestRevisionRef.current > queuedRevisionRef.current) {
        void enqueue(latestRef.current, latestRevisionRef.current).catch(() => undefined);
      }
    };
  }, [clearTimer, enqueue]);

  const chooseItem = useCallback((ordinal: number, autoPlay = false) => {
    if (ordinal === latestRef.current.itemOrdinal) return;
    setAutoPlayOrdinal(autoPlay ? ordinal : null);
    void persistNow({
      itemOrdinal: ordinal,
      positionMs: 0,
      durationMs: null,
      completed: false,
      status: "active",
    }).catch(() => undefined);
  }, [persistNow]);

  const updateTimedProgress = useCallback((
    itemOrdinal: number,
    positionMs: number,
    durationMs: number | null,
    status: "active" | "paused",
  ) => {
    if (itemOrdinal !== latestRef.current.itemOrdinal) return;
    schedulePersist({
      itemOrdinal,
      positionMs,
      durationMs,
      completed: false,
      status,
    });
  }, [schedulePersist]);

  const updatePlaybackState = useCallback((itemOrdinal: number, status: "active" | "paused") => {
    if (itemOrdinal !== latestRef.current.itemOrdinal) return;
    void persistNow({ ...latestRef.current, completed: false, status }).catch(() => undefined);
  }, [persistNow]);

  const complete = useCallback(() => {
    void persistNow({
      ...latestRef.current,
      positionMs: latestRef.current.durationMs ?? latestRef.current.positionMs,
      completed: true,
      status: "completed",
    }).catch(() => undefined);
  }, [persistNow]);

  const finishTimedItem = useCallback((itemOrdinal: number) => {
    if (itemOrdinal !== latestRef.current.itemOrdinal) return;
    if (queueSettings.repeatMode === "one") return;
    const currentIndex = orderedItems.findIndex((item) => (
      item.ordinal === itemOrdinal
    ));
    const next = orderedItems[currentIndex + 1]
      ?? (queueSettings.repeatMode === "all" ? orderedItems[0] : undefined);
    if (next) chooseItem(next.ordinal, true);
    else complete();
  }, [chooseItem, complete, orderedItems, queueSettings.repeatMode]);

  const updateQueueSettings = async (next: UpdateMediaQueueSettingsRequest) => {
    if (settingsPending) return;
    const previous = queueSettings;
    setQueueSettings({ repeatMode: next.repeatMode, shuffle: next.shuffle });
    setSettingsPending(true);
    setSaveError("");
    try {
      await gateway.updateMediaQueueSettings(next);
    } catch (error) {
      setQueueSettings(previous);
      setSaveError(error instanceof Error ? error.message : String(error));
    } finally {
      setSettingsPending(false);
    }
  };

  const changeRepeatMode = () => {
    const repeatMode = nextRepeatMode(queueSettings.repeatMode);
    void updateQueueSettings({
      sessionId: initialSession.id,
      repeatMode,
      shuffle: queueSettings.shuffle,
    });
  };

  const changeShuffle = () => {
    void updateQueueSettings({
      sessionId: initialSession.id,
      repeatMode: queueSettings.repeatMode,
      shuffle: !queueSettings.shuffle,
    });
  };

  const closeAndBack = async () => {
    if (closing) return;
    setClosing(true);
    setSaveError("");
    try {
      if (latestRef.current.status !== "completed") {
        await persistNow({ ...latestRef.current, completed: false, status: "paused" });
      }
      await gateway.closeMediaSession(initialSession.id);
      await onBack();
    } catch (error) {
      setSaveError(error instanceof Error ? error.message : String(error));
      setClosing(false);
    }
  };

  if (initialSession.action === "play_video") {
    return (
      <VideoPlayer
        gateway={gateway}
        session={initialSession}
        installationName={installationName}
        items={orderedItems}
        currentOrdinal={progress.itemOrdinal}
        positionMs={progress.positionMs}
        durationMs={progress.durationMs}
        completed={progress.completed}
        autoPlay={autoPlayOrdinal === progress.itemOrdinal}
        repeatMode={queueSettings.repeatMode}
        shuffle={queueSettings.shuffle}
        saveError={saveError}
        closing={closing}
        posterUrls={posterUrls}
        onChoose={chooseItem}
        onProgress={updateTimedProgress}
        onPlaybackState={(itemOrdinal, status) => {
          if (status === "active") setAutoPlayOrdinal(null);
          updatePlaybackState(itemOrdinal, status);
        }}
        onEnded={finishTimedItem}
        onComplete={complete}
        onRepeatMode={(repeatMode) => {
          void updateQueueSettings({
            sessionId: initialSession.id,
            repeatMode,
            shuffle: queueSettings.shuffle,
          });
        }}
        onShuffle={(shuffle) => {
          void updateQueueSettings({
            sessionId: initialSession.id,
            repeatMode: queueSettings.repeatMode,
            shuffle,
          });
        }}
        onBack={() => void closeAndBack()}
      />
    );
  }

  const progressPercent = mediaProgressPercent({
    ...initialSession,
    progress: {
      ...initialSession.progress,
      itemOrdinal: progress.itemOrdinal,
      positionMs: progress.positionMs,
      durationMs: progress.durationMs,
      completed: progress.completed,
    },
  });

  return (
    <main className={`media-session-page media-session-${initialSession.action}`}>
      <header className="media-session-heading">
        <button className="library-back-button" type="button" disabled={closing} onClick={() => void closeAndBack()}>
          {closing ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <ArrowLeft aria-hidden="true" />}
          {t("library.back")}
        </button>
        <div>
          <span className="library-eyebrow">{mediaSessionIcon(initialSession)}{t(mediaSessionTitleMessageKey(initialSession))}</span>
          <h1>{installationName}</h1>
          <p>{currentItem ? mediaItemName(currentItem) : t("media.noItems")}</p>
        </div>
        <span className={`media-session-status media-session-status-${progress.status}`}>
          {progress.status === "completed" && <CheckCircle2 aria-hidden="true" />}
          {t(mediaStatusMessageKey(progress.status))}
        </span>
      </header>

      {saveError && <div className="library-callout library-callout-error" role="alert">{t("common.requestFailed", { error: saveError })}</div>}

      <section className="media-session-workspace">
        <div className="media-session-stage">
          {currentItem ? (
            <MediaSurface
              gateway={gateway}
              session={initialSession}
              item={currentItem}
            />
          ) : <strong>{t("media.noItems")}</strong>}
          {initialSession.action === "open_document" && (
            <ReaderControls
              gateway={gateway}
              session={initialSession}
              items={orderedItems}
              currentOrdinal={progress.itemOrdinal}
              completed={progress.completed}
              onChoose={chooseItem}
              onComplete={complete}
            />
          )}
        </div>

        <aside className="media-session-queue" aria-label={t("media.queue")}>
          <header>
            <span>{t("media.queue")}</span>
            <strong>{Math.max(0, orderedItems.findIndex((item) => item.ordinal === progress.itemOrdinal)) + 1} / {orderedItems.length}</strong>
          </header>
          {initialSession.action === "play_audio" ? (
            <div className="media-session-queue-controls">
              <button
                className={queueSettings.shuffle ? "active" : undefined}
                type="button"
                aria-pressed={queueSettings.shuffle}
                disabled={settingsPending}
                onClick={changeShuffle}
              >
                {settingsPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <Shuffle aria-hidden="true" />}
                {t("media.shuffle")}
              </button>
              <button
                className={queueSettings.repeatMode !== "off" ? "active" : undefined}
                type="button"
                disabled={settingsPending}
                onClick={changeRepeatMode}
              >
                <Repeat2 aria-hidden="true" />
                {t(repeatModeMessageKey(queueSettings.repeatMode))}
              </button>
            </div>
          ) : null}
          <div className="media-session-overall-progress" aria-hidden="true">
            <span style={{ width: `${progressPercent ?? 0}%` }} />
          </div>
          <ol>
            {orderedItems.map((item, index) => (
              <li key={item.ordinal}>
                <button
                  className={item.ordinal === progress.itemOrdinal ? "active" : undefined}
                  type="button"
                  onClick={() => chooseItem(item.ordinal, initialSession.action === "play_audio")}
                >
                  <span>{String(index + 1).padStart(2, "0")}</span>
                  {initialSession.action === "read_images" ? (
                    <img src={gateway.mediaAssetUrl(initialSession.id, item.ordinal)} alt="" loading="lazy" />
                  ) : mediaItemIcon(item)}
                  <strong>
                    {initialSession.kind === "personalized_voice" && item.workCode
                      ? <small>{item.workCode}</small>
                      : null}
                    {mediaItemName(item)}
                  </strong>
                </button>
              </li>
            ))}
          </ol>
        </aside>
      </section>
    </main>
  );
}

function MediaSurface({
  gateway,
  session,
  item,
}: {
  gateway: LibraryGateway;
  session: MediaSession;
  item: MediaSessionItem;
}) {
  const source = gateway.mediaAssetUrl(session.id, item.ordinal);
  return <iframe className="media-document-surface" src={source} title={mediaItemName(item)} />;
}

function ReaderControls({
  gateway,
  session,
  items,
  currentOrdinal,
  completed,
  onChoose,
  onComplete,
}: {
  gateway: LibraryGateway;
  session: MediaSession;
  items: MediaSessionItem[];
  currentOrdinal: number;
  completed: boolean;
  onChoose: (ordinal: number) => void;
  onComplete: () => void;
}) {
  const { t } = usePresentation();
  const stripRef = useRef<HTMLDivElement>(null);
  const index = items.findIndex((item) => item.ordinal === currentOrdinal);
  const previous = items[index - 1];
  const next = items[index + 1];
  const showThumbnails = session.action === "read_images";

  useEffect(() => {
    const strip = stripRef.current;
    const active = strip?.querySelector<HTMLElement>("[data-active='true']");
    active?.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "center" });
  }, [currentOrdinal]);

  return (
    <div className="media-reader-strip">
      <nav className="media-reader-controls" aria-label={t("media.readerControls")}>
        <button type="button" disabled={!previous} onClick={() => previous && onChoose(previous.ordinal)}>
          <ChevronLeft aria-hidden="true" />{t("media.previous")}
        </button>
        <button type="button" disabled={completed} onClick={onComplete}>
          <CheckCircle2 aria-hidden="true" />{t(completed ? "media.completed" : "media.markFinished")}
        </button>
        <button type="button" disabled={!next} onClick={() => next && onChoose(next.ordinal)}>
          {t("media.next")}<ChevronRight aria-hidden="true" />
        </button>
      </nav>
      <div className="media-reader-filmstrip" ref={stripRef} role="group" aria-label={t("media.filmstrip")}>
        {items.map((item, position) => (
          <button
            className={item.ordinal === currentOrdinal ? "media-reader-thumb active" : "media-reader-thumb"}
            data-active={item.ordinal === currentOrdinal}
            type="button"
            key={item.ordinal}
            title={mediaItemName(item)}
            aria-label={t("media.goToItem", { number: position + 1 })}
            aria-current={item.ordinal === currentOrdinal ? "true" : undefined}
            onClick={() => onChoose(item.ordinal)}
          >
            {showThumbnails ? (
              <img
                src={gateway.mediaAssetUrl(session.id, item.ordinal)}
                alt=""
                loading="lazy"
                decoding="async"
              />
            ) : (
              <FileText aria-hidden="true" />
            )}
            <b>{position + 1}</b>
          </button>
        ))}
      </div>
    </div>
  );
}

function writableProgress(session: MediaSession): WritableMediaProgress {
  return {
    itemOrdinal: session.progress.itemOrdinal,
    positionMs: session.progress.positionMs,
    durationMs: session.progress.durationMs,
    completed: session.progress.completed,
    status: session.status === "paused" ? "paused" : session.status === "completed" ? "completed" : "active",
  };
}

function mediaSessionIcon(session: MediaSession) {
  switch (session.action) {
    case "play_audio": return <Headphones aria-hidden="true" />;
    case "read_images": return <ImageIcon aria-hidden="true" />;
    case "open_document": return <BookOpen aria-hidden="true" />;
    default: return <Video aria-hidden="true" />;
  }
}

function mediaItemIcon(item: MediaSessionItem) {
  switch (item.mediaType) {
    case "audio": return <Headphones aria-hidden="true" />;
    case "video": return <Video aria-hidden="true" />;
    case "pdf": return <FileText aria-hidden="true" />;
    default: return <ImageIcon aria-hidden="true" />;
  }
}

function nextRepeatMode(current: MediaRepeatMode): MediaRepeatMode {
  if (current === "off") return "all";
  if (current === "all") return "one";
  return "off";
}

function repeatModeMessageKey(mode: MediaRepeatMode) {
  if (mode === "all") return "media.repeatAll" as const;
  if (mode === "one") return "media.repeatOne" as const;
  return "media.repeatOff" as const;
}
