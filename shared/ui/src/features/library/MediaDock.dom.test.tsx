// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import { PresentationProvider } from "../../preferences/PresentationProvider";
import { MediaDock } from "./MediaDock";
import {
  MediaPlaybackProvider,
  useMediaPlayback,
  type MediaPlaybackGateway,
  type VideoPlaybackRegistration,
} from "./MediaPlaybackProvider";
import type { MediaSession, MediaSessionItem } from "./types";

afterEach(cleanup);

describe("MediaDock progress bar", () => {
  it("seeks once on release for a click, through the blur a press can cause", () => {
    const registration = videoRegistration();
    renderDock(registration);
    const rail = screen.getByLabelText("Playback position") as HTMLInputElement;

    fireEvent.pointerDown(rail);
    fireEvent.input(rail, { target: { value: "30" } });
    fireEvent.blur(rail);

    expect(registration.seek).not.toHaveBeenCalled();
    expect(rail.value).toBe("30");

    fireEvent.pointerUp(rail);
    fireEvent.keyUp(rail, { key: "ArrowRight" });

    expect(registration.seek).toHaveBeenCalledTimes(1);
    expect(registration.seek).toHaveBeenCalledWith(30);
    // The player still reports the old position for a moment; the bar holds.
    expect(rail.value).toBe("30");
  });

  it("previews while dragging and lands on release", () => {
    const registration = videoRegistration();
    renderDock(registration);
    const rail = screen.getByLabelText("Playback position") as HTMLInputElement;

    fireEvent.pointerDown(rail);
    fireEvent.pointerMove(rail);
    fireEvent.input(rail, { target: { value: "20" } });
    fireEvent.input(rail, { target: { value: "25" } });
    fireEvent.pointerUp(rail);

    expect(registration.seek.mock.calls).toEqual([[20, false], [25, false], [25]]);
  });

  it("commits a keyboard step on key release", () => {
    const registration = videoRegistration();
    renderDock(registration);
    const rail = screen.getByLabelText("Playback position") as HTMLInputElement;

    fireEvent.input(rail, { target: { value: "0.1" } });
    fireEvent.keyUp(rail, { key: "ArrowRight" });

    expect(registration.seek.mock.calls).toEqual([[0.1]]);
  });
});

describe("MediaDock in fullscreen", () => {
  it("mutes from the volume icon and offers to unmute when silent", () => {
    const registration = videoRegistration();
    renderDock(registration);

    fireEvent.click(screen.getByRole("button", { name: "Mute" }));
    expect(registration.setVolume).toHaveBeenCalledWith(0);

    cleanup();
    const silent = { ...videoRegistration(), volume: 0 };
    renderDock(silent);
    fireEvent.click(screen.getByRole("button", { name: "Unmute" }));
    expect(silent.setVolume).toHaveBeenCalledWith(1);
  });

  it("lies over the picture and leaves with the player's controls", () => {
    const registration = videoRegistration();
    registration.fullscreen = true;
    registration.controlsVisible = false;
    renderDock(registration);

    const dock = screen.getByRole("region", { name: "Now playing" });
    expect(dock.classList.contains("is-overlay")).toBe(true);
    expect(dock.classList.contains("is-hidden")).toBe(true);
  });
});

describe("MediaDock video menu", () => {
  it("offers fit or fill, and full screen apart from them", () => {
    const registration = videoRegistration();
    renderDock(registration);

    fireEvent.click(screen.getByRole("button", { name: "More options" }));

    fireEvent.click(screen.getByRole("menuitem", { name: "Video fit, Fit video" }));
    expect(screen.getByRole("menuitemradio", { name: "Fit video" }).getAttribute("aria-checked")).toBe("true");
    expect(screen.queryByRole("menuitemradio", { name: "Original size" })).toBeNull();
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Fill video" }));
    expect(registration.setFit).toHaveBeenCalledWith("cover");

    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    // The glow switches in place and keeps the menu open.
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Ambient glow" }));
    expect(registration.setAmbient).toHaveBeenCalledWith(false);
    fireEvent.click(screen.getByRole("menuitem", { name: "Enter fullscreen" }));
    expect(registration.toggleFullscreen).toHaveBeenCalledOnce();
  });
});

