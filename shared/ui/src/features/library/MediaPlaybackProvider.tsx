import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useQueryClient } from "@tanstack/react-query";

import {
  clampPlaybackPosition,
  clampPlaybackRate,
  restorePlaybackPosition,
  resumePlaybackPosition,
} from "./audioPlayer";
import { useMaterializedMediaSource, type MediaAssetFailure } from "./mediaAsset";
import { useBoundKeys } from "../../preferences/KeyBindingsProvider";
import { orderedSessionItems } from "./mediaSession";
import type { VideoFit } from "./videoPlayerModel";
import type {
  LibraryGateway,
  MediaRepeatMode,
  MediaSession,
  MediaSessionItem,
  NativeVideoSubtitle,
} from "./types";

export type MediaPlaybackGateway = Pick<
  LibraryGateway,
  | "openMediaSession"
  | "openPersonalizedVoiceQueue"
  | "closeMediaSession"
  | "updateMediaProgress"
  | "updateMediaQueueSettings"
  | "mediaAssetUrl"
>;

export interface VideoPlaybackRegistration {
  session: MediaSession;
  item: MediaSessionItem;
  items: MediaSessionItem[];
  playing: boolean;
  positionSeconds: number;
  durationSeconds: number | null;
  volume: number;
  loading: boolean;
  error: string;
  failure: MediaAssetFailure | null;
  fit: VideoFit;
  fullscreen: boolean;
  subtitles: NativeVideoSubtitle[];
  subtitleTrack: number | null;
  subtitleError: boolean;
  toggle: () => void;
  seek: (positionSeconds: number, persistProgress?: boolean) => void;
  skip: (offsetSeconds: number) => void;
  selectOrdinal: (ordinal: number, positionSeconds?: number) => void;
  step: (direction: -1 | 1) => void;
  setVolume: (volume: number) => void;
  setRepeatMode: (mode: MediaRepeatMode) => void;
  setShuffle: (shuffle: boolean) => void;
  setFit: (fit: VideoFit) => void;
  toggleFullscreen: () => void;
  setSubtitle: (track: number | null) => void;
  setPlaybackRate: (rate: number) => void;
  togglePictureInPicture: () => void;
  bufferedSeconds: number;
  close: () => void;
}

export interface VideoDisplayControls {
  fit: VideoFit;
  fullscreen: boolean;
  subtitles: NativeVideoSubtitle[];
  subtitleTrack: number | null;
  subtitleError: boolean;
  setFit: (fit: VideoFit) => void;
  toggleFullscreen: () => void;
  setSubtitle: (track: number | null) => void;
  togglePictureInPicture: () => void;
}

interface MediaPlaybackValue {
  session: MediaSession | null;
  item: MediaSessionItem | null;
  items: MediaSessionItem[];
  playing: boolean;
  positionSeconds: number;
  durationSeconds: number | null;
  volume: number;
  loading: boolean;
  error: string;
  failure: MediaAssetFailure | null;
  analyser: AnalyserNode | null;
  videoDisplay: VideoDisplayControls | null;
  playbackRate: number;
  setPlaybackRate: (rate: number) => void;
  bufferedSeconds: number;
  openWork: (installationId: string) => Promise<MediaSession | null>;
  openVoiceQueue: () => Promise<MediaSession | null>;
  toggle: () => void;
  seek: (positionSeconds: number, persistProgress?: boolean) => void;
  skip: (offsetSeconds: number) => void;
  selectOrdinal: (ordinal: number, positionSeconds?: number) => void;
  step: (direction: -1 | 1) => void;
  setVolume: (volume: number) => void;
  setRepeatMode: (mode: MediaRepeatMode) => void;
  setShuffle: (shuffle: boolean) => void;
  close: () => void;
  publishVideoPlayback: (playback: VideoPlaybackRegistration) => void;
  releaseVideoPlayback: (sessionId: string) => void;
}

const MediaPlaybackContext = createContext<MediaPlaybackValue | null>(null);

