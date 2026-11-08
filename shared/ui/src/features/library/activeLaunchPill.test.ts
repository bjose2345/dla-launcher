import { describe, expect, it } from "vitest";

import { activeLaunch, launchPollInterval, quickExits } from "./ActiveLaunchPill";
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

describe("quickExits", () => {
  it("reports a game that ended by itself right after starting, once", () => {
    const handled = new Set<string>();
    const closed = { ...activity("a", "exited"), durationMs: 5_400 };

    expect(quickExits([closed], handled).map((launch) => launch.id)).toEqual(["a"]);
    expect(quickExits([closed], handled)).toEqual([]);
  });

  it("reports a game that crashed right after starting, but not one that never started", () => {
    const handled = new Set<string>();
    const launches = [
      { ...activity("crashed", "failed"), durationMs: 4_400, exitCode: -1, error: "process exited with code -1" },
      { ...activity("refused", "failed"), startedAt: null, error: "the file is missing" },
    ];

    expect(quickExits(launches, handled).map((launch) => launch.id)).toEqual(["crashed"]);
  });

  it("leaves games that ran a while, that the player stopped, or that still run", () => {
    const handled = new Set<string>();
    const launches = [
      { ...activity("played", "exited"), durationMs: 600_000 },
      { ...activity("stopped", "exited"), durationMs: 3_000, stopRequestedAt: "2026-08-13T10:00:03Z" },
      { ...activity("halted", "stopped"), durationMs: 3_000 },
      activity("running", "running"),
    ];

    expect(quickExits(launches, handled)).toEqual([]);
    // A game still running is looked at again once it ends.
    expect(handled.has("running")).toBe(false);
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

describe("launchPollInterval", () => {
  const running = { status: "running" } as Parameters<typeof launchPollInterval>[0] extends (infer T)[] | undefined ? T : never;
  const exited = { status: "exited" } as typeof running;

  it("polls often while a launch runs and now and then otherwise when there are no events", () => {
    expect(launchPollInterval([running], false)).toBe(750);
    expect(launchPollInterval([exited], false)).toBe(15_000);
    expect(launchPollInterval(undefined, false)).toBe(15_000);
  });

  it("keeps only a slow safety poll during a launch once events report changes", () => {
    expect(launchPollInterval([running], true)).toBe(5_000);
    expect(launchPollInterval([exited], true)).toBe(false);
    expect(launchPollInterval(undefined, true)).toBe(false);
  });
});
