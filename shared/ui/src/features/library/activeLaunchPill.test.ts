import { describe, expect, it } from "vitest";

import { activeLaunch } from "./ActiveLaunchPill";
import type { LaunchActivity, LaunchActivityStatus } from "./types";

describe("activeLaunch", () => {
  it("finds the first launcher-owned process that is still live", () => {
    expect(activeLaunch([activity("a", "exited"), activity("b", "running")])?.id).toBe("b");
    expect(activeLaunch([activity("a", "starting")])?.id).toBe("a");
    expect(activeLaunch([activity("a", "stopping")])?.id).toBe("a");
  });

  it("ignores terminal activity so a finished game never keeps the pill open", () => {
    const terminal: LaunchActivityStatus[] = ["exited", "failed", "stopped", "interrupted"];
    expect(activeLaunch(terminal.map((status) => activity(status, status)))).toBeNull();
    expect(activeLaunch([])).toBeNull();
  });
});

function activity(id: string, status: LaunchActivityStatus): LaunchActivity {
  return {
    id,
    installationId: "installation",
    action: "launch_executable",
    targetPath: "Game.exe",
    adapter: "linux_wine",
    status,
    processId: 42,
    error: null,
    attemptedAt: "2026-08-13T10:00:00Z",
    startedAt: "2026-08-13T10:00:01Z",
    endedAt: null,
    durationMs: null,
    exitCode: null,
    stopRequestedAt: null,
  };
}
