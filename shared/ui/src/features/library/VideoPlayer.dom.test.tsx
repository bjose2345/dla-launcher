// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import { PresentationProvider } from "../../preferences/PresentationProvider";
import { MediaPlaybackProvider, useMediaPlayback } from "./MediaPlaybackProvider";
import type { LibraryGateway, MediaSession, MediaSessionItem, NativeVideoState } from "./types";
import { VideoPlayer } from "./VideoPlayer";

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, status: 200 }));
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => (
    window.setTimeout(() => callback(0), 0)
  ));
  vi.stubGlobal("cancelAnimationFrame", (handle: number) => window.clearTimeout(handle));
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    x: 10, y: 96, left: 10, top: 96, width: 640, height: 360, right: 650, bottom: 456, toJSON: () => ({}),
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("VideoPlayer with a surface the page draws the bar for", () => {
  it("shows its own bar beside a native surface that nothing can be drawn over", async () => {
    const { gateway } = renderPlayer();

    await waitFor(() => expect(gateway.openNativeVideo).toHaveBeenCalledOnce());
    expect(screen.getByRole("button", { name: "Back to Library" })).toBeTruthy();
    expect(document.querySelector(".video-player-page")?.classList.contains("is-page-chrome")).toBe(true);
  });

  it("settles instead of republishing its controls for a page that passes new callbacks on every render", async () => {
    let renders = 0;
    renderPlayer({
      wrap: (player) => function Page() {
        // Like MediaSessionPage: reads the playback context and passes
        // inline callbacks.
        useMediaPlayback();
        renders += 1;
        return player({ onRepeatMode: () => {}, onShuffle: () => {}, onBack: () => {} });
      },
    });
    await new Promise((resolve) => setTimeout(resolve, 300));
    const settled = renders;
    await new Promise((resolve) => setTimeout(resolve, 300));

    expect(renders).toBe(settled);
    expect(settled).toBeLessThan(20);
  });

  it("keeps the chosen position until the native player reports a seek near it", async () => {
    let playback: ReturnType<typeof useMediaPlayback> | undefined;
    const { gateway, events } = renderPlayer({
      wrap: (player) => function Page() {
        playback = useMediaPlayback();
        return player({});
      },
    });
    await waitFor(() => expect(events.listener).toBeTypeOf("function"));
    act(() => events.listener?.({ ...nativeState("ready"), durationSeconds: 100, readyState: 4 }));

    act(() => playback?.seek(40));
    expect(gateway.controlNativeVideo).toHaveBeenCalledWith({ sessionId: "session-1", command: "seek", value: 40 });
    expect(playback?.positionSeconds).toBe(40);

    act(() => events.listener?.({ ...nativeState("time"), durationSeconds: 100, positionSeconds: 12.5, readyState: 4 }));
    expect(playback?.positionSeconds).toBe(40);
    act(() => events.listener?.({ ...nativeState("time"), durationSeconds: 100, positionSeconds: 0, readyState: 4 }));
    expect(playback?.positionSeconds).toBe(40);

    act(() => events.listener?.({ ...nativeState("time"), durationSeconds: 100, positionSeconds: 39.8, readyState: 4 }));
    expect(playback?.positionSeconds).toBe(39.8);
    act(() => events.listener?.({ ...nativeState("time"), durationSeconds: 100, positionSeconds: 40.3, readyState: 4 }));
    expect(playback?.positionSeconds).toBe(40.3);
  });

  it("scrubs with preview seeks and holds the thumb until the accurate seek on release lands", async () => {
    let playback: ReturnType<typeof useMediaPlayback> | undefined;
    const { gateway, events } = renderPlayer({
      wrap: (player) => function Page() {
        playback = useMediaPlayback();
        return player({});
      },
    });
    await waitFor(() => expect(events.listener).toBeTypeOf("function"));
    act(() => events.listener?.({ ...nativeState("ready"), durationSeconds: 100, readyState: 4 }));
    gateway.controlNativeVideo.mockClear();

    act(() => playback?.seek(30, false));
    act(() => playback?.seek(31, false));
    expect(gateway.controlNativeVideo.mock.calls.map(([request]) => request)).toEqual([
      { sessionId: "session-1", command: "seek", value: 30, preview: true },
      { sessionId: "session-1", command: "seek", value: 31, preview: true },
    ]);
    // A preview seek lands on a keyframe near the thumb; the thumb stays put.
    act(() => events.listener?.({ ...nativeState("time"), durationSeconds: 100, positionSeconds: 30.9, readyState: 4 }));
    expect(playback?.positionSeconds).toBe(31);

    act(() => playback?.seek(31));
    expect(gateway.controlNativeVideo).toHaveBeenLastCalledWith({ sessionId: "session-1", command: "seek", value: 31 });
    act(() => events.listener?.({ ...nativeState("time"), durationSeconds: 100, positionSeconds: 31.1, readyState: 4 }));
    expect(playback?.positionSeconds).toBe(31.1);
  });

  it("plays and pauses from a click on the picture", async () => {
    const { gateway, events } = renderPlayer();
    await waitFor(() => expect(events.listener).toBeTypeOf("function"));
    act(() => events.listener?.({ ...nativeState("ready"), durationSeconds: 100, readyState: 4 }));
    const picture = document.querySelector(".video-player-native-surface") as HTMLElement;

    fireEvent.click(picture);
    await waitFor(() => expect(gateway.controlNativeVideo).toHaveBeenCalledWith({ sessionId: "session-1", command: "play", badge: true }));

    act(() => events.listener?.({ ...nativeState("playing"), durationSeconds: 100, readyState: 4, paused: false }));
    fireEvent.click(picture);
    expect(gateway.controlNativeVideo).toHaveBeenCalledWith({ sessionId: "session-1", command: "pause", badge: true });
    // The native picture draws the badge itself.
    expect(document.querySelector(".video-player-badge")).toBeNull();
  });

  it("shows a close button in fullscreen and tells the surface what lies over it", async () => {
    const { gateway, events } = renderPlayer();
    await waitFor(() => expect(events.listener).toBeTypeOf("function"));
    act(() => events.listener?.({ ...nativeState("fullscreen"), fullscreen: true, readyState: 4 }));

    const exit = await screen.findByRole("button", { name: "Exit fullscreen" });
    expect(screen.queryByRole("button", { name: "Back" })).toBeNull();
    await waitFor(() => {
      const last = gateway.updateNativeVideoViewport.mock.calls.at(-1);
      expect(last?.[3]).toEqual(expect.arrayContaining([expect.objectContaining({ width: 640, height: 360 })]));
    });

    fireEvent.click(exit);
    expect(gateway.controlNativeVideo).toHaveBeenCalledWith({ sessionId: "session-1", command: "fullscreen", enabled: false });
  });

  it("offers to open a video it cannot decode in a video player", async () => {
    const { gateway, events } = renderPlayer();
    await waitFor(() => expect(events.listener).toBeTypeOf("function"));

    act(() => events.listener?.(nativeState("error")));

    fireEvent.click(await screen.findByRole("button", { name: "Open in a video player" }));
    expect(gateway.openVideoExternally).toHaveBeenCalledWith("session-1", 0);
  });
});

