// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { CatalogDetailGateway } from "../catalog";
import type { AndroidAppGateway, AndroidAppView } from "../../android-package";
import { PresentationProvider } from "../../preferences/PresentationProvider";
import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import { ImageReaderProvider } from "./ImageReaderProvider";
import { LibraryPage } from "./LibraryPage";
import { MediaPlaybackProvider } from "./MediaPlaybackProvider";
import type {
  Installation,
  LibraryGateway,
  LibraryShelves,
  PersonalizedRecommendationItem,
} from "./types";

afterEach(() => {
  cleanup();
});

describe("LibraryPage data loading", () => {
  it("loads preparation and catalog metadata through one batch call each", async () => {
    const installations = [
      installation("installation-1", "RJ00000001"),
      installation("installation-2", "RJ00000002"),
    ];
    const readPreparedPackage = vi.fn();
    const readPreparedPackages = vi.fn().mockResolvedValue([]);
    const readCatalogWork = vi.fn();
    const readCatalogWorks = vi.fn().mockResolvedValue([]);
    const shelves: LibraryShelves = {
      installations,
      recent: [],
      continueItems: [],
      neverLaunched: [],
      unfinished: [],
      launchTotals: [],
    };
    const gateway = {
      readShelves: vi.fn().mockResolvedValue(shelves),
      listRecentLaunches: vi.fn().mockResolvedValue([]),
      readLocalPersonalization: vi.fn().mockResolvedValue({
        favorites: [],
        becauseYou: [],
        voiceMix: [],
        activityWorkCount: 0,
        voiceActivityWorkCount: 0,
        becauseYouMinimum: 2,
        voiceMixMinimum: 2,
      }),
      readPreparedPackage,
      readPreparedPackages,
      mediaAssetUrl: vi.fn().mockReturnValue(""),
    } as unknown as LibraryGateway;
    const catalogGateway = {
      read: readCatalogWork,
      readWorks: readCatalogWorks,
    } as unknown as CatalogDetailGateway;
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={client}>
        <PresentationProvider>
          <KeyBindingsProvider>
            <MediaPlaybackProvider gateway={gateway}>
              <ImageReaderProvider gateway={gateway}>
                <LibraryPage
                  gateway={gateway}
                  catalogGateway={catalogGateway}
                  onOpenReview={vi.fn()}
                  onOpenMedia={vi.fn()}
                  onOpenWork={vi.fn()}
                />
              </ImageReaderProvider>
            </MediaPlaybackProvider>
          </KeyBindingsProvider>
        </PresentationProvider>
      </QueryClientProvider>,
    );

    await waitFor(() => {
      expect(readPreparedPackages).toHaveBeenCalledWith(["installation-1", "installation-2"]);
      expect(readCatalogWorks).toHaveBeenCalledWith(["RJ00000001", "RJ00000002"]);
    });
    expect(readPreparedPackage).not.toHaveBeenCalled();
    expect(readCatalogWork).not.toHaveBeenCalled();
  });

  it("shows and explicitly launches a certificate-bound Android Library entry", async () => {
    const shelves: LibraryShelves = {
      installations: [],
      recent: [],
      continueItems: [],
      neverLaunched: [],
      unfinished: [],
      launchTotals: [],
    };
    const gateway = {
      readShelves: vi.fn().mockResolvedValue(shelves),
      listRecentLaunches: vi.fn().mockResolvedValue([]),
      readLocalPersonalization: vi.fn().mockResolvedValue({
        favorites: [], becauseYou: [], voiceMix: [], activityWorkCount: 0,
        voiceActivityWorkCount: 0, becauseYouMinimum: 2, voiceMixMinimum: 2,
      }),
      readPreparedPackages: vi.fn().mockResolvedValue([]),
      mediaAssetUrl: vi.fn().mockReturnValue(""),
    } as unknown as LibraryGateway;
    const item = androidAppView();
    const androidGateway: AndroidAppGateway = {
      list: vi.fn().mockResolvedValue([item]),
      associateInstalled: vi.fn().mockResolvedValue(item),
      launch: vi.fn().mockResolvedValue({
        ...item,
        association: { ...item.association, launchCount: 1 },
      }),
      remove: vi.fn().mockResolvedValue(undefined),
    };
    const catalogGateway = {
      readWorks: vi.fn().mockResolvedValue([]),
    } as unknown as CatalogDetailGateway;
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

    render(
      <QueryClientProvider client={client}>
        <PresentationProvider>
          <KeyBindingsProvider>
            <MediaPlaybackProvider gateway={gateway}>
              <ImageReaderProvider gateway={gateway}>
                <LibraryPage
                  gateway={gateway}
                  androidAppGateway={androidGateway}
                  catalogGateway={catalogGateway}
                  onOpenReview={vi.fn()}
                  onOpenMedia={vi.fn()}
                  onOpenWork={vi.fn()}
                  onReinstallAndroidApp={vi.fn()}
                />
              </ImageReaderProvider>
            </MediaPlaybackProvider>
          </KeyBindingsProvider>
        </PresentationProvider>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "Android apps" })).toBeTruthy();
    expect(screen.getByText("Installed and ready")).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Your library is empty" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Open app" }));
    await waitFor(() => expect(androidGateway.launch).toHaveBeenCalledWith(item.association.id));
  });
});

