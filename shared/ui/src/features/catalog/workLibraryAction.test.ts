import { describe, expect, it } from "vitest";

import type { Installation, PreparedPackageInstallation } from "../library/types";
import { resolveWorkLibraryAction, type WorkInstallationSnapshot } from "./workLibraryAction";

function installation(
  overrides: Partial<Installation> & { id: string },
): Installation {
  const { id, ...installationOverrides } = overrides;
  return {
    id,
    scanRootId: null,
    rootPath: `/library/${overrides.id}`,
    platform: "linux",
    status: "ready",
    detection: {
      sourceScanSessionId: null,
      catalogIdentity: {
        workCode: "RJ01326398",
        confidence: "exact",
        reasonCodes: ["archive_sha256_match"],
      },
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
      reviewedAt: null,
    },
    discoveredAt: "2026-08-09T00:00:00Z",
    updatedAt: "2026-08-09T00:00:00Z",
    ...installationOverrides,
  };
}

function snapshot(
  value: Installation,
  prepared: PreparedPackageInstallation | null = null,
): WorkInstallationSnapshot {
  return { installation: value, prepared };
}

function packageInstallation(id: string, safety: "safe" | "unsafe" = "safe"): Installation {
  const value = installation({ id });
  return {
    ...value,
    detection: {
      ...value.detection,
      packageInspection: {
        source: {
          scanEntryId: "archive-entry",
          kind: "archive",
          relativePath: "RJ01326398.zip",
          sizeBytes: 100,
          sha256: "fixture",
        },
        format: "zip",
        safety,
        entryCount: 1,
        fileCount: 1,
        directoryCount: 0,
        totalCompressedBytes: 100,
        totalUncompressedBytes: 200,
        commonRoot: null,
        issues: [],
        classification: {
          contentKind: "windows_game",
          engine: null,
          platform: "windows",
          confidence: "high",
          reasonCodes: ["executable_in_archive"],
          contentRoot: null,
          launchCandidates: [],
        },
        installPlan: {
          requiresExtraction: true,
          contentRoot: null,
          preferredAction: null,
          archiveRetention: "keep",
        },
        inspectedAt: "2026-08-09T00:00:00Z",
      },
    },
  };
}

describe("catalog work library action", () => {
  it("starts at the scanner when no local installation matches", () => {
    expect(resolveWorkLibraryAction([])).toEqual({ kind: "scan" });
  });

  it("opens safe archives in the existing preparation workflow", () => {
    expect(resolveWorkLibraryAction([snapshot(packageInstallation("package"))]))
      .toEqual({ kind: "install", installationId: "package" });
  });

  it("requires review for an unsafe package", () => {
    expect(resolveWorkLibraryAction([snapshot(packageInstallation("unsafe", "unsafe"))]))
      .toEqual({ kind: "review", installationId: "unsafe" });
  });

  it("projects a verified prepared action as play-ready", () => {
    const value = packageInstallation("prepared");
    value.overrides.reviewedAt = "2026-08-09T00:00:00Z";
    const prepared = {
      installationId: value.id,
      destinationRoot: "/library/prepared",
      contentRoot: null,
      preferredAction: {
        action: "launch_executable" as const,
        relativePath: "Game.exe",
        supportedPlatforms: ["windows" as const],
        confidence: "high" as const,
        reasonCodes: ["preferred_executable_name"],
      },
      sourceSet: {
        kind: "single_archive" as const,
        volumes: [],
      },
      archiveRetention: "keep" as const,
      sourcesDeleted: false,
      sourceCleanupError: null,
      installedFileCount: 1,
      installedBytes: 200,
      preparedAt: "2026-08-09T00:00:00Z",
    } satisfies PreparedPackageInstallation;

    expect(resolveWorkLibraryAction([snapshot(value, prepared)]))
      .toEqual({
        kind: "play",
        installationId: "prepared",
        action: "launch_executable",
      });
  });

  it("preserves a prepared media action for player routing", () => {
    const value = packageInstallation("album");
    value.overrides.reviewedAt = "2026-08-09T00:00:00Z";
    const prepared = {
      installationId: value.id,
      destinationRoot: "/library/album",
      contentRoot: "audio",
      preferredAction: {
        action: "play_audio" as const,
        relativePath: "audio",
        supportedPlatforms: ["linux" as const],
        confidence: "high" as const,
        reasonCodes: ["audio_collection"],
      },
      sourceSet: { kind: "single_archive" as const, volumes: [] },
      archiveRetention: "keep" as const,
      sourcesDeleted: false,
      sourceCleanupError: null,
      installedFileCount: 2,
      installedBytes: 200,
      preparedAt: "2026-08-09T00:00:00Z",
    } satisfies PreparedPackageInstallation;

    expect(resolveWorkLibraryAction([snapshot(value, prepared)]))
      .toEqual({ kind: "play", installationId: "album", action: "play_audio" });
  });

  it("does not claim a prepared package is playable without an action", () => {
    const value = packageInstallation("installed");
    value.overrides.reviewedAt = "2026-08-09T00:00:00Z";
    const prepared = {
      installationId: value.id,
      destinationRoot: "/library/installed",
      contentRoot: null,
      preferredAction: null,
      sourceSet: {
        kind: "single_archive" as const,
        volumes: [],
      },
      archiveRetention: "keep" as const,
      sourcesDeleted: false,
      sourceCleanupError: null,
      installedFileCount: 1,
      installedBytes: 200,
      preparedAt: "2026-08-09T00:00:00Z",
    } satisfies PreparedPackageInstallation;

    expect(resolveWorkLibraryAction([snapshot(value, prepared)]))
      .toEqual({ kind: "installed", installationId: "installed" });
  });

  it("returns an unreviewed prepared package to the review checkpoint", () => {
    const value = packageInstallation("unreviewed");
    const prepared = {
      installationId: value.id,
      destinationRoot: "/library/unreviewed",
      contentRoot: null,
      preferredAction: {
        action: "launch_executable" as const,
        relativePath: "Game.exe",
        supportedPlatforms: ["windows" as const],
        confidence: "high" as const,
        reasonCodes: ["preferred_executable_name"],
      },
      sourceSet: { kind: "single_archive" as const, volumes: [] },
      archiveRetention: "keep" as const,
      sourcesDeleted: false,
      sourceCleanupError: null,
      installedFileCount: 1,
      installedBytes: 200,
      preparedAt: "2026-08-09T00:00:00Z",
    } satisfies PreparedPackageInstallation;

    expect(resolveWorkLibraryAction([snapshot(value, prepared)]))
      .toEqual({ kind: "review", installationId: "unreviewed" });
  });

  it("does not launch a generated candidate without an explicit manual selection", () => {
    const value = installation({ id: "generated" });
    value.overrides.reviewedAt = "2026-08-09T00:00:00Z";
    value.detection.launchCandidates = [{
      id: "candidate-1",
      action: "launch_executable",
      target: { kind: "relative_path", path: "Game.exe" },
      supportedPlatforms: ["windows"],
      confidence: "high",
      reasonCodes: ["preferred_executable_name"],
    }];

    expect(resolveWorkLibraryAction([snapshot(value)]))
      .toEqual({ kind: "review", installationId: "generated" });
  });
});