type PlayerCallbacks = Partial<Pick<Parameters<typeof VideoPlayer>[0], "onRepeatMode" | "onShuffle" | "onBack">>;

function renderPlayer({
  wrap,
}: {
  wrap?: (player: (callbacks: PlayerCallbacks) => ReactElement) => () => ReactElement;
} = {}) {
  const events: { listener?: (state: NativeVideoState) => void } = {};
  const gateway = {
    mediaAssetUrl: vi.fn(() => "http://dla-media.localhost/session-1/0"),
    openNativeVideo: vi.fn().mockResolvedValue({ surfaceId: "surface-1", subtitles: [] }),
    updateNativeVideoViewport: vi.fn().mockResolvedValue(undefined),
    controlNativeVideo: vi.fn().mockResolvedValue(undefined),
    closeNativeVideo: vi.fn().mockResolvedValue(undefined),
    subscribeNativeVideoState: vi.fn(async (listener: (state: NativeVideoState) => void) => {
      events.listener = listener;
      return () => {};
    }),
    nativeVideoChrome: "page" as const,
    openVideoExternally: vi.fn().mockResolvedValue(undefined),
    updateMediaProgress: vi.fn().mockResolvedValue(undefined),
  };
  const item: MediaSessionItem = {
    ordinal: 0,
    installationId: "installation-1",
    workCode: "RJ01109096",
    relativePath: "WBunny.mp4",
    mediaType: "video",
    sizeBytes: 1_031_611_835,
    discNumber: null,
    trackNumber: null,
    bonus: false,
  };
  const session: MediaSession = {
    id: "session-1",
    kind: "work",
    installationId: "installation-1",
    action: "play_video",
    status: "active",
    repeatMode: "off",
    shuffle: false,
    items: [item],
    progress: { itemOrdinal: 0, positionMs: 0, durationMs: null, completed: false, updatedAt: "2026-11-01T00:00:00Z" },
    openedAt: "2026-11-01T00:00:00Z",
    updatedAt: "2026-11-01T00:00:00Z",
    endedAt: null,
    error: null,
  };
  const items = [item];
  const posterUrls: string[] = [];
  const player = (callbacks: PlayerCallbacks) => (
    <VideoPlayer
      gateway={gateway}
      session={session}
      installationName="Test work"
      items={items}
      currentOrdinal={0}
      positionMs={0}
      durationMs={null}
      completed={false}
      autoPlay={false}
      repeatMode="off"
      shuffle={false}
      saveError=""
      closing={false}
      posterUrls={posterUrls}
      onChoose={vi.fn()}
      onProgress={vi.fn()}
      onPlaybackState={vi.fn()}
      onEnded={vi.fn()}
      onRepeatMode={callbacks.onRepeatMode ?? vi.fn()}
      onShuffle={callbacks.onShuffle ?? vi.fn()}
      onBack={callbacks.onBack ?? vi.fn()}
    />
  );
  const Page = wrap ? wrap(player) : () => player({});
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <KeyBindingsProvider>
          <MediaPlaybackProvider gateway={gateway as unknown as LibraryGateway}>
            <Page />
          </MediaPlaybackProvider>
        </KeyBindingsProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
  return { gateway, events };
}

function nativeState(kind: NativeVideoState["kind"]): NativeVideoState {
  return {
    surfaceId: "surface-1",
    sessionId: "session-1",
    ordinal: 0,
    kind,
    positionSeconds: 0,
    durationSeconds: null,
    paused: true,
    ended: false,
    readyState: 0,
    errorCode: 4,
    subtitleTrack: null,
    actionOrdinal: null,
    fullscreen: false,
  };
}
