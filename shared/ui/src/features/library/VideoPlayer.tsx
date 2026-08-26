import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  ChevronRight,
  Film,
  LoaderCircle,
  PanelRightOpen,
  Pause,
  RotateCw,
  Video,
} from "lucide-react";
import {
  type SyntheticEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { useBoundKeys } from "../../preferences/KeyBindingsProvider";
import {
  clampPlaybackPosition,
  clampPlaybackRate,
  restorePlaybackPosition,
} from "./audioPlayer";
import { assetFailureMessageKey, useMediaAssetProbe } from "./mediaAsset";
import { useMediaPlayback, type VideoPlaybackRegistration } from "./MediaPlaybackProvider";
import { mediaItemName } from "./mediaSession";
import {
  NativeVideoSurface,
  resolveNativeVideoGateway,
  type VideoPlayerGateway,
} from "./NativeVideoSurface";
import type {
  MediaRepeatMode,
  MediaSession,
  MediaSessionItem,
  NativeVideoControlRequest,
  NativeVideoState,
  NativeVideoSubtitle,
} from "./types";
import {
  bufferedAhead,
  clampVideoVolume,
  readVideoPlayerPreferences,
  videoPlaybackFailure,
  videoStepTarget,
  writeVideoPlayerPreferences,
  type VideoFit,
  type VideoPlaybackFailure,
  type VideoPlayerPreferences,
  seekFractionForKey,
} from "./videoPlayerModel";

const CONTROLS_HIDE_DELAY_MS = 2_600;
const SKIP_SECONDS = 15;

interface VideoPlayerProps {
  gateway: VideoPlayerGateway;
  session: MediaSession;
  installationName: string;
  items: MediaSessionItem[];
  currentOrdinal: number;
  positionMs: number;
  durationMs: number | null;
  completed: boolean;
  autoPlay: boolean;
  repeatMode: MediaRepeatMode;
  shuffle: boolean;
  saveError: string;
  closing: boolean;
  posterUrls: string[];
  onChoose: (ordinal: number, autoPlay?: boolean) => void;
  onProgress: (
    itemOrdinal: number,
    positionMs: number,
    durationMs: number | null,
    status: "active" | "paused",
  ) => void;
  onPlaybackState: (itemOrdinal: number, status: "active" | "paused") => void;
  onEnded: (itemOrdinal: number) => void;
  onComplete: () => void;
  onRepeatMode: (mode: MediaRepeatMode) => void;
  onShuffle: (shuffle: boolean) => void;
  onBack: () => void;
}

export function VideoPlayer({
  gateway,
  session,
  installationName,
  items,
  currentOrdinal,
  positionMs,
  durationMs,
  completed,
  autoPlay,
  repeatMode,
  shuffle,
  saveError,
  closing,
  posterUrls,
  onChoose,
  onProgress,
  onPlaybackState,
  onEnded,
  onComplete,
  onRepeatMode,
  onShuffle,
  onBack,
}: VideoPlayerProps) {
  const { t } = usePresentation();
  const playback = useMediaPlayback();
  const publishVideoPlayback = playback.publishVideoPlayback;
  const releaseVideoPlayback = playback.releaseVideoPlayback;
  const nativeGateway = useMemo(() => resolveNativeVideoGateway(gateway), [gateway]);
  const rootRef = useRef<HTMLElement>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const hideTimerRef = useRef<number | null>(null);
  const shouldPlayRef = useRef(autoPlay);
  const playAttemptRef = useRef(0);
  const ignoreNextPauseRef = useRef(false);
  const restoredSourceRef = useRef("");
  const readyRef = useRef(false);
  const restoringRef = useRef(false);
  const nativeReportedSecondRef = useRef(-1);
  const [preferences, setPreferences] = useState<VideoPlayerPreferences>(() => (
    readVideoPlayerPreferences(session.installationId)
  ));
  const [playing, setPlaying] = useState(false);
  const [mediaLoading, setMediaLoading] = useState(true);
  const [hasStarted, setHasStarted] = useState(false);
  const [positionSeconds, setPositionSeconds] = useState(positionMs / 1_000);
  const [durationSeconds, setDurationSeconds] = useState<number | null>(
    durationMs === null ? null : durationMs / 1_000,
  );
  const [controlsVisible, setControlsVisible] = useState(true);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [subtitles, setSubtitles] = useState<NativeVideoSubtitle[]>([]);
  const [subtitleTrack, setSubtitleTrack] = useState<number | null>(null);
  const [subtitleError, setSubtitleError] = useState(false);
  const [playbackFailure, setPlaybackFailure] = useState<VideoPlaybackFailure | null>(null);
  const [playError, setPlayError] = useState("");
  const [retryVersion, setRetryVersion] = useState(0);
  const [bufferedSeconds, setBufferedSeconds] = useState(0);
  const currentIndex = Math.max(0, items.findIndex((item) => item.ordinal === currentOrdinal));
  const currentItem = items[currentIndex];
  const source = currentItem ? gateway.mediaAssetUrl(session.id, currentItem.ordinal) : "";
  const probe = useMediaAssetProbe(source, retryVersion);
  const loading = probe.loading || mediaLoading;
  const failure = probe.failure;
  const effectiveVolume = preferences.muted ? 0 : preferences.volume;

  const controlNativeVideo = useCallback(async (
    request: Omit<NativeVideoControlRequest, "sessionId">,
  ) => {
    if (!nativeGateway) return false;
    try {
      await nativeGateway.controlNativeVideo({ sessionId: session.id, ...request });
      return true;
    } catch (error) {
      setPlayError(error instanceof Error ? error.message : String(error));
      return false;
    }
  }, [nativeGateway, session.id]);

  const clearHideTimer = useCallback(() => {
    if (hideTimerRef.current !== null) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
  }, []);

  const revealControls = useCallback(() => {
    setControlsVisible(true);
    clearHideTimer();
    if (playing && !drawerOpen) {
      hideTimerRef.current = window.setTimeout(() => {
        hideTimerRef.current = null;
        if (videoControlsAreFocused()) return;
        setControlsVisible(false);
      }, CONTROLS_HIDE_DELAY_MS);
    }
  }, [clearHideTimer, drawerOpen, playing]);

  const reportProgress = useCallback((persisted = true) => {
    if (nativeGateway) return;
    const element = videoRef.current;
    if (!currentItem || !element || !readyRef.current) return;
    const duration = Number.isFinite(element.duration) && element.duration > 0
      ? element.duration
      : durationSeconds;
    const position = clampPlaybackPosition(element.currentTime, duration);
    setPositionSeconds(position);
    setDurationSeconds(duration);
    if (persisted) {
      onProgress(
        currentItem.ordinal,
        Math.round(position * 1_000),
        duration === null ? null : Math.round(duration * 1_000),
        element.paused ? "paused" : "active",
      );
    }
  }, [currentItem, durationSeconds, nativeGateway, onProgress]);

  const attemptPlay = useCallback(async () => {
    shouldPlayRef.current = true;
    if (nativeGateway) {
      if (probe.failure || playbackFailure) return false;
      setPlayError("");
      return controlNativeVideo({ command: "play" });
    }
    const element = videoRef.current;
    if (
      !element
      || probe.failure
      || !readyRef.current
      || element.readyState < HTMLMediaElement.HAVE_CURRENT_DATA
      || element.seeking
    ) return false;
    const attempt = playAttemptRef.current + 1;
    playAttemptRef.current = attempt;
    setPlayError("");
    try {
      if (element.ended) element.currentTime = 0;
      await element.play();
      if (!shouldPlayRef.current) {
        element.pause();
        return false;
      }
      return playAttemptRef.current === attempt;
    } catch (error) {
      if (playAttemptRef.current !== attempt) return false;
      setPlaying(false);
      const expectedInterruption = error instanceof DOMException
        && (error.name === "NotAllowedError" || error.name === "AbortError");
      if (expectedInterruption) {
        shouldPlayRef.current = false;
        setPlayError("");
        return false;
      }
      setPlayError(error instanceof Error ? error.message : String(error));
      return false;
    }
  }, [controlNativeVideo, nativeGateway, playbackFailure, probe.failure]);

  const finishPreparation = useCallback(() => {
    const element = videoRef.current;
    if (!element || readyRef.current) return;
    restoringRef.current = false;
    readyRef.current = true;
    setMediaLoading(false);
    if (shouldPlayRef.current) void attemptPlay();
  }, [attemptPlay]);

  const toggle = useCallback(() => {
    if (nativeGateway) {
      if (probe.failure || playbackFailure) return;
      if (playing) {
        playAttemptRef.current += 1;
        shouldPlayRef.current = false;
        setPlaying(false);
        void controlNativeVideo({ command: "pause" });
      } else {
        void attemptPlay();
      }
      return;
    }
    const element = videoRef.current;
    if (!element || probe.failure || playbackFailure) return;
    if (element.paused) {
      void attemptPlay();
      return;
    }
    playAttemptRef.current += 1;
    shouldPlayRef.current = false;
    setPlaying(false);
    element.pause();
  }, [attemptPlay, controlNativeVideo, nativeGateway, playbackFailure, playing, probe.failure]);

  const seek = useCallback((next: number, persistProgress = true) => {
    const element = videoRef.current;
    const bounded = clampPlaybackPosition(next, durationSeconds);
    setPositionSeconds(bounded);
    if (nativeGateway) void controlNativeVideo({ command: "seek", value: bounded });
    else if (element && readyRef.current) element.currentTime = bounded;
    if (persistProgress && currentItem) {
      onProgress(
        currentItem.ordinal,
        Math.round(bounded * 1_000),
        durationSeconds === null ? null : Math.round(durationSeconds * 1_000),
        playing ? "active" : "paused",
      );
    }
  }, [controlNativeVideo, currentItem, durationSeconds, nativeGateway, onProgress, playing]);

  const skip = useCallback((offsetSeconds: number) => {
    seek((nativeGateway ? positionSeconds : videoRef.current?.currentTime ?? positionSeconds) + offsetSeconds);
  }, [nativeGateway, positionSeconds, seek]);

  const setVolume = useCallback((next: number) => {
    const volume = clampVideoVolume(next);
    setPreferences((current) => ({ ...current, volume, muted: false }));
    if (nativeGateway) {
      void controlNativeVideo({ command: "volume", value: volume });
      void controlNativeVideo({ command: "muted", enabled: false });
    }
  }, [controlNativeVideo, nativeGateway]);

  const toggleMute = useCallback(() => {
    setPreferences((current) => {
      const muted = !current.muted;
      if (nativeGateway) void controlNativeVideo({ command: "muted", enabled: muted });
      return { ...current, muted };
    });
  }, [controlNativeVideo, nativeGateway]);

  const setFit = useCallback((fit: VideoFit) => {
    setPreferences((current) => ({ ...current, fit }));
    if (nativeGateway) void controlNativeVideo({ command: "fit", fit });
  }, [controlNativeVideo, nativeGateway]);

  const setSubtitle = useCallback((track: number | null) => {
    const selected = track === null
      ? null
      : subtitles.find((subtitle) => subtitle.index === track) ?? null;
    if (track !== null && !selected) return;
    setSubtitleTrack(selected?.index ?? null);
    setSubtitleError(false);
    setPreferences((current) => ({ ...current, subtitleLabel: selected?.label ?? null }));
    if (nativeGateway) {
      void controlNativeVideo({ command: "subtitle", subtitleTrack: selected?.index });
    }
  }, [controlNativeVideo, nativeGateway, subtitles]);

  const step = useCallback((direction: -1 | 1) => {
    const next = videoStepTarget(items, currentOrdinal, direction, repeatMode);
    if (next) onChoose(next.ordinal, true);
  }, [currentOrdinal, items, onChoose, repeatMode]);

  const selectOrdinal = useCallback((ordinal: number, requestedPosition = 0) => {
    if (ordinal === currentOrdinal) {
      seek(requestedPosition);
      return;
    }
    onChoose(ordinal, true);
  }, [currentOrdinal, onChoose, seek]);

  const toggleFullscreen = useCallback(async () => {
    if (nativeGateway) {
      const enabled = !fullscreen;
      if (await controlNativeVideo({ command: "fullscreen", enabled })) setFullscreen(enabled);
      return;
    }
    const root = rootRef.current;
    if (!root) return;
    const target = root.closest<HTMLElement>(".app-frame") ?? root;
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await target.requestFullscreen();
    } catch {
      return;
    }
  }, [controlNativeVideo, fullscreen, nativeGateway]);

  const finish = useCallback(() => {
    const element = videoRef.current;
    shouldPlayRef.current = false;
    if (nativeGateway) void controlNativeVideo({ command: "pause" });
    else {
      if (element && !element.paused) ignoreNextPauseRef.current = true;
      element?.pause();
    }
    setPlaying(false);
    setHasStarted(false);
    onComplete();
  }, [controlNativeVideo, nativeGateway, onComplete]);

  const retry = useCallback(() => {
    setPlaybackFailure(null);
    setPlayError("");
    readyRef.current = false;
    restoringRef.current = false;
    restoredSourceRef.current = "";
    setMediaLoading(true);
    setRetryVersion((current) => current + 1);
  }, []);

  useEffect(() => {
    writeVideoPlayerPreferences(session.installationId, preferences);
    if (nativeGateway) return;
    const element = videoRef.current;
    if (element) {
      element.volume = preferences.volume;
      element.muted = preferences.muted;
    }
  }, [nativeGateway, preferences, session.installationId]);

  useEffect(() => {
    shouldPlayRef.current = autoPlay;
    playAttemptRef.current += 1;
    restoredSourceRef.current = "";
    readyRef.current = false;
    restoringRef.current = false;
    nativeReportedSecondRef.current = -1;
    setPlaying(false);
    setHasStarted(false);
    setMediaLoading(source !== "");
    setPositionSeconds(positionMs / 1_000);
    setDurationSeconds(durationMs === null ? null : durationMs / 1_000);
    setPlaybackFailure(null);
    setPlayError("");
    setSubtitles([]);
    setSubtitleTrack(null);
    setSubtitleError(false);
  }, [source]);

  useEffect(() => {
    revealControls();
    return clearHideTimer;
  }, [clearHideTimer, revealControls]);

  useEffect(() => {
    if (nativeGateway) return;
    const onFullscreenChange = () => {
      const root = rootRef.current;
      const target = root?.closest<HTMLElement>(".app-frame") ?? root;
      setFullscreen(Boolean(target && document.fullscreenElement === target));
    };
    document.addEventListener("fullscreenchange", onFullscreenChange);
    return () => document.removeEventListener("fullscreenchange", onFullscreenChange);
  }, [nativeGateway]);

  useEffect(() => {
    if (!closing) return;
    shouldPlayRef.current = false;
    if (nativeGateway) {
      void controlNativeVideo({ command: "pause" });
      return;
    }
    const element = videoRef.current;
    if (element && !element.paused) ignoreNextPauseRef.current = true;
    element?.pause();
  }, [closing, controlNativeVideo, nativeGateway]);

  const videoHandlers = useMemo(() => ({
    videoPlayPause: () => toggle(),
    videoSkipBack: () => skip(-SKIP_SECONDS),
    videoSkipForward: () => skip(SKIP_SECONDS),
    videoVolumeUp: () => setVolume(effectiveVolume + 0.05),
    videoVolumeDown: () => setVolume(effectiveVolume - 0.05),
    videoMute: () => toggleMute(),
    videoSubtitles: subtitles.length ? () => {
      const index = subtitles.findIndex((subtitle) => subtitle.index === subtitleTrack);
      setSubtitle(index < 0 ? subtitles[0]?.index ?? null : subtitles[index + 1]?.index ?? null);
    } : undefined,
    videoFullscreen: () => void toggleFullscreen(),
    videoNext: () => step(1),
    videoPrevious: () => step(-1),
  }), [
    effectiveVolume, setSubtitle, setVolume, skip, step, subtitleTrack, subtitles,
    toggle, toggleFullscreen, toggleMute,
  ]);
  useBoundKeys("video", videoHandlers, { enabled: !drawerOpen });

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (drawerOpen) {
        if (event.key === "Escape") {
          event.preventDefault();
          setDrawerOpen(false);
        }
        return;
      }
      if (isInteractiveTarget(event.target)) return;
      revealControls();
      if (event.defaultPrevented) return;
      const key = event.key.toLocaleLowerCase();
      if (seekFractionForKey(key) !== null && durationSeconds) {
        event.preventDefault();
        seek(seekFractionForKey(key)! * durationSeconds);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [drawerOpen, durationSeconds, revealControls, seek]);

  const activeSession = useMemo<MediaSession>(() => ({
    ...session,
    repeatMode,
    shuffle,
    status: completed ? "completed" : playing ? "active" : "paused",
    progress: {
      ...session.progress,
      itemOrdinal: currentOrdinal,
      positionMs: Math.round(positionSeconds * 1_000),
      durationMs: durationSeconds === null ? null : Math.round(durationSeconds * 1_000),
      completed,
    },
  }), [completed, currentOrdinal, durationSeconds, playing, positionSeconds, repeatMode, session, shuffle]);

  const registration = useMemo<VideoPlaybackRegistration | null>(() => currentItem ? ({
    session: activeSession,
    item: currentItem,
    items,
    playing,
    positionSeconds,
    durationSeconds,
    volume: effectiveVolume,
    loading,
    error: probe.error || playError,
    failure,
    fit: preferences.fit,
    fullscreen,
    subtitles,
    subtitleTrack,
    subtitleError,
    toggle,
    seek,
    skip,
    selectOrdinal,
    step,
    setVolume,
    setRepeatMode: onRepeatMode,
    setShuffle: onShuffle,
    setFit,
    toggleFullscreen: () => void toggleFullscreen(),
    setSubtitle,
    setPlaybackRate: (rate: number) => {
      const element = videoRef.current;
      if (element) element.playbackRate = clampPlaybackRate(rate);
    },
    togglePictureInPicture: () => {
      const element = videoRef.current;
      if (!element || !document.pictureInPictureEnabled) return;
      if (document.pictureInPictureElement === element) {
        void document.exitPictureInPicture().catch(() => undefined);
      } else {
        void element.requestPictureInPicture().catch(() => undefined);
      }
    },
    bufferedSeconds,
    close: onBack,
  }) : null, [
    activeSession, bufferedSeconds, currentItem, durationSeconds, effectiveVolume, failure, fullscreen, items, loading, onBack,
    onRepeatMode, onShuffle, playError, playing, positionSeconds, probe.error, seek, selectOrdinal,
    preferences.fit, setFit, setSubtitle, setVolume, skip, step, subtitles, subtitleError, subtitleTrack,
    toggle, toggleFullscreen,
  ]);

  useEffect(() => {
    if (registration) publishVideoPlayback(registration);
  }, [publishVideoPlayback, registration]);

  useEffect(() => () => releaseVideoPlayback(session.id), [releaseVideoPlayback, session.id]);

  const handleLoadedMetadata = (event: SyntheticEvent<HTMLVideoElement>) => {
    const element = event.currentTarget;
    if (restoredSourceRef.current === source) return;
    restoredSourceRef.current = source;
    const duration = Number.isFinite(element.duration) && element.duration > 0 ? element.duration : null;
    const target = restorePlaybackPosition(positionMs / 1_000, duration, completed);
    setDurationSeconds(duration);
    setPositionSeconds(target);
    if (Math.abs(element.currentTime - target) <= 0.01) {
      finishPreparation();
      return;
    }
    restoringRef.current = true;
    element.currentTime = target;
    if (!element.seeking) finishPreparation();
  };

  const handleSeeked = () => {
    if (restoringRef.current) finishPreparation();
    else reportProgress();
  };

  const handlePlay = () => {
    if (!shouldPlayRef.current) {
      videoRef.current?.pause();
      return;
    }
    setPlaying(true);
    setHasStarted(true);
    setPlayError("");
    if (currentItem) onPlaybackState(currentItem.ordinal, "active");
  };

  const handlePause = () => {
    setPlaying(false);
    revealControls();
    if (ignoreNextPauseRef.current) {
      ignoreNextPauseRef.current = false;
      return;
    }
    if (currentItem && readyRef.current && !videoRef.current?.ended) {
      reportProgress();
      onPlaybackState(currentItem.ordinal, "paused");
    }
  };

  const handleEnded = () => {
    shouldPlayRef.current = false;
    setPlaying(false);
    setHasStarted(false);
    if (currentItem) onEnded(currentItem.ordinal);
  };

  const handleError = () => {
    const next = videoPlaybackFailure(videoRef.current?.error?.code);
    if (next) setPlaybackFailure(next);
    setMediaLoading(false);
    setPlaying(false);
    revealControls();
  };

  const handleNativeState = (state: NativeVideoState) => {
    setFullscreen(state.fullscreen);
    setSubtitleTrack(state.subtitleTrack);
    if (state.kind === "back") {
      onBack();
      return;
    }
    if (state.kind === "finish") {
      if (!completed) onComplete();
      return;
    }
    if (state.kind === "choose") {
      if (state.actionOrdinal !== null) onChoose(state.actionOrdinal, true);
      return;
    }
    if (state.kind === "subtitle") {
      setSubtitleError(false);
      return;
    }
    if (state.kind === "subtitle_error") {
      setSubtitleError(true);
      return;
    }
    if (state.kind === "interaction" || state.kind === "fullscreen") {
      revealControls();
      return;
    }
    const duration = state.durationSeconds && state.durationSeconds > 0
      ? state.durationSeconds
      : null;
    const position = clampPlaybackPosition(state.positionSeconds, duration);
    setPositionSeconds(position);
    if (duration !== null) setDurationSeconds(duration);
    if (state.kind === "loading" || state.kind === "waiting") {
      setMediaLoading(true);
      return;
    }
    if (state.kind === "metadata" || state.kind === "ready") {
      readyRef.current = state.kind === "ready" || state.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA;
      setMediaLoading(state.kind !== "ready");
      return;
    }
    if (state.kind === "playing") {
      readyRef.current = true;
      setMediaLoading(false);
      setPlaying(true);
      setHasStarted(true);
      setPlayError("");
      if (currentItem) onPlaybackState(currentItem.ordinal, "active");
      return;
    }
    if (state.kind === "paused") {
      setMediaLoading(false);
      setPlaying(false);
      revealControls();
      if (currentItem && readyRef.current && hasStarted && !state.ended) {
        onProgress(
          currentItem.ordinal,
          Math.round(position * 1_000),
          duration === null ? null : Math.round(duration * 1_000),
          "paused",
        );
        onPlaybackState(currentItem.ordinal, "paused");
      }
      return;
    }
    if (state.kind === "ended") {
      shouldPlayRef.current = false;
      setMediaLoading(false);
      setPlaying(false);
      setHasStarted(false);
      if (currentItem) onEnded(currentItem.ordinal);
      return;
    }
    if (state.kind === "error") {
      setPlaybackFailure(videoPlaybackFailure(state.errorCode) ?? "decode");
      setMediaLoading(false);
      setPlaying(false);
      revealControls();
      return;
    }
    if (state.kind === "time" && currentItem) {
      const wholeSecond = Math.floor(position);
      if (wholeSecond !== nativeReportedSecondRef.current) {
        nativeReportedSecondRef.current = wholeSecond;
        onProgress(
          currentItem.ordinal,
          Math.round(position * 1_000),
          duration === null ? null : Math.round(duration * 1_000),
          state.paused ? "paused" : "active",
        );
      }
    }
  };

  const nativeRequest = nativeGateway && currentItem ? {
    sessionId: session.id,
    ordinal: currentItem.ordinal,
    positionSeconds: restorePlaybackPosition(positionMs / 1_000, durationMs === null ? null : durationMs / 1_000, completed),
    volume: preferences.volume,
    muted: preferences.muted,
    fit: preferences.fit,
    autoPlay,
    posterUrl: posterUrls[0] ?? null,
    title: installationName,
    itemName: mediaItemName(currentItem),
    completed,
    playlist: items.map((item) => ({
      ordinal: item.ordinal,
      itemName: mediaItemName(item),
    })),
    subtitleLabel: preferences.subtitleLabel,
    labels: {
      back: t("common.back"),
      player: t("media.player.video"),
      play: t("media.play"),
      markFinished: t("media.markFinished"),
      completed: t("media.completed"),
      playlist: t("media.video.playlist"),
      openPlaylist: t("media.video.openPlaylist"),
      closePlaylist: t("media.video.closePlaylist"),
      nowPlaying: t("media.nowPlaying"),
      playbackFailed: t("media.video.playbackFailed"),
      codecUnsupported: t("media.video.codecUnsupported"),
      retry: t("media.video.retry"),
    },
  } : null;

  const errorMessage = probe.failure
    ? t(assetFailureMessageKey(probe.failure))
    : playbackFailure === "unsupported" || playbackFailure === "decode"
      ? t("media.video.codecUnsupported")
      : playbackFailure || playError
        ? t("media.video.playbackFailed")
        : "";

  return (
    <main
      className={`video-player-page is-fit-${preferences.fit}${nativeGateway ? " is-native-video" : ""}${controlsVisible || drawerOpen || !playing ? " is-controls-visible" : " is-controls-hidden"}${drawerOpen ? " is-drawer-open" : ""}${fullscreen ? " is-fullscreen" : ""}`}
      ref={rootRef}
      onMouseMove={revealControls}
      onPointerDown={revealControls}
      onFocusCapture={revealControls}
      onBlurCapture={revealControls}
    >
      {!nativeGateway ? <header className="video-player-topbar">
        <button className="video-player-back" type="button" disabled={closing} onClick={onBack}>
          {closing ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <ArrowLeft aria-hidden="true" />}
          <span>{t("common.back")}</span>
        </button>
        <div className="video-player-title">
          <span><Video aria-hidden="true" />{t("media.player.video")}</span>
          <strong>{installationName}</strong>
          <small>{currentItem ? mediaItemName(currentItem) : t("media.noItems")}</small>
        </div>
        <div className="video-player-position" aria-live="polite">
          <strong>{items.length > 0 ? currentIndex + 1 : 0}</strong><span>/ {items.length}</span>
        </div>
        <button className="video-player-finish" type="button" disabled={completed} onClick={finish}>
          <CheckCircle2 aria-hidden="true" /><span>{t(completed ? "media.completed" : "media.markFinished")}</span>
        </button>
        <button
          className="video-player-icon-button"
          type="button"
          aria-expanded={drawerOpen}
          aria-label={t(drawerOpen ? "media.video.closePlaylist" : "media.video.openPlaylist")}
          onClick={() => setDrawerOpen((open) => !open)}
        >
          <PanelRightOpen aria-hidden="true" />
        </button>
        <aside className="video-player-drawer" aria-hidden={!drawerOpen} inert={!drawerOpen}>
          <header>
            <div><Film aria-hidden="true" /><strong>{t("media.video.playlist")}</strong><span>{items.length}</span></div>
            <button type="button" aria-label={t("media.video.closePlaylist")} onClick={() => setDrawerOpen(false)}><ChevronRight aria-hidden="true" /></button>
          </header>
          <ol>
            {items.map((item, index) => (
              <li key={item.ordinal}>
                <button
                  className={item.ordinal === currentOrdinal ? "is-active" : undefined}
                  type="button"
                  aria-current={item.ordinal === currentOrdinal ? "true" : undefined}
                  onClick={() => {
                    onChoose(item.ordinal, true);
                    setDrawerOpen(false);
                  }}
                >
                  <span>{String(index + 1).padStart(2, "0")}</span>
                  <Film aria-hidden="true" />
                  <strong>{mediaItemName(item)}</strong>
                  {item.ordinal === currentOrdinal && playing ? <i aria-label={t("media.nowPlaying")}><Pause aria-hidden="true" /></i> : null}
                </button>
              </li>
            ))}
          </ol>
        </aside>
      </header> : null}

      <div className="video-player-workspace">
        <div
          className="video-player-viewport"
          onDoubleClick={(event) => {
            if (isInteractiveTarget(event.target)) return;
            void toggleFullscreen();
          }}
        >
        {nativeGateway && nativeRequest && !errorMessage ? (
          <NativeVideoSurface
            key={`${source}:${retryVersion}`}
            gateway={nativeGateway}
            request={nativeRequest}
            onState={handleNativeState}
            onOpen={(response) => {
              setSubtitles(response.subtitles);
              setSubtitleError(false);
              setSubtitleTrack(
                response.subtitles.find((subtitle) => (
                  subtitle.label.toLocaleLowerCase()
                  === preferences.subtitleLabel?.toLocaleLowerCase()
                ))?.index ?? null,
              );
            }}
            onError={(error) => {
              setMediaLoading(false);
              setPlayError(error instanceof Error ? error.message : String(error));
            }}
          />
        ) : null}
        {!nativeGateway && currentItem ? (
          <video
            key={`${source}:${retryVersion}`}
            ref={videoRef}
            className="video-player-element"
            src={source}
            preload="metadata"
            playsInline
            loop={repeatMode === "one" || (repeatMode === "all" && items.length === 1)}
            onLoadedMetadata={handleLoadedMetadata}
            onCanPlay={() => {
              setMediaLoading(false);
              if (!readyRef.current && !restoringRef.current) finishPreparation();
              else if (shouldPlayRef.current) void attemptPlay();
            }}
            onPlaying={() => setMediaLoading(false)}
            onWaiting={() => setMediaLoading(true)}
            onSeeked={handleSeeked}
            onPlay={handlePlay}
            onPause={handlePause}
            onTimeUpdate={() => reportProgress()}
            onProgress={(event) => {
              const element = event.currentTarget;
              const ranges = Array.from(
                { length: element.buffered.length },
                (_, index) => ({
                  start: element.buffered.start(index),
                  end: element.buffered.end(index),
                }),
              );
              setBufferedSeconds(bufferedAhead(ranges, element.currentTime));
            }}
            onEnded={handleEnded}
            onError={handleError}
          />
        ) : null}

        {!nativeGateway && !hasStarted && !errorMessage ? (
          <VideoPoster
            urls={posterUrls}
            title={installationName}
            itemName={currentItem ? mediaItemName(currentItem) : t("media.noItems")}
          />
        ) : null}
        {!nativeGateway ? <span className="video-player-vignette" aria-hidden="true" /> : null}

        {errorMessage ? (
          <div className="video-player-error" role="alert">
            <AlertTriangle aria-hidden="true" />
            <strong>{errorMessage}</strong>
            <span>{currentItem ? mediaItemName(currentItem) : t("media.noItems")}</span>
            <button type="button" onClick={retry}><RotateCw aria-hidden="true" />{t("media.video.retry")}</button>
          </div>
        ) : null}

        {saveError ? (
          <div className="video-player-save-error" role="alert">
            <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: saveError })}
          </div>
        ) : null}
        </div>

      </div>
    </main>
  );
}

function VideoPoster({ urls, title, itemName }: { urls: string[]; title: string; itemName: string }) {
  const [candidate, setCandidate] = useState(0);

  useEffect(() => setCandidate(0), [urls]);

  const source = urls[candidate];
  return (
    <div className="video-player-poster" aria-hidden="true">
      {source ? (
        <img
          src={source}
          alt=""
          decoding="async"
          referrerPolicy="no-referrer"
          onError={() => setCandidate((current) => current + 1)}
        />
      ) : (
        <span><Film /><strong>{title}</strong><small>{itemName}</small></span>
      )}
    </div>
  );
}

function isInteractiveTarget(target: EventTarget | null): boolean {
  return target instanceof Element && Boolean(target.closest("button, input, select, textarea, a"));
}

function videoControlsAreFocused(): boolean {
  const active = document.activeElement;
  return active instanceof Element && Boolean(active.closest(
    ".video-player-topbar, .video-player-drawer",
  ));
}
