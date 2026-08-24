import { describe, expect, it } from "vitest";

import { buildInstallationReviewDraft, installationReviewRequest, launchSelectionKey } from "./review";
import type { Installation } from "./types";

const installation: Installation = {
  id: "installation-root-1",
  scanRootId: "root-1",
  rootPath: "/synthetic/Game",
  platform: "windows",
  status: "needs_review",
  detection: {
    sourceScanSessionId: "session-1",
    catalogIdentity: { workCode: "RJ01326398", confidence: "strong", reasonCodes: ["code_in_path"] },
    suggestedStatus: "needs_review",
    contentItems: [{
      relativePath: "Game.exe",
      pathKey: "game.exe",
      mediaType: "executable",
      sizeBytes: 7,
      modifiedAt: null,
      confidence: "high",
      reasonCodes: ["file_extension"],
    }],
    launchCandidates: [{
      id: "game",
      action: "launch_executable",
      target: { kind: "relative_path", path: "Game.exe" },
      supportedPlatforms: ["windows"],
      confidence: "high",
      reasonCodes: ["preferred_executable_name"],
    }],
    packageInspection: null,
  },
  overrides: {
    catalogIdentity: null,
    customTitle: null,
    preferredAction: null,
    contentItems: [],
    reviewedAt: null,
  },
  discoveredAt: "2026-08-07T00:00:00Z",
  updatedAt: "2026-08-07T00:00:00Z",
};

describe("installation review", () => {
  it("turns review choices into explicit override contracts", () => {
    const draft = buildInstallationReviewDraft(installation);
    draft.identityMode = "catalog_work";
    draft.identityWorkCode = " rj01326398 ";
    draft.customTitle = " My game ";
    draft.preferredSelectionKey = launchSelectionKey({
      action: "launch_executable",
      target: { kind: "relative_path", path: "Game.exe" },
    });
    draft.content["Game.exe"] = { mediaType: "unknown", ignored: false, order: "2" };

    expect(installationReviewRequest(installation, draft)).toEqual({
      installationId: "installation-root-1",
      catalogIdentity: { kind: "catalog_work", workCode: "rj01326398" },
      customTitle: "My game",
      preferredAction: {
        action: "launch_executable",
        target: { kind: "relative_path", path: "Game.exe" },
      },
      contentItems: [{ relativePath: "Game.exe", mediaType: "unknown", ignored: false, order: 2 }],
    });
  });

  it("retains a missing preferred target as a review choice", () => {
    const withMissingTarget = {
      ...installation,
      overrides: {
        ...installation.overrides,
        preferredAction: {
          action: "launch_executable" as const,
          target: { kind: "relative_path" as const, path: "Missing.exe" },
        },
      },
    };
    const draft = buildInstallationReviewDraft(withMissingTarget);

    expect(installationReviewRequest(withMissingTarget, draft).preferredAction).toEqual(
      withMissingTarget.overrides.preferredAction,
    );
  });
});
