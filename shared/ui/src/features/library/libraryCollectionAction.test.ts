import { describe, expect, it } from "vitest";

import { collectionState, type LibraryCollectionEntry } from "./LibraryCollection";
import type { Installation, LaunchActivityStatus } from "./types";

describe("collection card state", () => {
  it("reports a live process before anything else", () => {
    expect(collectionState(entry({ status: "running" }))).toBe("running");
  });

  it("reports review only when no process is live", () => {
    expect(collectionState(entry({ needsReview: true }))).toBe("review");
    expect(collectionState(entry({ needsReview: true, status: "running" }))).toBe("running");
  });

  it("calls a work new only when it has no resume and no launch", () => {
    expect(collectionState(entry({}))).toBe("new");
    expect(collectionState(entry({ status: "exited" }))).toBeNull();
    expect(collectionState(entry({ resume: true }))).toBeNull();
  });

  it("surfaces unhealthy state before review or new state", () => {
    expect(collectionState(entry({ health: "moved" }))).toBe("moved");
    expect(collectionState(entry({ health: "repairable", needsReview: true }))).toBe("repairable");
    expect(collectionState(entry({ health: "missing_files", status: "running" }))).toBe("running");
  });
});

function entry({
  status,
  needsReview = false,
  resume = false,
  health,
}: {
  status?: LaunchActivityStatus;
  needsReview?: boolean;
  resume?: boolean;
  health?: "moved" | "repairable" | "missing_files";
}): LibraryCollectionEntry {
  const installation = {
    id: "work",
    scanRootId: null,
    rootPath: "/library/work",
    platform: "linux",
    status: needsReview ? "needs_review" : "ready",
    detection: {
      sourceScanSessionId: null,
      catalogIdentity: null,
      suggestedStatus: needsReview ? "needs_review" : "ready",
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
  } as Installation;

  return {
    installation,
    action: "read_images",
    health: health
      ? {
        installationId: "work",
        state: health,
        managed: true,
        repairable: health === "repairable",
        checkedRoot: "/library/work",
        expectedFiles: 1,
        presentFiles: 0,
        missingFiles: 1,
        modifiedFiles: 0,
        inaccessibleFiles: 0,
        unexpectedFiles: 0,
        issues: [],
        checkedAt: "2026-08-16T10:00:00Z",
      }
      : null,
    resume: resume
      ? {
        installationId: "work",
        action: "read_images",
        relativePath: "p1.png",
        positionMs: 1,
        durationMs: 2,
        completed: false,
        updatedAt: "2026-08-13T10:00:00Z",
      }
      : null,
    latestLaunch: status
      ? {
        id: "activity",
        installationId: "work",
        action: "read_images",
        targetPath: null,
        adapter: null,
        status,
        processId: null,
        error: null,
        attemptedAt: "2026-08-13T10:00:00Z",
        startedAt: "2026-08-13T10:00:00Z",
        endedAt: null,
        durationMs: null,
        exitCode: null,
        stopRequestedAt: null,
      }
      : null,
    launchTotals: null,
  };
}
