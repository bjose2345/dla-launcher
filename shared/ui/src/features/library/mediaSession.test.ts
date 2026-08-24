import { describe, expect, it } from "vitest";

import type { Installation, MediaSession, PreparedPackageInstallation } from "./types";
import {
  installationMediaAction,
  installationPrimaryAction,
  isMediaLaunchAction,
  mediaActionMessageKey,
  mediaItemName,
  mediaProgressPercent,
  mediaSessionTitleMessageKey,
  mediaStatusMessageKey,
} from "./mediaSession";

const session = {
  id: "media-1",
  installationId: "installation-1",
  kind: "work",
  action: "play_audio",
  status: "paused",
  repeatMode: "off",
  shuffle: false,
  items: [{
    ordinal: 0,
    installationId: "installation-1",
    workCode: "RJ00000001",
    relativePath: "disc/Track 01.flac",
    mediaType: "audio",
    sizeBytes: 12,
    discNumber: 1,
    trackNumber: 1,
    bonus: false,
  }],
  progress: {
    itemOrdinal: 0,
    positionMs: 30_000,
    durationMs: 120_000,
    completed: false,
    updatedAt: "2026-08-09T00:00:00Z",
  },
  openedAt: "2026-08-09T00:00:00Z",
  updatedAt: "2026-08-09T00:00:00Z",
  endedAt: null,
  error: null,
} satisfies MediaSession;

describe("library media session helpers", () => {
  it("recognizes only embedded player and reader actions", () => {
    expect(isMediaLaunchAction("play_audio")).toBe(true);
    expect(isMediaLaunchAction("read_images")).toBe(true);
    expect(isMediaLaunchAction("open_document")).toBe(true);
    expect(isMediaLaunchAction("play_video")).toBe(true);
    expect(isMediaLaunchAction("launch_executable")).toBe(false);
    expect(isMediaLaunchAction("open_archive")).toBe(false);
  });

  it("exposes only reviewed and ready media installations", () => {
    const installation = {
      id: "installation-1",
      scanRootId: null,
      rootPath: "/library/album",
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
        preferredAction: { action: "play_audio", target: { kind: "installation_root" } },
        contentItems: [],
        reviewedAt: "2026-08-09T00:00:00Z",
      },
      discoveredAt: "2026-08-09T00:00:00Z",
      updatedAt: "2026-08-09T00:00:00Z",
    } satisfies Installation;
    expect(installationMediaAction(installation, null)).toBe("play_audio");
    expect(installationMediaAction({
      ...installation,
      overrides: { ...installation.overrides, reviewedAt: null },
    }, null)).toBeNull();
    expect(installationMediaAction({
      ...installation,
      overrides: {
        ...installation.overrides,
        preferredAction: { action: "launch_executable", target: { kind: "installation_root" } },
      },
    }, null)).toBeNull();
  });

  it("uses the activated package action for prepared media", () => {
    const installation = {
      id: "installation-1",
      scanRootId: null,
      rootPath: "/library/archive.zip",
      platform: "linux",
      status: "ready",
      detection: {
        sourceScanSessionId: null,
        catalogIdentity: null,
        suggestedStatus: "ready",
        contentItems: [],
        launchCandidates: [],
        packageInspection: {} as Installation["detection"]["packageInspection"],
      },
      overrides: {
        catalogIdentity: null,
        customTitle: null,
        preferredAction: null,
        contentItems: [],
        reviewedAt: "2026-08-09T00:00:00Z",
      },
      discoveredAt: "2026-08-09T00:00:00Z",
      updatedAt: "2026-08-09T00:00:00Z",
    } satisfies Installation;
    const prepared = {
      preferredAction: { action: "read_images" },
    } as PreparedPackageInstallation;
    expect(installationMediaAction(installation, prepared)).toBe("read_images");
    expect(installationMediaAction(installation, null)).toBeNull();
  });

  it("projects executable actions for the Library without treating them as media", () => {
    const installation = {
      id: "installation-1",
      scanRootId: null,
      rootPath: "/library/game",
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
        preferredAction: { action: "launch_executable", target: { kind: "relative_path", path: "game" } },
        contentItems: [],
        reviewedAt: "2026-08-09T00:00:00Z",
      },
      discoveredAt: "2026-08-09T00:00:00Z",
      updatedAt: "2026-08-09T00:00:00Z",
    } satisfies Installation;
    expect(installationPrimaryAction(installation, null)).toBe("launch_executable");
    expect(installationMediaAction(installation, null)).toBeNull();
  });

  it("projects typed presentation keys", () => {
    expect(mediaActionMessageKey("play_audio")).toBe("media.action.listen");
    expect(mediaActionMessageKey("read_images")).toBe("media.action.read");
    expect(mediaActionMessageKey("play_video")).toBe("media.action.watch");
    expect(mediaActionMessageKey("open_document")).toBe("media.action.open");
    expect(mediaSessionTitleMessageKey(session)).toBe("media.player.audio");
    expect(mediaStatusMessageKey("paused")).toBe("media.status.paused");
  });

  it("formats item names and bounded progress", () => {
    expect(mediaItemName(session.items[0]!)).toBe("Track 01.flac");
    expect(mediaProgressPercent(session)).toBe(25);
    expect(mediaProgressPercent({
      ...session,
      progress: { ...session.progress, positionMs: 150_000 },
    })).toBe(100);
    expect(mediaProgressPercent({
      ...session,
      progress: { ...session.progress, durationMs: null },
    })).toBeNull();
  });

  it("tracks reader progress by ordered item and completion", () => {
    const reader = {
      ...session,
      action: "read_images" as const,
      items: [
        { ...session.items[0]!, ordinal: 0, relativePath: "001.webp", mediaType: "image" as const },
        { ...session.items[0]!, ordinal: 1, relativePath: "002.webp", mediaType: "image" as const },
        { ...session.items[0]!, ordinal: 2, relativePath: "003.webp", mediaType: "image" as const },
      ],
      progress: { ...session.progress, itemOrdinal: 1, durationMs: null, positionMs: 0 },
    };
    expect(mediaProgressPercent(reader)).toBeCloseTo(100 / 3);
    expect(mediaProgressPercent({
      ...reader,
      progress: { ...reader.progress, completed: true },
    })).toBe(100);
  });
});
