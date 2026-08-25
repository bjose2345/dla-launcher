// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import {
  MediaPlaybackProvider,
  useMediaPlayback,
  type MediaPlaybackGateway,
  type VideoPlaybackRegistration,
} from "./MediaPlaybackProvider";
import type { MediaSession, MediaSessionItem } from "./types";

beforeEach(() => {
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => undefined);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("MediaPlaybackProvider shortcuts", () => {
  it("routes always-live playback shortcuts through an active video session", async () => {
    const registration = videoRegistration();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <KeyBindingsProvider>
          <MediaPlaybackProvider gateway={gateway()}>
            <PublishVideo registration={registration} />
          </MediaPlaybackProvider>
        </KeyBindingsProvider>
      </QueryClientProvider>,
    );

    await screen.findByText("video-session");
    fireEvent.keyDown(window, { key: " ", ctrlKey: true });
    fireEvent.keyDown(window, { key: "m", ctrlKey: true });

    expect(registration.toggle).toHaveBeenCalledOnce();
    expect(registration.setVolume).toHaveBeenCalledWith(0);
  });

  it("starts audio before attaching the optional spectrum analyser", async () => {
    const openMediaSession = vi.fn().mockResolvedValue(audioSession());
    const audioGateway = {
      ...gateway(),
      openMediaSession,
      mediaAssetUrl: vi.fn().mockReturnValue("dla-media://localhost/media-audio/0"),
    };
    const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
    const load = vi.mocked(HTMLMediaElement.prototype.load);
    const createMediaElementSource = vi.fn().mockReturnValue({ connect: vi.fn() });
    const AudioContext = vi.fn(function AudioContextMock() {
      return {
        state: "running",
        destination: {},
        createAnalyser: vi.fn().mockReturnValue({
          connect: vi.fn(),
          fftSize: 0,
          smoothingTimeConstant: 0,
        }),
        createMediaElementSource,
        resume: vi.fn().mockResolvedValue(undefined),
      };
    });
    vi.stubGlobal("AudioContext", AudioContext);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
      ok: true,
      blob: vi.fn().mockResolvedValue(new Blob(["audio"], { type: "audio/mpeg" })),
    }));
    vi.stubGlobal("URL", {
      ...URL,
      createObjectURL: vi.fn().mockReturnValue("blob:audio-track"),
      revokeObjectURL: vi.fn(),
    });
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const view = render(
      <QueryClientProvider client={client}>
        <KeyBindingsProvider>
          <MediaPlaybackProvider gateway={audioGateway}>
            <AudioHarness />
          </MediaPlaybackProvider>
        </KeyBindingsProvider>
      </QueryClientProvider>,
    );
    const audio = view.container.querySelector("audio");
    expect(audio).not.toBeNull();
    Object.defineProperty(audio, "readyState", {
      configurable: true,
      value: HTMLMediaElement.HAVE_ENOUGH_DATA,
    });
    Object.defineProperty(audio, "duration", { configurable: true, value: 60 });

    fireEvent.click(screen.getByRole("button", { name: "Open audio" }));

    await waitFor(() => expect(play).toHaveBeenCalledOnce());
    expect(load).toHaveBeenCalled();
    expect(AudioContext).not.toHaveBeenCalled();
    expect(screen.getByTestId("playing").textContent).toBe("playing");

    fireEvent.click(screen.getByRole("button", { name: "Enable analyser" }));

    await waitFor(() => expect(createMediaElementSource).toHaveBeenCalledWith(audio));
  });
});

function AudioHarness() {
  const playback = useMediaPlayback();
  return (
    <>
      <button type="button" onClick={() => void playback.openWork("installation-audio")}>Open audio</button>
      <button type="button" onClick={playback.enableAnalyser}>Enable analyser</button>
      <span data-testid="playing">{playback.playing ? "playing" : "paused"}</span>
    </>
  );
}

function PublishVideo({ registration }: { registration: VideoPlaybackRegistration }) {
  const playback = useMediaPlayback();
  useEffect(() => {
    playback.publishVideoPlayback(registration);
    return () => playback.releaseVideoPlayback(registration.session.id);
  }, [playback.publishVideoPlayback, playback.releaseVideoPlayback, registration]);
  return <span>{playback.session?.id ?? "no-session"}</span>;
}

function gateway(): MediaPlaybackGateway {
  return {
    openMediaSession: vi.fn(),
    openPersonalizedVoiceQueue: vi.fn(),
    closeMediaSession: vi.fn(),
    updateMediaProgress: vi.fn(),
    updateMediaQueueSettings: vi.fn(),
    mediaAssetUrl: vi.fn().mockReturnValue(""),
  };
}

function videoRegistration(): VideoPlaybackRegistration {
  const item: MediaSessionItem = {
    ordinal: 0,
    installationId: "installation-1",
    workCode: "RJ00000001",
    relativePath: "movie.mp4",
    mediaType: "video",
    sizeBytes: 1024,
    discNumber: null,
    trackNumber: null,
    bonus: false,
  };
  const session: MediaSession = {
    id: "video-session",
    kind: "work",
    installationId: "installation-1",
    action: "play_video",
    status: "active",
    repeatMode: "off",
    shuffle: false,
    items: [item],
    progress: {
      itemOrdinal: 0,
      positionMs: 0,
      durationMs: 60_000,
      completed: false,
      updatedAt: "2026-08-20T00:00:00Z",
    },
    openedAt: "2026-08-20T00:00:00Z",
    updatedAt: "2026-08-20T00:00:00Z",
    endedAt: null,
    error: null,
  };
  return {
    session,
    item,
    items: [item],
    playing: true,
    positionSeconds: 0,
    durationSeconds: 60,
    volume: 0.6,
    loading: false,
    error: "",
    failure: null,
    fit: "contain",
    fullscreen: false,
    subtitles: [],
    subtitleTrack: null,
    subtitleError: false,
    toggle: vi.fn(),
    seek: vi.fn(),
    skip: vi.fn(),
    selectOrdinal: vi.fn(),
    step: vi.fn(),
    setVolume: vi.fn(),
    setRepeatMode: vi.fn(),
    setShuffle: vi.fn(),
    setFit: vi.fn(),
    toggleFullscreen: vi.fn(),
    setSubtitle: vi.fn(),
    setPlaybackRate: vi.fn(),
    togglePictureInPicture: vi.fn(),
    bufferedSeconds: 0,
    close: vi.fn(),
  };
}

function audioSession(): MediaSession {
  const item: MediaSessionItem = {
    ordinal: 0,
    installationId: "installation-audio",
    workCode: "RJ01678999",
    relativePath: "mp3/track.mp3",
    mediaType: "audio",
    sizeBytes: 1024,
    discNumber: null,
    trackNumber: 1,
    bonus: false,
  };
  return {
    id: "media-audio",
    kind: "work",
    installationId: "installation-audio",
    action: "play_audio",
    status: "active",
    repeatMode: "off",
    shuffle: false,
    items: [item],
    progress: {
      itemOrdinal: 0,
      positionMs: 0,
      durationMs: null,
      completed: false,
      updatedAt: "2026-08-25T00:00:00Z",
    },
    openedAt: "2026-08-25T00:00:00Z",
    updatedAt: "2026-08-25T00:00:00Z",
    endedAt: null,
    error: null,
  };
}
