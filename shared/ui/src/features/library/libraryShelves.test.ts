import { describe, expect, it } from "vitest";

import {
  formatShelfDate,
  shelfResumePercent,
} from "./LibraryShelves";
import type { MediaResume } from "./types";

describe("library shelf presentation", () => {
  it("bounds determinate media progress and leaves readers indeterminate", () => {
    expect(shelfResumePercent(resume(25_000, 100_000))).toBe(25);
    expect(shelfResumePercent(resume(120_000, 100_000))).toBe(100);
    expect(shelfResumePercent(resume(-1, 100_000))).toBe(0);
    expect(shelfResumePercent(resume(1, null))).toBeNull();
  });

  it("preserves malformed timestamps instead of throwing", () => {
    expect(formatShelfDate("not-a-date", "en-US")).toBe("not-a-date");
  });
});

function resume(positionMs: number, durationMs: number | null): MediaResume {
  return {
    installationId: "installation",
    action: "play_audio",
    relativePath: "track.flac",
    positionMs,
    durationMs,
    completed: false,
    updatedAt: "2026-08-09T10:00:00Z",
  };
}
