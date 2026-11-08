import {
  AlertTriangle,
  ChevronRight,
  ExternalLink,
  Film,
  PanelRightOpen,
  Pause,
  Play,
  RotateCw,
  Video,
  X,
} from "lucide-react";
import {
  type CSSProperties,
  type SyntheticEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { errorMessage } from "../../app/errorMessage";
import { useImageSourceChain } from "../../app/useImageSourceChain";
import { BackButton } from "../../app/BackButton";
import { useAmbientFill } from "./useAmbientFill";
import { usePresentation } from "../../preferences/PresentationProvider";
import { useBoundKeys } from "../../preferences/KeyBindingsProvider";
import { isInteractiveTarget } from "../../preferences/keyBindings";
import {
  clampPlaybackPosition,
  clampPlaybackRate,
  restorePlaybackPosition,
  VOLUME_WHEEL_STEP,
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
  NativeVideoRectangle,
  NativeVideoState,
  NativeVideoSubtitle,
} from "./types";
import {
  bufferedAhead,
  clampVideoVolume,
  readSubtitleStyle,
  readVideoPlayerPreferences,
  videoPlaybackFailure,
  videoStepTarget,
  writeSubtitleStyle,
  writeVideoPlayerPreferences,
  type SubtitleStyle,
  type VideoFit,
  type VideoPlaybackFailure,
  type VideoPlayerPreferences,
  seekFractionForKey,
} from "./videoPlayerModel";

const CONTROLS_HIDE_DELAY_MS = 2_600;
const SKIP_SECONDS = 15;
/** How long the play or pause badge stays, as YouTube's. */
const BADGE_MS = 500;

const CUE_COLORS: Record<SubtitleStyle["color"], string> = {
  white: "255 255 255",
  yellow: "255 228 77",
  cyan: "127 233 255",
};

/** The cue variables of the page's own tracks, as the surface document sets them. */
function cueStyle(style: SubtitleStyle): CSSProperties {
  return {
    "--cue-size": `calc(clamp(16px, 2.4vw, 34px) * ${style.size})`,
    "--cue-color": `rgb(${CUE_COLORS[style.color] ?? CUE_COLORS.white} / ${style.opacity})`,
    "--cue-background": `rgba(0, 0, 0, ${style.background})`,
  } as CSSProperties;
}
const NATIVE_SEEK_TOLERANCE_SECONDS = 0.75;
const NATIVE_SEEK_SETTLE_MS = 4_000;

interface PendingNativeSeek {
  target: number;
  at: number;
  preview: boolean;
}

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
  onRepeatMode,
  onShuffle,
  onBack,
}: VideoPlayerProps) {
  const { t } = usePresentation();
  const playback = useMediaPlayback();
  const publishVideoPlayback = playback.publishVideoPlayback;
  const releaseVideoPlayback = playback.releaseVideoPlayback;
  const nativeGateway = useMemo(() => resolveNativeVideoGateway(gateway), [gateway]);
  // The page draws the bar unless the native surface draws its own.
  const pageChrome = !nativeGateway || gateway.nativeVideoChrome === "page";
  const rootRef = useRef<HTMLElement>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const exitFullscreenRef = useRef<HTMLButtonElement>(null);
  // The play or pause badge a click on the picture shows, as YouTube does; a
  // native picture draws its own (the surface document, the GStreamer frame).
  const [badge, setBadge] = useState<{ kind: "play" | "pause"; nonce: number } | null>(null);
  useEffect(() => {
    if (!badge) return undefined;
    const timer = window.setTimeout(() => setBadge(null), BADGE_MS + 100);
    return () => window.clearTimeout(timer);
  }, [badge]);
  const videoRef = useRef<HTMLVideoElement>(null);
  const ambientRef = useRef<HTMLCanvasElement>(null);
  const hideTimerRef = useRef<number | null>(null);
  const shouldPlayRef = useRef(autoPlay);
  const playAttemptRef = useRef(0);
  const ignoreNextPauseRef = useRef(false);
  const restoredSourceRef = useRef("");
  const readyRef = useRef(false);
  const restoringRef = useRef(false);
  const nativeReportedSecondRef = useRef(-1);
  const nativeSeekRef = useRef<PendingNativeSeek | null>(null);
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
  const leaveFullscreenRef = useRef<() => void>(() => undefined);
  leaveFullscreenRef.current = () => {
    if (!fullscreen) return;
    if (nativeGateway) {
      void nativeGateway.controlNativeVideo({ sessionId: session.id, command: "fullscreen", enabled: false }).catch(() => undefined);
    } else if (document.fullscreenElement) {
      void document.exitFullscreen().catch(() => undefined);
    }
  };
  // Leaving the player gives the window back; nothing else of the app is fullscreen.
  useEffect(() => () => leaveFullscreenRef.current(), []);
  const [subtitles, setSubtitles] = useState<NativeVideoSubtitle[]>([]);
  const [subtitleTrack, setSubtitleTrack] = useState<number | null>(null);
  const [subtitleError, setSubtitleError] = useState(false);
  const [subtitleStyle, setSubtitleStyleState] = useState<SubtitleStyle>(() => readSubtitleStyle());
  const [playbackFailure, setPlaybackFailure] = useState<VideoPlaybackFailure | null>(null);
  const [playError, setPlayError] = useState("");
  const [externalError, setExternalError] = useState("");
  const [retryVersion, setRetryVersion] = useState(0);
  const [bufferedSeconds, setBufferedSeconds] = useState(0);
  const currentIndex = Math.max(0, items.findIndex((item) => item.ordinal === currentOrdinal));
  const currentItem = items[currentIndex];
  const source = currentItem ? gateway.mediaAssetUrl(session.id, currentItem.ordinal) : "";
  useAmbientFill(videoRef, ambientRef, nativeGateway || !preferences.ambient ? "" : `${source}:${retryVersion}`);
  const probe = useMediaAssetProbe(source, retryVersion);
  // The page's own player lists the sidecar subtitles itself; a native picture
  // reports them as it opens. The remembered label picks the first track.
  const subtitleLabelRef = useRef(preferences.subtitleLabel);
  subtitleLabelRef.current = preferences.subtitleLabel;
  const currentOrdinalForSubtitles = currentItem?.ordinal ?? null;
  useEffect(() => {
    if (nativeGateway || currentOrdinalForSubtitles === null || !gateway.readVideoSubtitles) return undefined;
    let disposed = false;
    setSubtitles([]);
    setSubtitleTrack(null);
    setSubtitleError(false);
    gateway.readVideoSubtitles(session.id, currentOrdinalForSubtitles).then((found) => {
      if (disposed) return;
      setSubtitles(found);
      const wanted = subtitleLabelRef.current?.toLocaleLowerCase();
      setSubtitleTrack(found.find((subtitle) => subtitle.label.toLocaleLowerCase() === wanted)?.index ?? null);
    }).catch(() => {
      if (!disposed) setSubtitleError(true);
    });
    return () => {
      disposed = true;
    };
  }, [currentOrdinalForSubtitles, gateway, nativeGateway, session.id]);
  useEffect(() => {
    const element = videoRef.current;
    if (nativeGateway || !element) return;
    const tracks = element.textTracks;
    for (let index = 0; index < tracks.length; index += 1) {
      const track = tracks[index];
      const own = subtitles[index];
      if (track) track.mode = own && own.index === subtitleTrack ? "showing" : "disabled";
    }
  }, [nativeGateway, retryVersion, source, subtitles, subtitleTrack]);
  const loading = probe.loading || mediaLoading;
  const controlsShowing = controlsVisible || drawerOpen || !playing;
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
      setPlayError(errorMessage(error));
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

  const attemptPlay = useCallback(async (fromPicture = false) => {
    shouldPlayRef.current = true;
    if (nativeGateway) {
      if (probe.failure || playbackFailure) return false;
      setPlayError("");
      return controlNativeVideo(fromPicture ? { command: "play", badge: true } : { command: "play" });
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
      setPlayError(errorMessage(error));
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

  const toggle = useCallback((fromPicture = false) => {
    if (nativeGateway) {
      if (probe.failure || playbackFailure) return;
      if (playing) {
        playAttemptRef.current += 1;
        shouldPlayRef.current = false;
        setPlaying(false);
        void controlNativeVideo(fromPicture ? { command: "pause", badge: true } : { command: "pause" });
      } else {
        void attemptPlay(fromPicture);
      }
      return;
    }
    const element = videoRef.current;
    if (!element || probe.failure || playbackFailure) return;
    if (fromPicture) setBadge({ kind: element.paused ? "play" : "pause", nonce: Date.now() });
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
    if (nativeGateway) {
      nativeSeekRef.current = { target: bounded, at: performance.now(), preview: !persistProgress };
      void controlNativeVideo({ command: "seek", value: bounded, ...(persistProgress ? {} : { preview: true }) });
    } else if (element && readyRef.current) element.currentTime = bounded;
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
  const volumeRef = useRef(preferences.volume);
  volumeRef.current = preferences.volume;

  // The wheel over the picture turns the volume, as desktop players do. A
  // listener of its own, since React's wheel handlers cannot stop the page
  // from scrolling.
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const onWheel = (event: WheelEvent) => {
      if (isInteractiveTarget(event.target) || event.deltaY === 0) return;
      event.preventDefault();
      setVolume(volumeRef.current + (event.deltaY < 0 ? VOLUME_WHEEL_STEP : -VOLUME_WHEEL_STEP));
      revealControls();
    };
    viewport.addEventListener("wheel", onWheel, { passive: false });
    return () => viewport.removeEventListener("wheel", onWheel);
  }, [revealControls, setVolume]);

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

  const setAmbient = useCallback((ambient: boolean) => {
    setPreferences((current) => ({ ...current, ambient }));
    if (nativeGateway) void controlNativeVideo({ command: "ambient", enabled: ambient });
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

  const setSubtitleStyle = useCallback((style: SubtitleStyle) => {
    setSubtitleStyleState(style);
    writeSubtitleStyle(style);
    if (nativeGateway) void controlNativeVideo({ command: "subtitle_style", subtitleStyle: style });
  }, [controlNativeVideo, nativeGateway]);

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
      if (event.key === "Escape" && fullscreen) {
        event.preventDefault();
        void toggleFullscreen();
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
  }, [drawerOpen, durationSeconds, fullscreen, revealControls, seek, toggleFullscreen]);

  // In fullscreen the dock lies over the picture outside this page, so the
  // whole document's pointer brings the controls back.
  useEffect(() => {
    if (!fullscreen) return;
    document.addEventListener("mousemove", revealControls);
    document.addEventListener("pointerdown", revealControls);
    return () => {
      document.removeEventListener("mousemove", revealControls);
      document.removeEventListener("pointerdown", revealControls);
    };
  }, [fullscreen, revealControls]);

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

  // The dock calls the player through the registration. Its actions read the
  // latest callbacks from this ref, so the new functions the page passes on
  // every render (and it renders on every published registration) do not
  // publish a new registration and loop.
  const actionsRef = useRef({
    toggle, seek, skip, selectOrdinal, step, setVolume, setFit, setAmbient, toggleFullscreen, setSubtitle, setSubtitleStyle,
    controlNativeVideo, nativeGateway, onRepeatMode, onShuffle, onBack,
  });
  useEffect(() => {
    actionsRef.current = {
      toggle, seek, skip, selectOrdinal, step, setVolume, setFit, setAmbient, toggleFullscreen, setSubtitle, setSubtitleStyle,
      controlNativeVideo, nativeGateway, onRepeatMode, onShuffle, onBack,
    };
  });

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
    ambient: preferences.ambient,
    fullscreen,
    subtitles,
    subtitleTrack,
    subtitleError,
    subtitleStyle,
    controlsVisible: controlsShowing,
    toggle: () => actionsRef.current.toggle(),
    seek: (seconds: number, persistProgress?: boolean) => actionsRef.current.seek(seconds, persistProgress),
    skip: (offsetSeconds: number) => actionsRef.current.skip(offsetSeconds),
    selectOrdinal: (ordinal: number, positionSeconds?: number) => actionsRef.current.selectOrdinal(ordinal, positionSeconds),
    step: (direction: -1 | 1) => actionsRef.current.step(direction),
    setVolume: (volume: number) => actionsRef.current.setVolume(volume),
    setRepeatMode: (mode: MediaRepeatMode) => actionsRef.current.onRepeatMode(mode),
    setShuffle: (next: boolean) => actionsRef.current.onShuffle(next),
    setFit: (fit: VideoFit) => actionsRef.current.setFit(fit),
    setAmbient: (enabled: boolean) => actionsRef.current.setAmbient(enabled),
    toggleFullscreen: () => void actionsRef.current.toggleFullscreen(),
    setSubtitle: (track: number | null) => actionsRef.current.setSubtitle(track),
    setSubtitleStyle: (style: SubtitleStyle) => actionsRef.current.setSubtitleStyle(style),
    setPlaybackRate: (rate: number) => {
      const { controlNativeVideo: control, nativeGateway: native } = actionsRef.current;
      if (native) {
        void control({ command: "rate", value: clampPlaybackRate(rate) });
        return;
      }
      const element = videoRef.current;
      if (element) element.playbackRate = clampPlaybackRate(rate);
    },
    canPictureInPicture: !nativeGateway && document.pictureInPictureEnabled === true,
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
    close: () => actionsRef.current.onBack(),
  }) : null, [
    activeSession, bufferedSeconds, controlsShowing, currentItem, durationSeconds, effectiveVolume, failure, fullscreen, items, loading,
    nativeGateway, playError, playing, positionSeconds, probe.error, preferences.ambient, preferences.fit, subtitles, subtitleError,
    subtitleStyle, subtitleTrack,
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
    if (state.kind === "volume") {
      if (typeof state.volume === "number") {
        const volume = clampVideoVolume(state.volume);
        setPreferences((current) => ({ ...current, volume, muted: false }));
      }
      return;
    }
    if (state.kind === "interaction" || state.kind === "fullscreen") {
      revealControls();
      return;
    }
    const duration = state.durationSeconds && state.durationSeconds > 0
      ? state.durationSeconds
      : null;
    const settled = settleNativeSeek(
      nativeSeekRef.current,
      clampPlaybackPosition(state.positionSeconds, duration),
      performance.now(),
    );
    nativeSeekRef.current = settled.pending;
    const position = settled.position;
    setPositionSeconds(position);
    if (duration !== null) setDurationSeconds(duration);
    if (state.kind === "loading" || state.kind === "waiting") {
      setMediaLoading(true);
      return;
    }
    if (state.kind === "metadata" || state.kind === "ready") {
      // A new surface starts at normal speed; carry the chosen speed over.
      if (state.kind === "ready" && playback.playbackRate !== 1) {
        void controlNativeVideo({ command: "rate", value: playback.playbackRate });
      }
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
    subtitleStyle,
    ambient: preferences.ambient,
    labels: {
      back: t("common.back"),
      player: t("media.player.video"),
      play: t("media.play"),
      playlist: t("media.video.playlist"),
      openPlaylist: t("media.video.openPlaylist"),
      closePlaylist: t("media.video.closePlaylist"),
      nowPlaying: t("media.nowPlaying"),
      playbackFailed: t("media.video.playbackFailed"),
      codecUnsupported: t("media.video.codecUnsupported"),
      exitFullscreen: t("media.video.exitFullscreen"),
      retry: t("media.video.retry"),
    },
  } : null;

  // The native surface cannot be drawn over, so it is told which controls lie
  // over it in fullscreen and shows them through.
  const measureObstructions = (): NativeVideoRectangle[] => {
    if (!fullscreen || !controlsShowing) return [];
    // The dock's menus open beside it, over the picture as well.
    const dock = document.querySelector(".media-dock");
    const menus = Array.from(document.querySelectorAll(".media-dock-menu-panel"));
    return [exitFullscreenRef.current, dock, ...menus]
      .filter((element): element is Element => element !== null)
      .map((element) => ({
        bounds: element.getBoundingClientRect(),
        radius: Number.parseFloat(getComputedStyle(element).borderTopLeftRadius) || 0,
      }))
      .filter(({ bounds }) => bounds.width > 0 && bounds.height > 0)
      .map(({ bounds, radius }) => ({
        x: bounds.left,
        y: bounds.top,
        width: bounds.width,
        height: bounds.height,
        radius: Math.min(radius, bounds.width / 2, bounds.height / 2),
      }));
  };

  const failureText = probe.failure
    ? t(assetFailureMessageKey(probe.failure))
    : playbackFailure === "unsupported" || playbackFailure === "decode"
      ? t("media.video.codecUnsupported")
      : playbackFailure || playError
        ? t("media.video.playbackFailed")
        : "";

  return (
    <main
      className={`video-player-page is-fit-${preferences.fit}${nativeGateway ? " is-native-video" : ""}${nativeGateway && pageChrome ? " is-page-chrome" : ""}${controlsShowing ? " is-controls-visible" : " is-controls-hidden"}${drawerOpen ? " is-drawer-open" : ""}${fullscreen ? " is-fullscreen" : ""}`}
      ref={rootRef}
      onMouseMove={revealControls}
      onPointerDown={revealControls}
      onFocusCapture={revealControls}
      onBlurCapture={revealControls}
    >
      {fullscreen && pageChrome ? (
        <button
          ref={exitFullscreenRef}
          className="video-player-fullscreen-exit"
          type="button"
          title={t("media.video.exitFullscreen")}
          aria-label={t("media.video.exitFullscreen")}
          onClick={() => void toggleFullscreen()}
        >
          <X aria-hidden="true" />
        </button>
      ) : null}
      {pageChrome && !fullscreen ? <header className="video-player-topbar">
        <BackButton closing={closing} onClick={onBack} />
        <div className="video-player-title">
          <span><Video aria-hidden="true" />{t("media.player.video")}</span>
          <strong>{installationName}</strong>
          <small>{currentItem ? mediaItemName(currentItem) : t("media.noItems")}</small>
        </div>
        <div className="video-player-position" aria-live="polite">
          <strong>{items.length > 0 ? currentIndex + 1 : 0}</strong><span>/ {items.length}</span>
        </div>
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
          ref={viewportRef}
          className="video-player-viewport"
          onClick={(event) => {
            if (isInteractiveTarget(event.target)) return;
            toggle(true);
          }}
          onDoubleClick={(event) => {
            if (isInteractiveTarget(event.target)) return;
            void toggleFullscreen();
          }}
        >
          {nativeGateway && nativeRequest && !failureText ? (
            <NativeVideoSurface
              key={`${source}:${retryVersion}`}
              gateway={nativeGateway}
              request={nativeRequest}
              onState={handleNativeState}
              obstructions={measureObstructions}
              overlays={`${fullscreen}:${controlsShowing}`}
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
                setPlayError(errorMessage(error));
              }}
            />
          ) : null}
          {badge ? (
            <div key={badge.nonce} className="video-player-badge" aria-hidden="true">
              {badge.kind === "pause" ? <Pause fill="currentColor" /> : <Play fill="currentColor" />}
            </div>
          ) : null}
          {!nativeGateway && currentItem && preferences.ambient ? (
            <canvas className="video-player-ambient" ref={ambientRef} aria-hidden="true" />
          ) : null}
          {!nativeGateway && currentItem ? (
            <video
              key={`${source}:${retryVersion}`}
              ref={videoRef}
              className="video-player-element"
              src={source}
              crossOrigin="anonymous"
              style={cueStyle(subtitleStyle)}
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
            >
              {subtitles.map((subtitle) => (
                <track
                  key={subtitle.index}
                  kind="subtitles"
                  label={subtitle.label}
                  srcLang={subtitle.language ?? undefined}
                  src={gateway.videoSubtitleUrl?.(session.id, currentItem.ordinal, subtitle.index) ?? ""}
                  onError={() => setSubtitleError(true)}
                />
              ))}
            </video>
          ) : null}

        {!nativeGateway && !hasStarted && !failureText ? (
          <VideoPoster
            urls={posterUrls}
            title={installationName}
            itemName={currentItem ? mediaItemName(currentItem) : t("media.noItems")}
          />
        ) : null}
        {failureText ? (
          <div className="video-player-error" role="alert">
            <AlertTriangle aria-hidden="true" />
            <strong>{failureText}</strong>
            <span>{currentItem ? mediaItemName(currentItem) : t("media.noItems")}</span>
            <button type="button" onClick={retry}><RotateCw aria-hidden="true" />{t("media.video.retry")}</button>
            {gateway.openVideoExternally && currentItem && !probe.failure ? (
              <button
                type="button"
                onClick={() => {
                  setExternalError("");
                  void gateway.openVideoExternally?.(session.id, currentItem.ordinal).catch((error: unknown) => {
                    setExternalError(errorMessage(error));
                  });
                }}
              >
                <ExternalLink aria-hidden="true" />{t("media.video.openExternally")}
              </button>
            ) : null}
            {externalError ? <small>{t("common.requestFailed", { error: externalError })}</small> : null}
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
  const { source, advance } = useImageSourceChain(urls);
  return (
    <div className="video-player-poster" aria-hidden="true">
      {source ? (
        <img
          src={source}
          alt=""
          decoding="async"
          referrerPolicy="no-referrer"
          onError={advance}
        />
      ) : (
        <span><Film /><strong>{title}</strong><small>{itemName}</small></span>
      )}
    </div>
  );
}

/**
 * The position to show while the native player works through a seek. Its
 * reports lag a flushing seek, can fail into 0 during the flush, and a
 * scrub's preview seeks land on keyframes, so the page keeps the chosen
 * position until an accurate seek is reported near it or the wait runs out.
 */
export function settleNativeSeek(
  pending: PendingNativeSeek | null,
  reported: number,
  now: number,
): { position: number; pending: PendingNativeSeek | null } {
  if (!pending) return { position: reported, pending: null };
  const settled = now - pending.at >= NATIVE_SEEK_SETTLE_MS
    || (!pending.preview && Math.abs(reported - pending.target) <= NATIVE_SEEK_TOLERANCE_SECONDS);
  return settled ? { position: reported, pending: null } : { position: pending.target, pending };
}

function videoControlsAreFocused(): boolean {
  const active = document.activeElement;
  return active instanceof Element && Boolean(active.closest(
    ".video-player-topbar, .video-player-drawer",
  ));
}
