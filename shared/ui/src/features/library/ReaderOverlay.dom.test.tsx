// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import { ImageReaderProvider, useImageReader, type ImageReaderGateway } from "./ImageReaderProvider";
import { ReaderOverlay } from "./ReaderOverlay";
import type { Installation, LibraryGateway, MediaSession } from "./types";

afterEach(() => {
  cleanup();
  document.body.style.overflow = "";
  document.documentElement.style.overflow = "";
});

describe("ReaderOverlay", () => {
  it("traps focus, locks scrolling, closes with Escape, and restores focus", async () => {
    const session = documentSession();
    const gateway: ImageReaderGateway = {
      openMediaSession: vi.fn().mockResolvedValue(session),
      closeMediaSession: vi.fn().mockResolvedValue(session),
      updateMediaProgress: vi.fn().mockResolvedValue(session),
      readInstallation: vi.fn().mockResolvedValue(readerInstallation()),
      mediaAssetUrl: vi.fn().mockReturnValue("data:application/pdf;base64,JVBERi0="),
    };
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={client}>
        <PresentationProvider>
          <KeyBindingsProvider>
            <ImageReaderProvider gateway={gateway}>
              <OpenReader />
              <ReaderOverlay gateway={gateway as LibraryGateway} />
            </ImageReaderProvider>
          </KeyBindingsProvider>
        </PresentationProvider>
      </QueryClientProvider>,
    );

    const opener = screen.getByRole("button", { name: "Open test reader" });
    opener.focus();
    fireEvent.click(opener);

    const dialog = await screen.findByRole("dialog", { name: /Document reader/ });
    await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true));
    expect(document.body.style.overflow).toBe("hidden");
    expect(document.documentElement.style.overflow).toBe("hidden");

    const buttons = [...dialog.querySelectorAll<HTMLButtonElement>("button:not([disabled])")];
    expect(buttons.length).toBeGreaterThan(1);
    const first = buttons[0]!;
    const last = buttons[buttons.length - 1]!;
    last.focus();
    fireEvent.keyDown(last, { key: "Tab" });
    expect(document.activeElement).toBe(first);

    fireEvent.keyDown(first, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(document.body.style.overflow).toBe("");
      expect(document.documentElement.style.overflow).toBe("");
      expect(document.activeElement).toBe(opener);
    });
  });
});

function OpenReader() {
  const reader = useImageReader();
  return <button type="button" onClick={() => void reader.open("installation-1")}>Open test reader</button>;
}

function documentSession(): MediaSession {
  return {
    id: "session-1",
    kind: "work",
    installationId: "installation-1",
    action: "open_document",
    status: "active",
    repeatMode: "off",
    shuffle: false,
    items: [{
      ordinal: 0,
      installationId: "installation-1",
      workCode: "RJ00000001",
      relativePath: "book.pdf",
      mediaType: "pdf",
      sizeBytes: 128,
      discNumber: null,
      trackNumber: null,
      bonus: false,
    }],
    progress: {
      itemOrdinal: 0,
      positionMs: 0,
      durationMs: null,
      completed: false,
      updatedAt: "2026-08-15T00:00:00Z",
    },
    openedAt: "2026-08-15T00:00:00Z",
    updatedAt: "2026-08-15T00:00:00Z",
    endedAt: null,
    error: null,
  };
}

function readerInstallation(): Installation {
  return {
    id: "installation-1",
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
      customTitle: "Test document",
      preferredAction: null,
      contentItems: [],
      reviewedAt: "2026-08-15T00:00:00Z",
    },
    discoveredAt: "2026-08-15T00:00:00Z",
    updatedAt: "2026-08-15T00:00:00Z",
  };
}
