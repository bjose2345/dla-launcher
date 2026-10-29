// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import { PresentationProvider } from "../../preferences/PresentationProvider";
import { ReaderOverlay } from "./ReaderOverlay";
import { ReaderProvider, useReader, type ReaderGateway } from "./ReaderProvider";
import type { Installation, LibraryGateway, MediaSession, MediaSessionItem } from "./types";

vi.mock("./pdfDocument", () => ({ openPdfDocument: vi.fn() }));

const FILES = ["Set/main/01.png", "Set/main/02.png", "Set/main/03.png", "Set/notext/01.png", "Set/notext/02.png", "Set/notext/03.png"];

afterEach(() => {
  cleanup();
  localStorage.clear();
  document.body.style.overflow = "";
  document.documentElement.style.overflow = "";
});

describe("ImageViewer", () => {
  it("opens where the viewing stopped, steps with the keys, and saves the place", async () => {
    const { gateway } = renderViewer(imageSession(1));
    await openViewer();

    expect(await screen.findByText("2 / 6")).toBeTruthy();
    expect(screen.getByText("Continuing from image 2")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Start over?" })).toBeTruthy();

    fireEvent.keyDown(document.activeElement ?? window, { key: "ArrowRight" });
    await waitFor(() => expect(gateway.updateMediaProgress).toHaveBeenLastCalledWith({
      sessionId: "session-1",
      itemOrdinal: 2,
      positionMs: 3,
      durationMs: 6,
      completed: false,
      status: "active",
    }), { timeout: 2_000 });

    // Stepping hid the bars; a click in the middle brings them back.
    expect(screen.queryByText("3 / 6")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show or hide the controls; double-click to zoom" }));
    expect(screen.getByText("3 / 6")).toBeTruthy();

    const strip = screen.getByRole("button", { name: "Image 1, 01.png" });
    expect(strip.querySelector("img")?.getAttribute("src")).toContain("?thumbnail");
  });

  it("narrows the set to one folder and shows every folder in the gallery", async () => {
    renderViewer(imageSession(1));
    await openViewer();
    await screen.findByText("2 / 6");

    fireEvent.click(screen.getByRole("button", { name: "Choose which folder to view" }));
    const folders = screen.getByRole("region", { name: "Folders" });
    fireEvent.click(within(folders).getByRole("button", { name: /^notext/ }));
    expect(screen.getByText("1 / 3")).toBeTruthy();

    fireEvent.keyDown(document.activeElement ?? window, { key: "g" });
    const gallery = await screen.findByRole("navigation", { name: "Folders" });
    expect(screen.getAllByRole("button", { name: /^Image \d, / })).toHaveLength(3);

    fireEvent.click(within(gallery).getByRole("button", { name: /^All images/ }));
    expect(screen.getAllByRole("button", { name: /^Image \d, / })).toHaveLength(6);

    fireEvent.click(screen.getByRole("button", { name: "Image 5, 02.png" }));
    expect(await screen.findByText("5 / 6")).toBeTruthy();
    expect(screen.queryByRole("navigation", { name: "Folders" })).toBeNull();
  });

  it("marks the set viewed at its last image and offers the gallery at the end", async () => {
    const { gateway } = renderViewer(imageSession(0));
    await openViewer();
    await screen.findByText("1 / 6");

    fireEvent.keyDown(document.activeElement ?? window, { key: "End" });
    await waitFor(() => expect(gateway.updateMediaProgress).toHaveBeenCalledWith(expect.objectContaining({
      itemOrdinal: 5,
      positionMs: 6,
      completed: true,
      status: "completed",
    })));

    fireEvent.keyDown(document.activeElement ?? window, { key: "ArrowRight" });
    expect(await screen.findByRole("heading", { name: "You have seen every image" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Open the gallery" }));
    expect(await screen.findByRole("button", { name: "Viewer" })).toBeTruthy();
  });

  it("moves an image set to the comic reader and back at the same image", async () => {
    renderViewer(imageSession(2));
    await openViewer();
    await screen.findByText("3 / 6");

    fireEvent.click(screen.getByRole("button", { name: "View" }));
    fireEvent.click(within(screen.getByRole("region", { name: "View settings" })).getByRole("button", { name: "Comic reader" }));

    expect(await screen.findByRole("button", { name: "Reading" })).toBeTruthy();
    expect(screen.getByText("3 / 6")).toBeTruthy();
    expect(screen.queryByText("Continuing from page 3")).toBeNull();
    expect(JSON.parse(localStorage.getItem("dla-launcher:images:v1:installation-1") ?? "{}").openIn).toBe("reader");

    fireEvent.click(screen.getByRole("button", { name: "Reading" }));
    fireEvent.click(screen.getByRole("button", { name: "CG viewer" }));
    expect(await screen.findByRole("button", { name: "View" })).toBeTruthy();
    expect(screen.getByText("3 / 6")).toBeTruthy();
  });
});

function renderViewer(session: MediaSession) {
  const gateway = {
    openMediaSession: vi.fn().mockResolvedValue(session),
    closeMediaSession: vi.fn().mockResolvedValue(session),
    updateMediaProgress: vi.fn().mockResolvedValue(session),
    readInstallation: vi.fn().mockResolvedValue(installation()),
    readOwnWorkDetails: vi.fn().mockResolvedValue(null),
    mediaAssetUrl: vi.fn((sessionId: string, ordinal: number) => `dla-media://localhost/${sessionId}%2F${ordinal}`),
    mediaThumbnailUrl: vi.fn((sessionId: string, ordinal: number) => `dla-media://localhost/${sessionId}%2F${ordinal}?thumbnail`),
  } satisfies ReaderGateway;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <KeyBindingsProvider>
          <ReaderProvider gateway={gateway}>
            <OpenViewer />
            <ReaderOverlay gateway={gateway as unknown as LibraryGateway} />
          </ReaderProvider>
        </KeyBindingsProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
  return { gateway };
}

/** Opens the viewer and waits until it holds focus, as it does before a person can press a key. */
async function openViewer() {
  fireEvent.click(screen.getByRole("button", { name: "Open test viewer" }));
  const dialog = await screen.findByRole("dialog");
  await waitFor(() => expect(dialog.querySelector(".reader")).toBe(document.activeElement));
}

function OpenViewer() {
  const reader = useReader();
  return <button type="button" onClick={() => void reader.open("installation-1")}>Open test viewer</button>;
}

function imageSession(at: number): MediaSession {
  const items: MediaSessionItem[] = FILES.map((relativePath, ordinal) => ({
    ordinal,
    installationId: "installation-1",
    workCode: "RJ00000002",
    relativePath,
    mediaType: "image",
    sizeBytes: 2_000_000,
    discNumber: null,
    trackNumber: null,
    bonus: false,
  }));
  return {
    id: "session-1",
    kind: "work",
    installationId: "installation-1",
    action: "read_images",
    status: "active",
    repeatMode: "off",
    shuffle: false,
    items,
    progress: {
      itemOrdinal: at,
      positionMs: at + 1,
      durationMs: FILES.length,
      completed: false,
      updatedAt: "2026-08-15T00:00:00Z",
    },
    openedAt: "2026-08-15T00:00:00Z",
    updatedAt: "2026-08-15T00:00:00Z",
    endedAt: null,
    error: null,
  };
}

function installation(): Installation {
  return {
    id: "installation-1",
    scanRootId: null,
    rootPath: "/library/RJ00000002",
    platform: "linux",
    status: "ready",
    detection: {
      sourceScanSessionId: null,
      catalogIdentity: { workCode: "RJ00000002", confidence: "exact", reasonCodes: [] },
      suggestedStatus: "ready",
      contentItems: [],
      launchCandidates: [],
      packageInspection: null,
    },
    overrides: {
      catalogIdentity: null,
      customTitle: "Test set",
      preferredAction: null,
      contentItems: [],
      reviewedAt: "2026-08-15T00:00:00Z",
    },
    discoveredAt: "2026-08-15T00:00:00Z",
    updatedAt: "2026-08-15T00:00:00Z",
  };
}
