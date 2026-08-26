import { describe, expect, it } from "vitest";

import {
  featuredInstallationId,
  libraryContentKind,
  launchTotalsByInstallation,
  libraryLensCounts,
  matchesLibraryLens,
  playTimeParts,
} from "./libraryHome";
import type { Installation, LibraryShelves } from "./types";

describe("library home presentation", () => {
  it("derives the content surface from the reviewed action", () => {
    const value = installation();
    expect(libraryContentKind(value, "play_audio")).toBe("audio");
    expect(libraryContentKind(value, "read_images")).toBe("images");
    expect(libraryContentKind(value, "play_video")).toBe("video");
    expect(libraryContentKind(value, "open_document")).toBe("documents");
    expect(libraryContentKind(value, "launch_executable")).toBe("apps");
  });

  it("falls back to detected content without inventing an action", () => {
    const value = installation();
    value.detection.contentItems = [
      { relativePath: "01.mp3", pathKey: "01.mp3", mediaType: "audio", sizeBytes: null, modifiedAt: null, confidence: "high", reasonCodes: [] },
      { relativePath: "02.mp3", pathKey: "02.mp3", mediaType: "audio", sizeBytes: null, modifiedAt: null, confidence: "high", reasonCodes: [] },
      { relativePath: "cover.jpg", pathKey: "cover.jpg", mediaType: "image", sizeBytes: null, modifiedAt: null, confidence: "high", reasonCodes: [] },
    ];
    expect(libraryContentKind(value, null)).toBe("audio");
  });

  it("features a newer resume ahead of older launch activity", () => {
    const shelves = {
      installations: [{ ...installation(), id: "fallback" }],
      recent: [{ installationId: "recent", action: "play_audio", kind: "media_session", occurredAt: "2026-08-13T10:00:00Z", active: false }],
      continueItems: [{ installationId: "resume", action: "play_audio", relativePath: "01.mp3", positionMs: 1, durationMs: 2, completed: false, updatedAt: "2026-08-13T11:00:00Z" }],
      neverLaunched: ["new"],
      unfinished: [],
      launchTotals: [],
    } satisfies LibraryShelves;
    expect(featuredInstallationId(shelves)).toBe("resume");
  });

  it("features a newer launch ahead of older unfinished activity", () => {
    const shelves = {
      installations: [{ ...installation(), id: "fallback" }],
      recent: [{ installationId: "recent", action: "launch_executable", kind: "executable_launch", occurredAt: "2026-08-13T11:00:00Z", active: false }],
      continueItems: [{ installationId: "resume", action: "open_document", relativePath: "book.pdf", positionMs: 1, durationMs: 2, completed: false, updatedAt: "2026-08-13T10:00:00Z" }],
      neverLaunched: ["new"],
      unfinished: [],
      launchTotals: [],
    } satisfies LibraryShelves;
    expect(featuredInstallationId(shelves)).toBe("recent");
  });

  it("features the most relevant activity inside the active lens", () => {
    const shelves = {
      installations: [
        { ...installation(), id: "video" },
        { ...installation(), id: "audio" },
      ],
      recent: [
        { installationId: "video", action: "play_video", kind: "media_session", occurredAt: "2026-08-13T11:00:00Z", active: false },
        { installationId: "audio", action: "play_audio", kind: "media_session", occurredAt: "2026-08-13T10:00:00Z", active: false },
      ],
      continueItems: [
        { installationId: "video", action: "play_video", relativePath: "movie.mp4", positionMs: 1, durationMs: 2, completed: false, updatedAt: "2026-08-13T11:00:00Z" },
      ],
      neverLaunched: [],
      unfinished: [],
      launchTotals: [],
    } satisfies LibraryShelves;

    expect(featuredInstallationId(shelves, new Set(["audio"]))).toBe("audio");
  });
});

function installation(): Installation {
  return {
    id: "installation",
    scanRootId: null,
    rootPath: "/library/RJ00000001",
    platform: "linux",
    status: "ready",
    detection: {
      sourceScanSessionId: null,
      catalogIdentity: null,
      suggestedStatus: "ready",
      contentItems: [],
      launchCandidates: [],
      packageInspection: null,
    },
    overrides: {
      catalogIdentity: null,
      customTitle: null,
      preferredAction: null,
      contentItems: [],
      reviewedAt: "2026-08-13T10:00:00Z",
    },
    discoveredAt: "2026-08-13T10:00:00Z",
    updatedAt: "2026-08-13T10:00:00Z",
  };
}

describe("libraryLensCounts", () => {
  it("counts every kind under all and excludes other from its own lens", () => {
    const counts = libraryLensCounts(["audio", "audio", "apps", "other"]);
    expect(counts.get("all")).toBe(4);
    expect(counts.get("audio")).toBe(2);
    expect(counts.get("apps")).toBe(1);
    expect(counts.get("images")).toBeUndefined();
  });

  it("reports an empty library", () => {
    expect(libraryLensCounts([]).get("all")).toBe(0);
  });
});

describe("matchesLibraryLens", () => {
  it("keeps everything under the all lens", () => {
    expect(matchesLibraryLens("all", "other")).toBe(true);
    expect(matchesLibraryLens("all", "audio")).toBe(true);
  });

  it("hides unclassified works behind a specific lens", () => {
    expect(matchesLibraryLens("audio", "audio")).toBe(true);
    expect(matchesLibraryLens("audio", "other")).toBe(false);
    expect(matchesLibraryLens("apps", "audio")).toBe(false);
  });
});

describe("playTimeParts", () => {
  it("splits durations into whole hours and minutes", () => {
    expect(playTimeParts(0)).toEqual({ hours: 0, minutes: 0 });
    expect(playTimeParts(90_000)).toEqual({ hours: 0, minutes: 1 });
    expect(playTimeParts(67_320_000)).toEqual({ hours: 18, minutes: 42 });
  });

  it("never reports negative time", () => {
    expect(playTimeParts(-5_000)).toEqual({ hours: 0, minutes: 0 });
  });
});

describe("launchTotalsByInstallation", () => {
  it("indexes totals by installation", () => {
    const totals = launchTotalsByInstallation([
      { installationId: "game", launchCount: 4, totalDurationMs: 900_000 },
    ]);
    expect(totals.get("game")?.launchCount).toBe(4);
    expect(totals.get("absent")).toBeUndefined();
  });
});
