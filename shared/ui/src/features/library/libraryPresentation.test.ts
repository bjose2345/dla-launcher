import { describe, expect, it } from "vitest";

import {
  installationFolder,
  installationName,
  installationTitle,
  pathInInstallationFolder,
} from "./libraryPresentation";
import type { Installation } from "./types";

function installation(paths: string[], rootPath = "D:\\Games\\DLsite"): Installation {
  return {
    id: "installation-1",
    scanRootId: "root-1",
    rootPath,
    platform: "windows",
    status: "ready",
    detection: {
      sourceScanSessionId: null,
      catalogIdentity: null,
      suggestedStatus: "ready",
      contentItems: paths.map((relativePath) => ({
        relativePath,
        pathKey: relativePath.toLowerCase(),
        mediaType: "executable",
        sizeBytes: 1,
        modifiedAt: null,
        confidence: "high",
        reasonCodes: ["fixture"],
      })),
      launchCandidates: [],
      packageInspection: null,
    },
    overrides: {
      catalogIdentity: { kind: "unidentified" },
      customTitle: null,
      preferredAction: null,
      contentItems: [],
      reviewedAt: "2026-10-08T00:00:00Z",
    },
    discoveredAt: "2026-10-08T00:00:00Z",
    updatedAt: "2026-10-08T00:00:00Z",
  };
}

describe("library presentation", () => {
  it("names an unknown work after its own folder inside a shared scan root", () => {
    const work = installation(["old_backup/game_v2/Game.exe", "old_backup/game_v2/www/data.json"]);

    expect(installationTitle(work)).toBe("old_backup");
    expect(installationFolder(work)).toBe("D:\\Games\\DLsite\\old_backup");
    expect(pathInInstallationFolder(work, "old_backup/game_v2/Game.exe")).toBe("game_v2/Game.exe");
  });

  it("names a single file without its extension, and a whole root after the root", () => {
    expect(installationName(installation(["voice.mp3"]))).toBe("voice");
    const root = installation(["Game.exe", "data/map.dat"], "/home/developer/Games/KUNOICHI RED");
    expect(installationName(root)).toBe("KUNOICHI RED");
    expect(installationFolder(root)).toBe("/home/developer/Games/KUNOICHI RED");
    expect(pathInInstallationFolder(root, "data/map.dat")).toBe("data/map.dat");
  });

  it("prefers a custom title", () => {
    const work = installation(["old_backup/Game.exe"]);
    work.overrides.customTitle = "  My backup  ";

    expect(installationTitle(work)).toBe("My backup");
  });
});
