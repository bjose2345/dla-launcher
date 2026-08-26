import { describe, expect, it } from "vitest";

import {
  archivePolicyMessageKey,
  catalogProfileMessageKey,
  confidenceMessageKey,
  evidenceReasonMessageKey,
  generationKindMessageKey,
  launchActionMessageKey,
  launchAdapterMessageKey,
  mediaTypeMessageKey,
  packageContentMessageKey,
  platformMessageKey,
  sourceSetMessageKey,
} from "./domainLabels";

describe("domain label localization", () => {
  it("maps stable backend values to message keys", () => {
    expect(platformMessageKey("linux")).toBe("domain.platform.linux");
    expect(confidenceMessageKey("exact")).toBe("domain.confidence.exact");
    expect(mediaTypeMessageKey("audio")).toBe("domain.media.audio");
    expect(launchActionMessageKey("play_audio")).toBe("domain.action.playAudio");
    expect(launchAdapterMessageKey("linux_wine")).toBe("domain.adapter.linuxWine");
    expect(packageContentMessageKey("windows_game")).toBe("domain.package.windowsGame");
    expect(sourceSetMessageKey("multipart_rar")).toBe("domain.sourceSet.multipartRar");
    expect(archivePolicyMessageKey("keep")).toBe("domain.archivePolicy.keep");
    expect(catalogProfileMessageKey("full")).toBe("domain.profile.full");
    expect(generationKindMessageKey("imported")).toBe("domain.generation.imported");
    expect(evidenceReasonMessageKey("archive_sha256_match"))
      .toBe("domain.evidence.archiveHashMatch");
  });

  it("uses deliberate localized labels for unknown stable values", () => {
    expect(platformMessageKey("plan9")).toBe("domain.platform.unknown");
    expect(confidenceMessageKey("unexpected")).toBe("domain.confidence.unknown");
    expect(evidenceReasonMessageKey("future_reason")).toBe("domain.evidence.observed");
  });
});
