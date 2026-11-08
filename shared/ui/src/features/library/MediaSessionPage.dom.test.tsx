// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import { PresentationProvider } from "../../preferences/PresentationProvider";
import { MediaPlaybackProvider } from "./MediaPlaybackProvider";
import { MediaSessionPage } from "./MediaSessionPage";
import { ReaderProvider } from "./ReaderProvider";
import type { LibraryGateway, MediaSession } from "./types";

afterEach(cleanup);

describe("MediaSessionPage", () => {
  it("opens the work's session again on the next visit, as the last one may be closed by then", async () => {
    const gateway = {
      readInstallation: vi.fn(() => new Promise(() => {})),
      openMediaSession: vi.fn().mockResolvedValue(session()),
    } as unknown as LibraryGateway;
    // The app keeps queries fresh indefinitely (tauri2/ui/src/main.tsx).
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Number.POSITIVE_INFINITY } },
    });
    const page = () => (
      <QueryClientProvider client={client}>
        <PresentationProvider>
          <KeyBindingsProvider>
            <MediaPlaybackProvider gateway={gateway}>
              <ReaderProvider gateway={gateway}>
                <MediaSessionPage installationId="installation-1" gateway={gateway} onBack={vi.fn()} />
              </ReaderProvider>
            </MediaPlaybackProvider>
          </KeyBindingsProvider>
        </PresentationProvider>
      </QueryClientProvider>
    );

    const first = render(page());
    await waitFor(() => expect(gateway.openMediaSession).toHaveBeenCalledTimes(1));
    first.unmount();
    await new Promise((resolve) => setTimeout(resolve, 0));
    render(page());

    await waitFor(() => expect(gateway.openMediaSession).toHaveBeenCalledTimes(2));
  });
});

function session(): MediaSession {
  return {
    id: "session-1",
    kind: "work",
    installationId: "installation-1",
    action: "play_video",
    status: "active",
    repeatMode: "off",
    shuffle: false,
    items: [],
    progress: { itemOrdinal: 0, positionMs: 0, durationMs: null, completed: false, updatedAt: "2026-11-01T00:00:00Z" },
    openedAt: "2026-11-01T00:00:00Z",
    updatedAt: "2026-11-01T00:00:00Z",
    endedAt: null,
    error: null,
  };
}
