import { describe, expect, it } from "vitest";

import { orderedSessionItems } from "./mediaSession";
import type { MediaSession, MediaSessionItem } from "./types";

describe("orderedSessionItems", () => {
  it("sorts by ordinal when shuffle is off", () => {
    expect(ordinals(orderedSessionItems(session(false)))).toEqual([1, 2, 3, 4, 5]);
  });

  it("shuffles deterministically from the session id", () => {
    const first = ordinals(orderedSessionItems(session(true)));
    const second = ordinals(orderedSessionItems(session(true)));
    expect(first).toEqual(second);
    expect(first).not.toEqual([1, 2, 3, 4, 5]);
    expect([...first].sort((a, b) => a - b)).toEqual([1, 2, 3, 4, 5]);
  });

  it("lets the caller override the stored shuffle choice", () => {
    expect(ordinals(orderedSessionItems(session(true), false))).toEqual([1, 2, 3, 4, 5]);
  });
});

function ordinals(items: MediaSessionItem[]): number[] {
  return items.map((item) => item.ordinal);
}

function session(shuffle: boolean): MediaSession {
  return {
    id: "session-1",
    kind: "work",
    installationId: "installation",
    action: "play_audio",
    status: "active",
    repeatMode: "off",
    shuffle,
    items: [5, 3, 1, 4, 2].map((ordinal) => ({
      ordinal,
      installationId: "installation",
      workCode: null,
      relativePath: `track-${ordinal}.mp3`,
      mediaType: "audio" as const,
      sizeBytes: null,
      discNumber: null,
      trackNumber: ordinal,
      bonus: false,
    })),
    progress: { itemOrdinal: 1, positionMs: 0, durationMs: null, completed: false, updatedAt: "2026-08-13T10:00:00Z" },
    openedAt: "2026-08-13T10:00:00Z",
    updatedAt: "2026-08-13T10:00:00Z",
    endedAt: null,
    error: null,
  };
}
