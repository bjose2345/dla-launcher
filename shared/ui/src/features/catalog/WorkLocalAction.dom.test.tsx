// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import { PresentationProvider } from "../../preferences/PresentationProvider";
import { MediaPlaybackProvider } from "../library/MediaPlaybackProvider";
import type { Installation, LaunchActivity, LibraryGateway, LibraryLaunchTotals } from "../library/types";
import { WorkLibraryRow, WorkLocalActionButton, WorkPlayedRows } from "./WorkDetailPage";

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  vi.useRealTimers();
});

const INSTALLATION = "installation-1";

describe("a work's launch block outside the Library", () => {
  it("shows play time in the data table and no list of launches", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 10, 17, 12));
    const onOpenLibrary = vi.fn();
    renderBlock({
      totals: { installationId: INSTALLATION, launchCount: 14, totalDurationMs: 18_720_000, lastStartedAt: new Date(2026, 10, 16, 19, 34).toISOString() },
      launches: [launch("exited")],
      onOpenLibrary,
    });

    await waitFor(() => expect(rows()).toEqual([
      ["Played", "5 h 12 m"],
      ["Last played", "Yesterday"],
      ["Library", "Open →"],
    ]));
    expect(screen.getByRole("button", { name: /^Play/ })).toBeTruthy();
    expect(screen.queryByText("Recent launches")).toBeNull();
    expect(screen.queryByText(/Exited after/)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /^Open in Library/ }));
    expect(onOpenLibrary).toHaveBeenCalledWith(INSTALLATION);
  });

  it("says a running work is running and offers Stop", async () => {
    renderBlock({
      totals: { installationId: INSTALLATION, launchCount: 2, totalDurationMs: 600_000, lastStartedAt: "2026-11-10T10:00:00Z" },
      launches: [launch("running")],
    });

    await waitFor(() => expect(rows()).toEqual([["Played", "10 m"], ["Last played", "Now…"]]));
    expect(document.querySelector(".is-running dd")?.textContent).toBe("Now…");
    expect(screen.getByRole("button", { name: /^Stop/ })).toBeTruthy();
  });

  it("has no play rows for a work never started, and none when play time is hidden", async () => {
    renderBlock({ totals: null, launches: [] });
    await waitFor(() => expect(screen.getByRole("button", { name: /^Play/ })).toBeTruthy());
    expect(rows()).toEqual([]);
    cleanup();

    window.localStorage.setItem(PREFERENCES_KEY, JSON.stringify({ showPlayTime: false }));
    renderBlock({
      totals: { installationId: INSTALLATION, launchCount: 3, totalDurationMs: 600_000, lastStartedAt: "2026-11-10T10:00:00Z" },
      launches: [launch("exited")],
    });
    await waitFor(() => expect(screen.getByRole("button", { name: /^Play/ })).toBeTruthy());
    expect(rows()).toEqual([]);
  });

  it("keeps the error of a start that failed, and has no Library row inside the Library", async () => {
    renderBlock({ totals: null, launches: [{ ...launch("failed"), error: "wine is not installed" }] });

    expect((await screen.findByRole("alert")).textContent).toBe("Could not start this work: wine is not installed");
    expect(screen.queryByRole("button", { name: /^Open in Library/ })).toBeNull();
  });
});

const PREFERENCES_KEY = "dla-launcher:presentation:v1";

function rows(): string[][] {
  return [...document.querySelectorAll("dl > div")].map((row) => [
    row.querySelector("dt")?.textContent ?? "",
    row.querySelector("dd")?.textContent ?? "",
  ]);
}

function renderBlock({
  totals,
  launches,
  onOpenLibrary,
}: {
  totals: LibraryLaunchTotals | null;
  launches: LaunchActivity[];
  onOpenLibrary?: (installationId: string) => void;
}) {
  const gateway = {
    listInstallationsForWork: vi.fn().mockResolvedValue([installation()]),
    readPreparedPackages: vi.fn().mockResolvedValue([]),
    readShelves: vi.fn().mockResolvedValue({
      installations: [],
      recent: [],
      continueItems: [],
      neverLaunched: [],
      unfinished: [],
      launchTotals: totals ? [totals] : [],
    }),
    listInstallationLaunchHistory: vi.fn().mockResolvedValue(launches),
    launchInstallation: vi.fn(),
    stopLaunch: vi.fn(),
  } as unknown as LibraryGateway;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <KeyBindingsProvider>
          <MediaPlaybackProvider gateway={gateway}>
            <dl>
              <WorkPlayedRows workCode="RJ00000001" gateway={gateway} />
              {onOpenLibrary ? (
                <WorkLibraryRow workCode="RJ00000001" gateway={gateway} onOpen={onOpenLibrary} />
              ) : null}
            </dl>
            <WorkLocalActionButton
              workCode="RJ00000001"
              gateway={gateway}
              onOpenScanner={vi.fn()}
              onOpenInstallation={vi.fn()}
              onOpenMedia={vi.fn()}
              onOpenExternal={vi.fn()}
            />
          </MediaPlaybackProvider>
        </KeyBindingsProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function launch(status: LaunchActivity["status"]): LaunchActivity {
  return {
    id: `launch-${status}`,
    installationId: INSTALLATION,
    action: "launch_executable",
    targetPath: "game",
    adapter: null,
    status,
    processId: 4242,
    error: null,
    attemptedAt: "2026-11-16T19:34:00Z",
    startedAt: "2026-11-16T19:34:00Z",
    endedAt: status === "running" ? null : "2026-11-16T19:39:50Z",
    durationMs: status === "running" ? null : 350_000,
    exitCode: status === "exited" ? 0 : null,
    stopRequestedAt: null,
  };
}

function installation(): Installation {
  return {
    id: INSTALLATION,
    scanRootId: null,
    rootPath: "/library/RJ00000001",
    platform: "linux",
    status: "ready",
    detection: {
      sourceScanSessionId: null,
      catalogIdentity: { workCode: "RJ00000001", confidence: "exact", reasonCodes: [] },
      suggestedStatus: "ready",
      contentItems: [],
      launchCandidates: [],
      packageInspection: null,
    },
    overrides: {
      catalogIdentity: null,
      customTitle: "RJ00000001",
      preferredAction: {
        action: "launch_executable",
        target: { kind: "relative_path", path: "game" },
      } as Installation["overrides"]["preferredAction"],
      contentItems: [],
      reviewedAt: "2026-08-15T00:00:00Z",
    },
    discoveredAt: "2026-08-15T00:00:00Z",
    updatedAt: "2026-08-15T00:00:00Z",
  };
}