describe("Library discovery", () => {
  it("opens the first lane that has content and never the Activity block", async () => {
    renderLibrary({
      favorites: [personalWork("RJ00000009")],
    });

    const favorites = await screen.findByRole("tab", { name: /Favorites/ });
    expect(favorites.getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("tab", { name: /Because you played/ }).getAttribute("aria-selected"))
      .toBe("false");
    expect(screen.queryByText(/Activity & management/i)).toBeNull();
    expect(screen.queryByText(/Launch history/i)).toBeNull();
  });

  it("switches lanes on click and shows one panel at a time", async () => {
    renderLibrary({ favorites: [personalWork("RJ00000009")] });

    const voice = await screen.findByRole("tab", { name: /Your voice mix/ });
    fireEvent.click(voice);

    await waitFor(() => {
      expect(voice.getAttribute("aria-selected")).toBe("true");
    });
    expect(screen.getAllByRole("tabpanel")).toHaveLength(1);
    expect(screen.getByRole("tab", { name: /Favorites/ }).getAttribute("aria-selected"))
      .toBe("false");
  });

  it("moves and activates discovery tabs from the keyboard", async () => {
    renderLibrary({ favorites: [personalWork("RJ00000009")] });

    const favorites = await screen.findByRole("tab", { name: /Favorites/ });
    favorites.focus();
    fireEvent.keyDown(favorites, { key: "ArrowRight" });

    const voice = screen.getByRole("tab", { name: /Your voice mix/ });
    expect(document.activeElement).toBe(voice);
    expect(voice.getAttribute("aria-selected")).toBe("true");

    fireEvent.keyDown(voice, { key: "Home" });
    const suggested = screen.getByRole("tab", { name: /Because you played/ });
    expect(document.activeElement).toBe(suggested);
    expect(suggested.getAttribute("aria-selected")).toBe("true");
  });

  it("exposes every recommendation anchor in an accessible tooltip", async () => {
    renderLibrary({ becauseYou: [personalRecommendation("RJ00000009")] });

    const reason = await screen.findByRole("button", { name: "Similar to what you played" });
    reason.focus();

    const tooltip = await screen.findByRole("tooltip");
    expect(tooltip.textContent).toContain("Similar to what you played: First source");
    expect(tooltip.textContent).toContain("Similar to what you listened to: Second source");
    expect(reason.getAttribute("aria-describedby")).toBe(tooltip.id);
  });

  it("moves preference management out of the library", async () => {
    renderLibrary({ favorites: [personalWork("RJ00000009")] });

    await screen.findByRole("tab", { name: /Because you played/ });
    expect(screen.queryByText("Manage the explicit signals that shape recommendations on this device.")).toBeNull();
  });
});