interface PendingPlaybackRestore {
  sessionId: string;
  itemOrdinal: number;
  positionSeconds: number;
  completed: boolean;
}

export function MediaPlaybackProvider({
  gateway,
  children,
}: {
  gateway: MediaPlaybackGateway;
  children: ReactNode;
}) {
  const queryClient = useQueryClient();
  const audioRef = useRef<HTMLAudioElement>(null);
  const reportedSecondRef = useRef(-1);
  const shouldPlayRef = useRef(false);
  const playAttemptRef = useRef(0);
  const resumePlaybackRef = useRef(false);
  const pendingRestoreRef = useRef<PendingPlaybackRestore | null>(null);
  const preparedSourceRef = useRef("");
  const progressWriteRef = useRef<Promise<void>>(Promise.resolve());
  const progressRequestRef = useRef(0);
  const sessionRef = useRef<MediaSession | null>(null);
  const videoPlaybackRef = useRef<VideoPlaybackRegistration | null>(null);
  const [session, setSession] = useState<MediaSession | null>(null);
  const [videoPlayback, setVideoPlayback] = useState<VideoPlaybackRegistration | null>(null);
  const [ordinal, setOrdinal] = useState<number | null>(null);
  const [prepareRequest, setPrepareRequest] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [positionSeconds, setPositionSeconds] = useState(0);
  const [durationSeconds, setDurationSeconds] = useState<number | null>(null);
  const [volume, setStoredVolume] = useState(1);
  const [playbackRate, setStoredPlaybackRate] = useState(1);
  const [error, setError] = useState("");
  const audioContextRef = useRef<AudioContext | null>(null);
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);

  const ensureAnalyser = useCallback(async () => {
    const element = audioRef.current;
    if (!element) return false;
    try {
      let context = audioContextRef.current;
      if (!context) {
        context = new AudioContext();
        const node = context.createAnalyser();
        node.fftSize = 1024;
        node.smoothingTimeConstant = 0.55;
        context.createMediaElementSource(element).connect(node);
        node.connect(context.destination);
        audioContextRef.current = context;
        setAnalyser(node);
      }
      if (context.state !== "running") await context.resume();
      return context.state === "running";
    } catch {
      setAnalyser(null);
      return false;
    }
  }, []);

  const items = useMemo(
    () => (session ? orderedSessionItems(session) : []),
    [session],
  );
  const item = items.find((candidate) => candidate.ordinal === ordinal) ?? null;
  const source = session && item ? gateway.mediaAssetUrl(session.id, item.ordinal) : "";
  const playable = useMaterializedMediaSource(source);

  const playCurrent = useCallback(async () => {
    const element = audioRef.current;
    shouldPlayRef.current = true;
    if (
      !element
      || !playable.url
      || preparedSourceRef.current !== playable.url
      || element.readyState < HTMLMediaElement.HAVE_CURRENT_DATA
      || element.seeking
    ) {
      return false;
    }
    const attempt = playAttemptRef.current + 1;
    playAttemptRef.current = attempt;
    const resumePlayback = resumePlaybackRef.current;
    resumePlaybackRef.current = false;
    setError("");
    try {
      const analyserReady = ensureAnalyser();
      if (resumePlayback) {
        element.currentTime = resumePlaybackPosition(
          element.currentTime,
          Number.isFinite(element.duration) && element.duration > 0 ? element.duration : null,
          element.ended,
        );
      }
      const playbackStarted = element.play();
      await Promise.all([analyserReady, playbackStarted]);
      if (!shouldPlayRef.current) {
        element.pause();
        return false;
      }
      if (playAttemptRef.current !== attempt) return false;
      setPlaying(true);
      return true;
    } catch (cause) {
      setPlaying(false);
      setError(cause instanceof Error ? cause.message : String(cause));
      return false;
    }
  }, [ensureAnalyser, playable.url]);

  const invalidateActivity = useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: ["library", "shelves"] });
  }, [queryClient]);

  const persist = useCallback((
    current: MediaSession,
    itemOrdinal: number,
    position: number,
    duration: number | null,
    status: "active" | "paused" | "completed",
  ) => {
    const request = {
      sessionId: current.id,
      itemOrdinal,
      positionMs: Math.round(position * 1_000),
      durationMs: duration === null ? null : Math.round(duration * 1_000),
      completed: status === "completed",
      status,
    } as const;
    const requestNumber = progressRequestRef.current + 1;
    progressRequestRef.current = requestNumber;
    progressWriteRef.current = progressWriteRef.current.then(async () => {
      try {
        const updated = await gateway.updateMediaProgress(request);
        if (requestNumber !== progressRequestRef.current) return;
        setSession((active) => {
          if (active?.id !== updated.id) return active;
          sessionRef.current = updated;
          return updated;
        });
      } catch {
        return;
      }
    });
  }, [gateway]);

  const adopt = useCallback((next: MediaSession) => {
    playAttemptRef.current += 1;
    shouldPlayRef.current = true;
    resumePlaybackRef.current = false;
    preparedSourceRef.current = "";
    pendingRestoreRef.current = {
      sessionId: next.id,
      itemOrdinal: next.progress.itemOrdinal,
      positionSeconds: next.progress.positionMs / 1_000,
      completed: next.progress.completed,
    };
    audioRef.current?.pause();
    sessionRef.current = next;
    setSession(next);
    setOrdinal(next.progress.itemOrdinal);
    setPositionSeconds(next.progress.positionMs / 1_000);
    setDurationSeconds(next.progress.durationMs === null ? null : next.progress.durationMs / 1_000);
    setPlaying(false);
    setError("");
    reportedSecondRef.current = -1;
    setPrepareRequest((current) => current + 1);
    return next;
  }, []);

  const stopPublishedVideo = useCallback(() => {
    const active = videoPlaybackRef.current;
    if (!active) return;
    videoPlaybackRef.current = null;
    setVideoPlayback(null);
    active.close();
  }, []);

  const openWork = useCallback(async (installationId: string) => {
    try {
      stopPublishedVideo();
      void ensureAnalyser();
      const element = audioRef.current;
      if (session?.installationId === installationId && element && playable.url) {
        resumePlaybackRef.current = element.paused;
        await playCurrent();
        return session;
      }
      return adopt(await gateway.openMediaSession(installationId));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      return null;
    }
  }, [adopt, ensureAnalyser, gateway, playable.url, playCurrent, session, stopPublishedVideo]);

  const openVoiceQueue = useCallback(async () => {
    try {
      stopPublishedVideo();
      void ensureAnalyser();
      return adopt(await gateway.openPersonalizedVoiceQueue());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      return null;
    }
  }, [adopt, ensureAnalyser, gateway, stopPublishedVideo]);

  const closeAudio = useCallback(() => {
    const current = sessionRef.current;
    playAttemptRef.current += 1;
    shouldPlayRef.current = false;
    resumePlaybackRef.current = false;
    pendingRestoreRef.current = null;
    preparedSourceRef.current = "";
    audioRef.current?.pause();
    sessionRef.current = null;
    setSession(null);
    setOrdinal(null);
    setPlaying(false);
    setPositionSeconds(0);
    setDurationSeconds(null);
    if (current) {
      void gateway.closeMediaSession(current.id).catch(() => undefined);
      invalidateActivity();
    }
  }, [gateway, invalidateActivity]);

  const publishVideoPlayback = useCallback((next: VideoPlaybackRegistration) => {
    const changingOwner = videoPlaybackRef.current?.session.id !== next.session.id;
    if (changingOwner && sessionRef.current) closeAudio();
    videoPlaybackRef.current = next;
    setVideoPlayback(next);
  }, [closeAudio]);

  const releaseVideoPlayback = useCallback((sessionId: string) => {
    if (videoPlaybackRef.current?.session.id !== sessionId) return;
    videoPlaybackRef.current = null;
    setVideoPlayback(null);
  }, []);

  const selectOrdinal = useCallback((next: number, positionSeconds = 0) => {
    const current = sessionRef.current;
    if (!current) return;
    const position = Math.max(0, Number.isFinite(positionSeconds) ? positionSeconds : 0);
    playAttemptRef.current += 1;
    shouldPlayRef.current = true;
    resumePlaybackRef.current = false;
    preparedSourceRef.current = "";
    pendingRestoreRef.current = {
      sessionId: current.id,
      itemOrdinal: next,
      positionSeconds: position,
      completed: false,
    };
    audioRef.current?.pause();
    setOrdinal(next);
    setPositionSeconds(position);
    setDurationSeconds(null);
    setPlaying(false);
    setError("");
    reportedSecondRef.current = -1;
    setPrepareRequest((current) => current + 1);
  }, []);

  const step = useCallback((direction: -1 | 1) => {
    const index = items.findIndex((candidate) => candidate.ordinal === ordinal);
    const next = items[index + direction];
    if (next) selectOrdinal(next.ordinal);
    else if (direction === 1 && session) {
      shouldPlayRef.current = false;
      setPlaying(false);
      invalidateActivity();
    }
  }, [invalidateActivity, items, ordinal, selectOrdinal, session]);

  const toggle = useCallback(() => {
    const element = audioRef.current;
    if (!element || !playable.url) return;
    if (element.paused) {
      resumePlaybackRef.current = true;
      void playCurrent();
    } else {
      playAttemptRef.current += 1;
      shouldPlayRef.current = false;
      resumePlaybackRef.current = false;
      setPlaying(false);
      element.pause();
      if (session && ordinal !== null) {
        const duration = Number.isFinite(element.duration) && element.duration > 0
          ? element.duration
          : durationSeconds;
        persist(session, ordinal, element.currentTime, duration, "paused");
      }
    }
  }, [durationSeconds, ordinal, persist, playable.url, playCurrent, session]);

  const seek = useCallback((next: number, persistProgress = true) => {
    const element = audioRef.current;
    const bounded = clampPlaybackPosition(next, durationSeconds);
    setPositionSeconds(bounded);
    if (element && playable.url) element.currentTime = bounded;
    if (persistProgress && session && ordinal !== null) {
      persist(session, ordinal, bounded, durationSeconds, playing ? "active" : "paused");
    }
  }, [durationSeconds, ordinal, persist, playable.url, playing, session]);

  const skip = useCallback((offsetSeconds: number) => {
    const position = audioRef.current?.currentTime ?? positionSeconds;
    seek(position + offsetSeconds);
  }, [positionSeconds, seek]);

  const setVolume = useCallback((next: number) => {
    const bounded = Math.max(0, Math.min(1, next));
    setStoredVolume(bounded);
    if (audioRef.current) audioRef.current.volume = bounded;
  }, []);

  const setPlaybackRate = useCallback((next: number) => {
    const bounded = clampPlaybackRate(next);
    setStoredPlaybackRate(bounded);
    if (audioRef.current) audioRef.current.playbackRate = bounded;
    videoPlaybackRef.current?.setPlaybackRate(bounded);
  }, []);

  const applyQueueSettings = useCallback((repeatMode: MediaRepeatMode, shuffle: boolean) => {
    if (!session) return;
    void gateway.updateMediaQueueSettings({ sessionId: session.id, repeatMode, shuffle })
      .then((updated) => {
        sessionRef.current = updated;
        setSession(updated);
      })
      .catch(() => undefined);
  }, [gateway, session]);

  const setRepeatMode = useCallback((mode: MediaRepeatMode) => {
    if (session) applyQueueSettings(mode, session.shuffle);
  }, [applyQueueSettings, session]);

  const setShuffle = useCallback((shuffle: boolean) => {
    if (session) applyQueueSettings(session.repeatMode, shuffle);
  }, [applyQueueSettings, session]);

  const close = useCallback(() => {
    const active = videoPlaybackRef.current;
    if (active) active.close();
    else closeAudio();
  }, [closeAudio]);

  useEffect(() => {
    const element = audioRef.current;
    if (!element || !playable.url || !session || ordinal === null) return;
    const sessionId = session.id;
    const itemOrdinal = ordinal;
    let disposed = false;
    let awaitingSeek = false;

    const startWhenReady = () => {
      if (disposed || !shouldPlayRef.current) return;
      if (element.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA && !element.seeking) {
        void playCurrent();
        return;
      }
      element.removeEventListener("canplay", startWhenReady);
      element.addEventListener("canplay", startWhenReady, { once: true });
    };

    const finishPreparation = () => {
      if (disposed) return;
      element.removeEventListener("seeked", finishPreparation);
      if (preparedSourceRef.current === playable.url) return;
      awaitingSeek = false;
      preparedSourceRef.current = playable.url;
      startWhenReady();
    };

    const prepare = () => {
      if (disposed) return;
      const duration = Number.isFinite(element.duration) && element.duration > 0
        ? element.duration
        : null;
      const pending = pendingRestoreRef.current;
      const matches = pending?.sessionId === sessionId && pending.itemOrdinal === itemOrdinal;
      const target = restorePlaybackPosition(
        matches ? pending.positionSeconds : 0,
        duration,
        matches ? pending.completed : false,
      );
      pendingRestoreRef.current = null;
      setPositionSeconds(target);
      setDurationSeconds(duration);
      reportedSecondRef.current = Math.floor(target);

      if (Math.abs(element.currentTime - target) <= 0.01) {
        finishPreparation();
        return;
      }
      awaitingSeek = true;
      element.addEventListener("seeked", finishPreparation, { once: true });
      element.currentTime = target;
      if (!element.seeking) finishPreparation();
    };

    preparedSourceRef.current = "";
    if (element.readyState >= HTMLMediaElement.HAVE_METADATA) prepare();
    else element.addEventListener("loadedmetadata", prepare, { once: true });
    return () => {
      disposed = true;
      element.removeEventListener("loadedmetadata", prepare);
      element.removeEventListener("canplay", startWhenReady);
      if (awaitingSeek) element.removeEventListener("seeked", finishPreparation);
    };
  }, [ordinal, playCurrent, playable.url, prepareRequest, session?.id]);

  useEffect(() => {
    if (playable.error) setError(playable.error);
  }, [playable.error]);

  const handleTimeUpdate = () => {
    const element = audioRef.current;
    if (!element || !playable.url || preparedSourceRef.current !== playable.url) return;
    const duration = Number.isFinite(element.duration) && element.duration > 0
      ? element.duration
      : durationSeconds;
    const position = clampPlaybackPosition(element.currentTime, duration);
    setPositionSeconds(position);
    setDurationSeconds(duration);
    const whole = Math.floor(position);
    if (whole === reportedSecondRef.current) return;
    reportedSecondRef.current = whole;
    if (session && ordinal !== null) {
      persist(session, ordinal, position, duration, element.paused ? "paused" : "active");
    }
  };

  const handlePlay = () => {
    const element = audioRef.current;
    if (!shouldPlayRef.current) {
      element?.pause();
      return;
    }
    setPlaying(true);
  };

  const handlePause = () => {
    setPlaying(false);
  };

  const handlePlaybackReady = () => {
    const element = audioRef.current;
    if (
      element
      && element.paused
      && shouldPlayRef.current
      && preparedSourceRef.current === playable.url
    ) {
      void playCurrent();
    }
  };

  const handleEnded = () => {
    const element = audioRef.current;
    const duration = element && Number.isFinite(element.duration) ? element.duration : durationSeconds;
    if (session && ordinal !== null) persist(session, ordinal, duration ?? 0, duration, "completed");
    if (session?.repeatMode === "one") {
      seek(0);
      void playCurrent();
      return;
    }
    step(1);
  };

  useEffect(() => {
    if (audioRef.current) audioRef.current.playbackRate = playbackRate;
  }, [playable.url, playbackRate]);

  const value = useMemo<MediaPlaybackValue>(() => ({
    session: videoPlayback?.session ?? session,
    item: videoPlayback?.item ?? item,
    items: videoPlayback?.items ?? items,
    playing: videoPlayback?.playing ?? playing,
    positionSeconds: videoPlayback?.positionSeconds ?? positionSeconds,
    durationSeconds: videoPlayback?.durationSeconds ?? durationSeconds,
    volume: videoPlayback?.volume ?? volume,
    loading: videoPlayback?.loading ?? playable.loading,
    error: videoPlayback?.error ?? error,
    failure: videoPlayback?.failure ?? playable.failure,
    analyser: videoPlayback ? null : analyser,
    playbackRate,
    setPlaybackRate,
    bufferedSeconds: videoPlayback?.bufferedSeconds ?? 0,
    videoDisplay: videoPlayback ? {
      fit: videoPlayback.fit,
      fullscreen: videoPlayback.fullscreen,
      subtitles: videoPlayback.subtitles,
      subtitleTrack: videoPlayback.subtitleTrack,
      subtitleError: videoPlayback.subtitleError,
      setFit: videoPlayback.setFit,
      toggleFullscreen: videoPlayback.toggleFullscreen,
      setSubtitle: videoPlayback.setSubtitle,
      togglePictureInPicture: videoPlayback.togglePictureInPicture,
    } : null,
    openWork,
    openVoiceQueue,
    toggle: videoPlayback?.toggle ?? toggle,
    seek: videoPlayback?.seek ?? seek,
    skip: videoPlayback?.skip ?? skip,
    selectOrdinal: videoPlayback?.selectOrdinal ?? selectOrdinal,
    step: videoPlayback?.step ?? step,
    setVolume: videoPlayback?.setVolume ?? setVolume,
    setRepeatMode: videoPlayback?.setRepeatMode ?? setRepeatMode,
    setShuffle: videoPlayback?.setShuffle ?? setShuffle,
    close,
    publishVideoPlayback,
    releaseVideoPlayback,
  }), [
    analyser, close, durationSeconds, error, item, items, openVoiceQueue, openWork, playable.failure, playable.loading,
    playing, positionSeconds, publishVideoPlayback, releaseVideoPlayback, seek, selectOrdinal, session,
    setRepeatMode, setShuffle, setVolume, skip, step, toggle, videoPlayback, volume,
  ]);

  const mutedVolumeRef = useRef(0);
  const shortcutSession = videoPlayback?.session ?? session;
  const shortcutVolume = videoPlayback?.volume ?? volume;
  const shortcutSetVolume = videoPlayback?.setVolume ?? setVolume;
  const shortcutStep = videoPlayback?.step ?? step;
  const shortcutToggle = videoPlayback?.toggle ?? toggle;
  const playbackHandlers = useMemo(() => ({
    playPause: () => shortcutToggle(),
    nextTrack: () => shortcutStep(1),
    previousTrack: () => shortcutStep(-1),
    toggleMute: () => {
      if (shortcutVolume > 0) {
        mutedVolumeRef.current = shortcutVolume;
        shortcutSetVolume(0);
      } else {
        shortcutSetVolume(mutedVolumeRef.current > 0 ? mutedVolumeRef.current : 1);
      }
    },
  }), [shortcutSetVolume, shortcutStep, shortcutToggle, shortcutVolume]);
  useBoundKeys("playback", playbackHandlers, { enabled: Boolean(shortcutSession) });

  return (
    <MediaPlaybackContext.Provider value={value}>
      {children}
      <audio
        ref={audioRef}
        src={playable.url || undefined}
        preload="auto"
        onTimeUpdate={handleTimeUpdate}
        onPlay={handlePlay}
        onPause={handlePause}
        onCanPlay={handlePlaybackReady}
        onSeeked={handlePlaybackReady}
        onEnded={handleEnded}
      />
    </MediaPlaybackContext.Provider>
  );
}

export function useMediaPlayback(): MediaPlaybackValue {
  const context = useContext(MediaPlaybackContext);
  if (!context) throw new Error("useMediaPlayback must be used within MediaPlaybackProvider");
  return context;
}
