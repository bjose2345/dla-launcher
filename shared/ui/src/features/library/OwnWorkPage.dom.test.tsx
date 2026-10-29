// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import { PresentationProvider } from "../../preferences/PresentationProvider";
import { MediaPlaybackProvider } from "./MediaPlaybackProvider";
import { OwnWorkPage } from "./OwnWorkPage";
import type { Installation, LibraryGateway, OwnWorkDetails } from "./types";

afterEach(() => {
  cleanup();
});

describe("OwnWorkPage", () => {
  it("lays out a work outside the catalog like a catalog work, from the metadata the user gave it", async () => {
    const { onEditMetadata, onOpenMedia } = renderPage(details());

    expect(await screen.findByRole("heading", { level: 1, name: "My CG set" })).toBeTruthy();
    expect(screen.getByText("My circle")).toBeTruthy();
    expect(screen.getByRole("button", { name: /D_123456/ })).toBeTruthy();
    expect(screen.getByText("2023")).toBeTruthy();
    expect(screen.getByText("Images")).toBeTruthy();
    expect(screen.getByText("Bonus")).toBeTruthy();
    expect(screen.getByText("2023-03-12")).toBeTruthy();
    expect([...document.querySelectorAll(".own-work-description p")].map((p) => p.textContent))
      .toEqual(["First line\nsecond line", "Last part"]);

    const gallery = screen.getByRole("heading", { level: 2, name: /Gallery/ }).closest("section")!;
    expect([...gallery.querySelectorAll("img")].map((image) => image.getAttribute("src")))
      .toEqual(["art:installation-1:sample-1", "art:installation-1:sample-2"]);

    fireEvent.click(screen.getByRole("button", { name: /Edit metadata/ }));
    expect(onEditMetadata).toHaveBeenCalledOnce();

    fireEvent.click(await screen.findByRole("button", { name: /^Read/ }));
    expect(onOpenMedia).toHaveBeenCalledWith("installation-1");
  });

  it("says what is missing instead of inventing it", async () => {
    renderPage(null);

    expect(await screen.findByRole("heading", { level: 1, name: "My CG set" })).toBeTruthy();
    expect(screen.getByText("Unknown circle")).toBeTruthy();
    expect(screen.getAllByText("Not set").length).toBeGreaterThanOrEqual(3);
    expect(screen.getByText("No sample images yet. Pick some in Edit metadata.")).toBeTruthy();
    expect(screen.getByText("No description yet.")).toBeTruthy();
  });
});

function renderPage(own: OwnWorkDetails | null) {
  const gateway = {
    readInstallation: vi.fn().mockResolvedValue(installation()),
    readOwnWorkDetails: vi.fn().mockResolvedValue(own),
    readPreparedPackage: vi.fn().mockResolvedValue(null),
    ownWorkArtworkUrl: vi.fn((installationId: string, artworkId: string) => `art:${installationId}:${artworkId}`),
    listInstallationLaunchHistory: vi.fn().mockResolvedValue([]),
    readMediaSession: vi.fn(),
    readOpenPersonalizedMediaSession: vi.fn().mockResolvedValue(null),
  } as unknown as LibraryGateway;
  const onEditMetadata = vi.fn();
  const onOpenMedia = vi.fn();
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <KeyBindingsProvider>
        <MediaPlaybackProvider gateway={gateway}>
          <OwnWorkPage
            installationId="installation-1"
            gateway={gateway}
            onBack={vi.fn()}
            onEditMetadata={onEditMetadata}
            onOpenScanner={vi.fn()}
            onOpenInstallation={vi.fn()}
            onOpenMedia={onOpenMedia}
            onOpenExternal={vi.fn()}
          />
        </MediaPlaybackProvider>
        </KeyBindingsProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
  return { onEditMetadata, onOpenMedia };
}

function details(): OwnWorkDetails {
  return {
    installationId: "installation-1",
    circle: "My circle",
    code: "D_123456",
    releaseDate: "2023-03-12",
    kind: "images",
    tags: ["Bonus"],
    description: "First line\nsecond line\n\nLast part",
    cover: "cover-1",
    samples: ["sample-1", "sample-2"],
    updatedAt: "2026-10-29T00:00:00Z",
  };
}

function installation(): Installation {
  return {
    id: "installation-1",
    scanRootId: null,
    rootPath: "/library/My CG set",
    platform: "linux",
    status: "ready",
    detection: {
      sourceScanSessionId: null,
      catalogIdentity: null,
      suggestedStatus: "ready",
      contentItems: [
        { relativePath: "01.png", pathKey: "01.png", mediaType: "image", sizeBytes: 1024, modifiedAt: null, confidence: "high", reasonCodes: [] },
        { relativePath: "02.png", pathKey: "02.png", mediaType: "image", sizeBytes: 1024, modifiedAt: null, confidence: "high", reasonCodes: [] },
      ],
      launchCandidates: [],
      packageInspection: null,
    },
    overrides: {
      catalogIdentity: { kind: "unidentified" },
      customTitle: "My CG set",
      preferredAction: { action: "read_images", target: { kind: "installation_root" } },
      contentItems: [],
      reviewedAt: "2026-10-29T00:00:00Z",
    },
    discoveredAt: "2026-10-29T00:00:00Z",
    updatedAt: "2026-10-29T00:00:00Z",
  };
}
