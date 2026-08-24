import { describe, expect, it } from "vitest";

import {
  formatShelfDate,
  recentActivityOpensMedia,
  shelfResumePercent,
} from "./LibraryShelves";
import type { LibraryRecentActivity, MediaResume } from "./types";

describe("library shelf presentation", () => {
  it("opens only built-in media activity in a media session", () => {
    expect(recentActivityOpensMedia(activity("media_session", "play_audio"))).toBe(true);
    expect(recentActivityOpensMedia(activity("media_session", "read_images"))).toBe(true);
    expect(recentActivityOpensMedia(activity("executable_launch", "launch_executable"))).toBe(false);
    expect(recentActivityOpensMedia(activity("media_session", "open_archive"))).toBe(false);
    expect(recentActivityOpensMedia(activity("media_session", null))).toBe(false);
  });

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

function activity(
  kind: LibraryRecentActivity["kind"],
  action: LibraryRecentActivity["action"],
): LibraryRecentActivity {
  return {
    installationId: "installation",
    kind,
    action,
    occurredAt: "2026-08-09T10:00:00Z",
    active: false,
  };
}

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
