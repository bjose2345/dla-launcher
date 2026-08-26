// @vitest-environment jsdom

import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { NativeVideoSurface, type NativeVideoGateway } from "./NativeVideoSurface";
import type { NativeVideoState, OpenNativeVideoRequest } from "./types";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("NativeVideoSurface", () => {
  it("closes a failed native surface before reporting the playback error", async () => {
    let stateListener: ((state: NativeVideoState) => void) | undefined;
    let finishClose: (() => void) | undefined;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(0), 0)
    ));
    vi.stubGlobal("cancelAnimationFrame", (handle: number) => window.clearTimeout(handle));
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(domRectangle({
      left: 40,
      top: 80,
      width: 640,
      height: 360,
    }));

    const gateway: NativeVideoGateway = {
      openNativeVideo: vi.fn().mockResolvedValue({ surfaceId: "surface-1", subtitles: [] }),
      updateNativeVideoViewport: vi.fn().mockResolvedValue(undefined),
      controlNativeVideo: vi.fn().mockResolvedValue(undefined),
      closeNativeVideo: vi.fn().mockImplementation(() => new Promise<void>((resolve) => {
        finishClose = resolve;
      })),
      subscribeNativeVideoState: vi.fn().mockImplementation(async (listener) => {
        stateListener = listener;
        return () => {};
      }),
    };
    const onState = vi.fn();

    render(
      <NativeVideoSurface
        gateway={gateway}
        request={request}
        onState={onState}
        onOpen={() => {}}
        onError={() => {}}
      />,
    );

    await waitFor(() => expect(gateway.openNativeVideo).toHaveBeenCalledOnce());
    await waitFor(() => expect(stateListener).toBeTypeOf("function"));

    const failure = nativeState("error");
    act(() => stateListener?.(failure));

    expect(gateway.closeNativeVideo).toHaveBeenCalledWith("session-1", "surface-1");
    expect(onState).not.toHaveBeenCalled();

    await act(async () => finishClose?.());

    expect(onState).toHaveBeenCalledWith(failure);
  });
});

const request: Omit<OpenNativeVideoRequest, "viewport"> = {
  sessionId: "session-1",
  ordinal: 0,
  positionSeconds: 0,
  volume: 1,
  muted: false,
  fit: "contain",
  autoPlay: true,
  posterUrl: null,
  title: "Video work",
  itemName: "video.mp4",
  completed: false,
  playlist: [{ ordinal: 0, itemName: "video.mp4" }],
  subtitleLabel: null,
  labels: {
    back: "Back",
    player: "Video player",
    play: "Play",
    markFinished: "Mark finished",
    completed: "Completed",
    playlist: "Playlist",
    openPlaylist: "Open playlist",
    closePlaylist: "Close playlist",
    nowPlaying: "Now playing",
    playbackFailed: "Playback failed",
    codecUnsupported: "Codec unsupported",
    retry: "Retry",
  },
};

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

function domRectangle({
  left,
  top,
  width,
  height,
}: {
  left: number;
  top: number;
  width: number;
  height: number;
}): DOMRect {
  return {
    x: left,
    y: top,
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    toJSON: () => ({}),
  };
}