describe("MediaDock subtitle style", () => {
  it("sets each part of the style from the subtitles menu, which stays open", () => {
    const registration = videoRegistration();
    registration.subtitles = [{ index: 0, label: "ENG", language: "en", format: "sub_rip" }];
    renderDock(registration);

    fireEvent.click(screen.getByRole("button", { name: "Subtitles" }));
    expect(screen.getByRole("menuitem", { name: "Size, 100%" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Color, White" })).toBeTruthy();

    fireEvent.click(screen.getByRole("menuitem", { name: "Size, 100%" }));
    expect(screen.queryByRole("menuitem", { name: "Color, White" })).toBeNull();
    fireEvent.click(screen.getByRole("menuitemradio", { name: "150%" }));

    // Back on the list, with the menu still open for the next setting.
    expect(screen.getByRole("menu", { name: "Subtitles" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "Background, 75%" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "None" }));

    expect(registration.setSubtitleStyle.mock.calls).toEqual([
      [{ size: 1.5, color: "white", opacity: 1, background: 0.75 }],
      [{ size: 1, color: "white", opacity: 1, background: 0 }],
    ]);
    expect(screen.getByRole("menu", { name: "Subtitles" })).toBeTruthy();
  });
});

describe("MediaDock volume", () => {
  it("turns the volume with the wheel over the control and keeps the page still", () => {
    const registration = videoRegistration();
    renderDock(registration);
    const slider = screen.getByRole("slider", { name: /volume/i });

    const scrolled = fireEvent.wheel(slider, { deltaY: -100 });
    fireEvent.wheel(slider, { deltaY: 100 });

    expect(scrolled).toBe(false);
    expect(registration.setVolume).toHaveBeenCalledTimes(2);
    expect(vi.mocked(registration.setVolume).mock.calls[0]?.[0]).toBeCloseTo(0.65);
    expect(vi.mocked(registration.setVolume).mock.calls[1]?.[0]).toBeCloseTo(0.55);
  });
});

function renderDock(registration: VideoPlaybackRegistration) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <KeyBindingsProvider>
          <MediaPlaybackProvider gateway={gateway()}>
            <PublishVideo registration={registration} />
            <MediaDock onExpand={() => undefined} />
          </MediaPlaybackProvider>
        </KeyBindingsProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function PublishVideo({ registration }: { registration: VideoPlaybackRegistration }) {
  const playback = useMediaPlayback();
  useEffect(() => {
    playback.publishVideoPlayback(registration);
    return () => playback.releaseVideoPlayback(registration.session.id);
  }, [playback.publishVideoPlayback, playback.releaseVideoPlayback, registration]);
  return null;
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

function videoRegistration() {
  const seek = vi.fn<VideoPlaybackRegistration["seek"]>();
  const setSubtitleStyle = vi.fn<VideoPlaybackRegistration["setSubtitleStyle"]>();
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
    progress: { itemOrdinal: 0, positionMs: 0, durationMs: 60_000, completed: false, updatedAt: "2026-11-01T00:00:00Z" },
    openedAt: "2026-11-01T00:00:00Z",
    updatedAt: "2026-11-01T00:00:00Z",
    endedAt: null,
    error: null,
  };
  const registration: VideoPlaybackRegistration & { seek: typeof seek; setSubtitleStyle: typeof setSubtitleStyle } = {
    session,
    item,
    items: [item],
    playing: false,
    positionSeconds: 10,
    durationSeconds: 60,
    volume: 0.6,
    loading: false,
    error: "",
    failure: null,
    fit: "contain",
    ambient: true,
    fullscreen: false,
    subtitles: [],
    subtitleTrack: null,
    subtitleError: false,
    subtitleStyle: { size: 1, color: "white", opacity: 1, background: 0.75 },
    controlsVisible: true,
    toggle: vi.fn(),
    seek,
    skip: vi.fn(),
    selectOrdinal: vi.fn(),
    step: vi.fn(),
    setVolume: vi.fn(),
    setRepeatMode: vi.fn(),
    setShuffle: vi.fn(),
    setFit: vi.fn(),
    setAmbient: vi.fn(),
    toggleFullscreen: vi.fn(),
    setSubtitle: vi.fn(),
    setSubtitleStyle,
    setPlaybackRate: vi.fn(),
    canPictureInPicture: false,
    togglePictureInPicture: vi.fn(),
    bufferedSeconds: 0,
    close: vi.fn(),
  };
  return registration;
}
