// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import {
  MediaPlaybackProvider,
  useMediaPlayback,
  type MediaPlaybackGateway,
  type VideoPlaybackRegistration,
} from "./MediaPlaybackProvider";
import type { MediaSession, MediaSessionItem } from "./types";

afterEach(() => {
  cleanup();
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
});

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