function renderLibrary(personalization: Record<string, unknown>) {
  const shelves: LibraryShelves = {
    installations: [],
    recent: [],
    continueItems: [],
    neverLaunched: [],
    unfinished: [],
    launchTotals: [],
  };
  const gateway = {
    readShelves: vi.fn().mockResolvedValue(shelves),
    listRecentLaunches: vi.fn().mockResolvedValue([]),
    readLocalPersonalization: vi.fn().mockResolvedValue({
      favorites: [],
      becauseYou: [],
      voiceMix: [],
      activityWorkCount: 0,
      voiceActivityWorkCount: 0,
      becauseYouMinimum: 2,
      voiceMixMinimum: 2,
      ...personalization,
    }),
    readPreparedPackages: vi.fn().mockResolvedValue([]),
    listWorkPreferences: vi.fn().mockResolvedValue([]),
    mediaAssetUrl: vi.fn().mockReturnValue(""),
  } as unknown as LibraryGateway;
  const catalogGateway = {
    read: vi.fn(),
    readWorks: vi.fn().mockResolvedValue([]),
  } as unknown as CatalogDetailGateway;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <KeyBindingsProvider>
          <MediaPlaybackProvider gateway={gateway}>
            <ImageReaderProvider gateway={gateway}>
              <LibraryPage
                gateway={gateway}
                catalogGateway={catalogGateway}
                onOpenReview={vi.fn()}
                onOpenMedia={vi.fn()}
                onOpenWork={vi.fn()}
              />
            </ImageReaderProvider>
          </MediaPlaybackProvider>
        </KeyBindingsProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function personalWork(code: string) {
  return {
    code,
    sourceCode: code,
    title: code,
    titleEnglish: "",
    addedDate: "2026-01-03",
    releaseDate: "2026-01-02",
    updatedDate: "2026-01-04",
    ageRating: "r18",
    releaseType: "digital",
    mainImageUrls: ["main.webp"],
    thumbnailUrls: ["small.webp", "large.webp"],
    circles: [],
    categories: [],
    tags: [],
    synthetic: false,
  };
}

function personalRecommendation(code: string): PersonalizedRecommendationItem {
  return {
    work: personalWork(code),
    score: 10,
    anchors: [
      {
        workCode: "RJ00000001",
        title: "First source",
        action: "launch_executable",
      },
      {
        workCode: "RJ00000002",
        title: "Second source",
        action: "play_audio",
      },
    ],
  };
}

function installation(id: string, workCode: string): Installation {
  return {
    id,
    scanRootId: null,
    rootPath: `/library/${workCode}`,
    platform: "linux",
    status: "ready",
    detection: {
      sourceScanSessionId: null,
      catalogIdentity: { workCode, confidence: "exact", reasonCodes: [] },
      suggestedStatus: "ready",
      contentItems: [],
      launchCandidates: [{
        id: `${id}-launch`,
        action: "launch_executable",
        target: { kind: "relative_path", path: "game" },
        supportedPlatforms: ["linux"],
        confidence: "high",
        reasonCodes: [],
      }],
      packageInspection: {
        classification: { contentKind: "unknown" },
      } as Installation["detection"]["packageInspection"],
    },
    overrides: {
      catalogIdentity: null,
      customTitle: workCode,
      preferredAction: null,
      contentItems: [],
      reviewedAt: "2026-08-15T00:00:00Z",
    },
    discoveredAt: "2026-08-15T00:00:00Z",
    updatedAt: "2026-08-15T00:00:00Z",
  };
}

function androidAppView(): AndroidAppView {
  return {
    association: {
      id: "android-app-1234567890",
      workCode: "RJ01326398",
      packageName: "org.dlaproject.fixture.launch",
      applicationLabel: "DLA Launch Fixture",
      expectedSigningCertificateSha256: ["a".repeat(64)],
      associatedVersionName: "1.0",
      associatedVersionCode: "1",
      associatedAt: "2026-08-22T12:00:00Z",
      updatedAt: "2026-08-22T12:00:00Z",
      lastLaunchedAt: null,
      launchCount: 0,
    },
    runtime: {
      state: "ready",
      applicationLabel: "DLA Launch Fixture",
      versionName: "1.0",
      versionCode: "1",
      technicalDetail: null,
    },
  };
}
