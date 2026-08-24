import { describe, expect, it } from "vitest";

import {
  clampVideoVolume,
  defaultVideoPlayerPreferences,
  readVideoPlayerPreferences,
  videoPlaybackFailure,
  videoStepTarget,
  writeVideoPlayerPreferences,
} from "./videoPlayerModel";
import {
  bufferedAhead,
  seekFractionForKey,
} from "./videoPlayerModel";
import type { MediaSessionItem } from "./types";

describe("video player model", () => {
  it("persists validated preferences per installation", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    };
    const preferences = {
      fit: "cover" as const,
      volume: 0.42,
      muted: true,
      subtitleLabel: "ENG",
    };

    writeVideoPlayerPreferences("installation-1", preferences, storage);

    expect(readVideoPlayerPreferences("installation-1", storage)).toEqual(preferences);
    expect(readVideoPlayerPreferences("installation-2", storage)).toEqual(defaultVideoPlayerPreferences);
  });

  it("rejects corrupt preferences and clamps volume", () => {
    const storage = {
      getItem: () => JSON.stringify({ fit: "stretch", volume: 18, muted: "yes" }),
      setItem: () => undefined,
    };
    expect(readVideoPlayerPreferences("installation", storage)).toEqual({
      ...defaultVideoPlayerPreferences,
      volume: 1,
    });
    expect(clampVideoVolume(-1)).toBe(0);
    expect(clampVideoVolume(0.456)).toBe(0.46);
    expect(clampVideoVolume(Number.NaN)).toBe(1);
  });

  it("migrates the legacy broken original fit to responsive contain", () => {
    const values = new Map<string, string>([[
      "dla-launcher:video-player:v1:installation",
      JSON.stringify({ fit: "original", volume: 0.38, muted: true, subtitleLabel: "KOR" }),
    ]]);
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    };

    expect(readVideoPlayerPreferences("installation", storage)).toEqual({
      fit: "contain",
      volume: 0.38,
      muted: true,
      subtitleLabel: "KOR",
    });
    expect(JSON.parse(values.get("dla-launcher:video-player:v2:installation") ?? "null")).toEqual({
      fit: "contain",
      volume: 0.38,
      muted: true,
      subtitleLabel: "KOR",
    });
  });

  it("steps through a playlist and wraps only in repeat-all mode", () => {
    const items = [item(2), item(7), item(9)];
    expect(videoStepTarget(items, 7, 1, "off")?.ordinal).toBe(9);
    expect(videoStepTarget(items, 2, -1, "off")).toBeUndefined();
    expect(videoStepTarget(items, 2, -1, "all")?.ordinal).toBe(9);
    expect(videoStepTarget(items, 9, 1, "all")?.ordinal).toBe(2);
    expect(videoStepTarget([item(2)], 2, 1, "all")).toBeUndefined();
  });

  it("classifies native media-element failures", () => {
    expect(videoPlaybackFailure(1)).toBeNull();
    expect(videoPlaybackFailure(2)).toBe("network");
    expect(videoPlaybackFailure(3)).toBe("decode");
    expect(videoPlaybackFailure(4)).toBe("unsupported");
  });
});

function item(ordinal: number): MediaSessionItem {
  return {
    ordinal,
    installationId: "installation",
    workCode: null,
    relativePath: `video-${ordinal}.mp4`,
    mediaType: "video",
    sizeBytes: null,
    discNumber: null,
    trackNumber: null,
    bonus: false,
  };
}

describe("keyboard seek helpers", () => {
  it("maps digit keys to deciles of the track", () => {
    expect(seekFractionForKey("0")).toBe(0);
    expect(seekFractionForKey("5")).toBe(0.5);
    expect(seekFractionForKey("9")).toBeCloseTo(0.9, 5);
  });

  it("ignores anything that is not a single digit", () => {
    expect(seekFractionForKey("k")).toBeNull();
    expect(seekFractionForKey("")).toBeNull();
    expect(seekFractionForKey("10")).toBeNull();
  });

});

describe("bufferedAhead", () => {
  it("reports how far the range covering the playhead reaches", () => {
    expect(bufferedAhead([{ start: 0, end: 30 }], 10)).toBe(30);
  });

  it("ignores ranges that do not contain the playhead", () => {
    expect(bufferedAhead([{ start: 0, end: 5 }, { start: 60, end: 90 }], 30)).toBe(30);
  });

  it("returns the playhead when nothing is buffered", () => {
    expect(bufferedAhead([], 12)).toBe(12);
  });
})
