// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { ActiveLaunchPill, type ActiveLaunchGateway } from "./ActiveLaunchPill";
import type { Installation, LaunchActivity, LaunchActivityStatus } from "./types";

afterEach(() => {
  cleanup();
});

describe("ActiveLaunchPill", () => {
  it("says when a game closed right after it started and starts it again", async () => {
    const running = launch("running", null);
    const closed = { ...launch("exited", 5_400), endedAt: "2026-10-30T10:00:06Z" };
    const gateway = {
      listRecentLaunches: vi.fn()
        .mockResolvedValueOnce([])
        .mockResolvedValueOnce([running])
        .mockResolvedValue([closed]),
      launchInstallation: vi.fn().mockResolvedValue(launch("starting", null)),
      readInstallation: vi.fn().mockResolvedValue({
        overrides: { customTitle: "Yukina" },
      } as unknown as Installation),
    } satisfies ActiveLaunchGateway;
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <PresentationProvider>
          <ActiveLaunchPill gateway={gateway} onOpenLibrary={() => undefined} />
        </PresentationProvider>
      </QueryClientProvider>,
    );
    // The launch the player starts shows up on the next look.
    await waitFor(() => expect(gateway.listRecentLaunches).toHaveBeenCalledOnce());
    await client.invalidateQueries({ queryKey: ["library", "launches"] });

    expect(await screen.findByText("Yukina closed 5 seconds after it started.", {}, { timeout: 3_000 })).toBeTruthy();
    expect(screen.getByText("If no window opened, the game may not run on this PC.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));

    await waitFor(() => expect(gateway.launchInstallation).toHaveBeenCalledWith("installation-1"));
    expect(screen.queryByText("Yukina closed 5 seconds after it started.")).toBeNull();
  });
});

describe("ActiveLaunchPill with launch events", () => {
  it("asks for the launches again when an event reports a change and stops listening on unmount", async () => {
    let emit: ((activity: LaunchActivity) => void) | undefined;
    const unsubscribe = vi.fn();
    const gateway = {
      listRecentLaunches: vi.fn().mockResolvedValue([]),
      subscribeLaunchActivity: vi.fn(async (listener: (activity: LaunchActivity) => void) => {
        emit = listener;
        return unsubscribe;
      }),
      launchInstallation: vi.fn(),
      readInstallation: vi.fn(),
    } satisfies ActiveLaunchGateway;
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const view = render(
      <QueryClientProvider client={client}>
        <PresentationProvider>
          <ActiveLaunchPill gateway={gateway} onOpenLibrary={() => undefined} />
        </PresentationProvider>
      </QueryClientProvider>,
    );
    await waitFor(() => expect(gateway.listRecentLaunches).toHaveBeenCalledOnce());
    await waitFor(() => expect(emit).toBeTypeOf("function"));

    emit?.(launch("running", null));
    await waitFor(() => expect(gateway.listRecentLaunches).toHaveBeenCalledTimes(2));
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: ["library", "shelves"] });

    emit?.(launch("exited", 65_000));
    await waitFor(() => expect(gateway.listRecentLaunches).toHaveBeenCalledTimes(3));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["library", "shelves"] });

    view.unmount();
    expect(unsubscribe).toHaveBeenCalledOnce();
  });
});

function launch(status: LaunchActivityStatus, durationMs: number | null): LaunchActivity {
  return {
    id: "launch-1",
    installationId: "installation-1",
    action: "launch_executable",
    targetPath: "Game.exe",
    adapter: "linux_wine",
    status,
    processId: 42,
    error: null,
    attemptedAt: "2026-10-30T10:00:00Z",
    startedAt: "2026-10-30T10:00:01Z",
    endedAt: null,
    durationMs,
    exitCode: status === "exited" ? 0 : null,
    stopRequestedAt: null,
  };
}
