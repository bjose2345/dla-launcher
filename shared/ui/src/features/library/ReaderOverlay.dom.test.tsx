// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import { openPdfDocument, type PdfDocument } from "./pdfDocument";
import { ReaderProvider, useReader, type ReaderGateway } from "./ReaderProvider";
import { ReaderOverlay } from "./ReaderOverlay";
import type { Installation, LibraryGateway, MediaSession } from "./types";

vi.mock("./pdfDocument", () => ({ openPdfDocument: vi.fn() }));

const PAGES = 5;

beforeEach(() => {
  vi.mocked(openPdfDocument).mockResolvedValue(fakePdf(PAGES));
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  document.body.style.overflow = "";
  document.documentElement.style.overflow = "";
});

describe("ReaderOverlay", () => {
  it("traps focus, locks scrolling, closes with Escape, and restores focus", async () => {
    const { gateway } = renderReader(documentSession());

    const opener = screen.getByRole("button", { name: "Open test reader" });
    opener.focus();
    fireEvent.click(opener);

    const dialog = await screen.findByRole("dialog", { name: /Document reader/ });
    await waitFor(() => expect(dialog.querySelector(".reader")).toBe(document.activeElement));
    expect(document.body.style.overflow).toBe("hidden");
    expect(document.documentElement.style.overflow).toBe("hidden");
    await screen.findByText("1 / 5");

    const buttons = await waitFor(() => {
      const found = [...dialog.querySelectorAll<HTMLElement>("button:not([disabled]):not([tabindex='-1']), input:not([disabled])")];
      expect(found.length).toBeGreaterThan(1);
      return found;
    });
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
    expect(gateway.closeMediaSession).toHaveBeenCalledWith("session-1");
  });

  it("opens where the reading stopped, turns pages with the keys, and saves the place", async () => {
    const { gateway } = renderReader(documentSession({ positionMs: 3, durationMs: PAGES }));
    fireEvent.click(screen.getByRole("button", { name: "Open test reader" }));

    expect(await screen.findByText("3 / 5")).toBeTruthy();
    expect(screen.getByText("Continuing from page 3")).toBeTruthy();

    fireEvent.keyDown(document.activeElement ?? window, { key: "ArrowRight" });
    await waitFor(() => expect(gateway.updateMediaProgress).toHaveBeenLastCalledWith({
      sessionId: "session-1",
      itemOrdinal: 0,
      positionMs: 4,
      durationMs: PAGES,
      completed: false,
      status: "active",
    }), { timeout: 2_000 });
    expect(screen.queryByText("Continuing from page 3")).toBeNull();

    // Turning the page hid the bars; a click in the middle brings them back.
    expect(screen.queryByText("4 / 5")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show or hide the controls" }));
    expect(screen.getByText("4 / 5")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Page 1" }));
    expect(await screen.findByText("1 / 5")).toBeTruthy();
  });

  it("marks the work finished on its last page and offers to read it again", async () => {
    const { gateway } = renderReader(documentSession());
    fireEvent.click(screen.getByRole("button", { name: "Open test reader" }));
    await screen.findByText("1 / 5");

    fireEvent.keyDown(document.activeElement ?? window, { key: "End" });
    expect(await screen.findByText("5 / 5")).toBeTruthy();
    await waitFor(() => expect(gateway.updateMediaProgress).toHaveBeenCalledWith(expect.objectContaining({
      positionMs: PAGES,
      durationMs: PAGES,
      completed: true,
      status: "completed",
    })));

    fireEvent.keyDown(document.activeElement ?? window, { key: "ArrowRight" });
    expect(await screen.findByRole("heading", { name: "You finished this work" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Read again" }));
    expect(await screen.findByText("1 / 5")).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "You finished this work" })).toBeNull();
  });

  it("closes the settings with Escape before it closes the reader", async () => {
    const { gateway } = renderReader(documentSession());
    fireEvent.click(screen.getByRole("button", { name: "Open test reader" }));
    await screen.findByText("1 / 5");

    fireEvent.click(screen.getByRole("button", { name: "Reading" }));
    expect(screen.getByRole("region", { name: "Reading settings" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Two pages" }));
    expect(screen.getByRole("button", { name: "Two pages" }).getAttribute("aria-pressed")).toBe("true");

    fireEvent.keyDown(document.activeElement ?? window, { key: "Escape" });
    expect(screen.queryByRole("region", { name: "Reading settings" })).toBeNull();
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(gateway.closeMediaSession).not.toHaveBeenCalled();
  });

  it("says when a work cannot be opened instead of doing nothing", async () => {
    renderReader(documentSession(), new Error("no media found"));
    fireEvent.click(screen.getByRole("button", { name: "Open test reader" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("This work could not be opened.");
    expect(alert.textContent).toContain("no media found");
  });
});

function renderReader(session: MediaSession, openFailure?: Error) {
  const gateway = {
    openMediaSession: openFailure ? vi.fn().mockRejectedValue(openFailure) : vi.fn().mockResolvedValue(session),
    closeMediaSession: vi.fn().mockResolvedValue(session),
    updateMediaProgress: vi.fn().mockResolvedValue(session),
    readInstallation: vi.fn().mockResolvedValue(readerInstallation()),
    readOwnWorkDetails: vi.fn().mockResolvedValue(null),
    mediaAssetUrl: vi.fn().mockReturnValue("dla-media://localhost/session-1%2F0"),
    mediaThumbnailUrl: vi.fn().mockReturnValue("dla-media://localhost/session-1%2F0?thumbnail"),
  } satisfies ReaderGateway;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <KeyBindingsProvider>
          <ReaderProvider gateway={gateway}>
            <OpenReader />
            <ReaderOverlay gateway={gateway as unknown as LibraryGateway} />
          </ReaderProvider>
        </KeyBindingsProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
  return { gateway };
}

function OpenReader() {
  const reader = useReader();
  return <button type="button" onClick={() => void reader.open("installation-1")}>Open test reader</button>;
}

function fakePdf(pageCount: number): PdfDocument {
  return {
    pageCount,
    pageSize: () => Promise.resolve({ width: 595, height: 842 }),
    render: () => Promise.resolve(document.createElement("canvas")),
    destroy: () => undefined,
  };
}

function documentSession(progress: { positionMs?: number; durationMs?: number | null } = {}): MediaSession {
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
      positionMs: progress.positionMs ?? 0,
      durationMs: progress.durationMs ?? null,
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
