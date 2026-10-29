// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import type { CatalogWork } from "../catalog/types";
import type { ScanInstallGateway } from "../scanner/types";
import { LibraryManageDrawer, recommendedKey, type ManageTab } from "./LibraryManageDrawer";
import type {
  Installation,
  InstallationHealthReport,
  InstallationReviewRequest,
  LibraryGateway,
  OwnWorkDetails,
} from "./types";

afterEach(() => {
  cleanup();
});

describe("LibraryManageDrawer", () => {
  it("shows General and Installed files and saves a new name as soon as it is typed", async () => {
    const gateway = libraryGateway();
    renderDrawer(gateway);

    expect(await screen.findByRole("dialog", { name: "Kunoichi Red" })).toBeTruthy();
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["General", "Installed files"]);
    expect(screen.getByText("Changes save as you make them")).toBeTruthy();

    const name = screen.getByLabelText("Name");
    fireEvent.change(name, { target: { value: "My ninja game" } });
    fireEvent.blur(name);

    await waitFor(() => expect(gateway.saveReview).toHaveBeenCalledOnce());
    expect(savedRequest(gateway).customTitle).toBe("My ninja game");
    expect(await screen.findByText("Saved")).toBeTruthy();
  });

  it("fixes the match by picking a catalog work", async () => {
    const gateway = libraryGateway();
    renderDrawer(gateway);

    fireEvent.click(await screen.findByRole("button", { name: "Fix match…" }));
    fireEvent.change(screen.getByLabelText("Find a work in the catalog"), { target: { value: "RJ01000004" } });
    fireEvent.click(await screen.findByRole("button", { name: /Her Secret/ }, { timeout: 2_000 }));

    await waitFor(() => expect(gateway.saveReview).toHaveBeenCalledOnce());
    expect(savedRequest(gateway).catalogIdentity).toEqual({ kind: "catalog_work", workCode: "RJ01000004" });
  });

  it("shows a work outside the catalog with the user's own details", async () => {
    const installation = gameInstallation();
    installation.detection.catalogIdentity = null;
    installation.overrides.catalogIdentity = { kind: "unidentified" };
    const own: OwnWorkDetails = {
      installationId: installation.id,
      circle: "My circle",
      code: "D_123456",
      releaseDate: null,
      kind: "game",
      tags: [],
      description: null,
      cover: "cover-1",
      samples: [],
      updatedAt: "2026-10-28T00:00:00Z",
    };
    renderDrawer(libraryGateway({ installation, own }));

    const dialog = await screen.findByRole("dialog", { name: "KUNOICHI RED" });
    expect(await within(dialog).findByText("My circle · D_123456")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Metadata" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Game" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("img", { name: "Cover" }).getAttribute("src")).toBe("art:installation-1:cover-1");
  });

  it("shows an unknown work with question marks and a way to find it", async () => {
    const installation = gameInstallation();
    installation.detection.catalogIdentity = null;
    installation.overrides.catalogIdentity = { kind: "unidentified" };
    renderDrawer(libraryGateway({ installation }));

    expect(await screen.findByRole("dialog", { name: "KUNOICHI RED" })).toBeTruthy();
    expect(screen.getAllByText("Unknown work").length).toBeGreaterThan(0);
    expect(screen.getAllByText("?").length).toBe(2);
    expect(screen.getByRole("button", { name: "Find in catalog…" })).toBeTruthy();
  });

  it("chooses what the work starts with, marking the recommended file", async () => {
    const gateway = libraryGateway();
    renderDrawer(gateway);

    const startsWith = await screen.findByLabelText("Starts with") as HTMLSelectElement;
    expect([...startsWith.options].map((option) => option.textContent)).toEqual([
      "Game.exe · Launch executable (recommended)",
      "Config.exe · Launch executable",
    ]);
    fireEvent.change(startsWith, { target: { value: "launch_executable|path:KUNOICHI RED/Config.exe" } });

    await waitFor(() => expect(gateway.saveReview).toHaveBeenCalledOnce());
    expect(savedRequest(gateway).preferredAction).toEqual({
      action: "launch_executable",
      target: { kind: "relative_path", path: "KUNOICHI RED/Config.exe" },
    });
  });

  it("shows the work's own folder without the Windows path marker and opens it", async () => {
    const installation = gameInstallation();
    installation.rootPath = "\\\\?\\D:\\Games";
    const gateway = libraryGateway({ installation });
    renderDrawer(gateway, { tab: "files" });

    expect(await screen.findByText("D:\\Games\\KUNOICHI RED")).toBeTruthy();
    expect(screen.getByText("2 files · 200 B")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Browse…" }));
    await waitFor(() => expect(gateway.openInstallationFolder).toHaveBeenCalledWith("installation-1"));
  });

  it("counts a single file in the singular", async () => {
    const installation = gameInstallation();
    installation.detection.contentItems = [contentItem("KUNOICHI RED/Game.exe")];
    renderDrawer(libraryGateway({
      installation,
      health: healthReport({ expectedFiles: 1, presentFiles: 1 }),
    }), { tab: "files" });

    expect(await screen.findByText("1 file · 100 B")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Verify integrity of files" }));
    expect(await screen.findByText("The file is present.")).toBeTruthy();
  });

  it("verifies the files and reports the result in plain words", async () => {
    const gateway = libraryGateway();
    renderDrawer(gateway, { tab: "files" });

    expect(screen.queryByText("Healthy")).toBeNull();
    fireEvent.click(await screen.findByRole("button", { name: "Verify integrity of files" }));

    await waitFor(() => expect(gateway.verifyInstallation).toHaveBeenCalledWith("installation-1"));
    expect(await screen.findByText("All 14 files are present.")).toBeTruthy();
  });

  it("offers Repair only when files are missing and can be repaired", async () => {
    renderDrawer(libraryGateway({
      health: healthReport({ state: "repairable", repairable: true, missingFiles: 3 }),
    }), { tab: "files" });

    expect(await screen.findByText("3 files are missing or changed.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Repair" })).toBeTruthy();
  });

  it("installs an archive that was added but never unpacked", async () => {
    const installation = gameInstallation();
    installation.detection.packageInspection = packageInspection();
    const installs = scanInstalls();
    renderDrawer(libraryGateway({ installation }), { tab: "files", installs });

    expect(await screen.findByText("This archive isn't installed yet")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Install" }));

    await waitFor(() => expect(installs.installArchive).toHaveBeenCalledWith("installation-1"));
  });

  it("confirms before uninstalling and closes once the work is gone", async () => {
    const gateway = libraryGateway();
    const onClose = vi.fn();
    renderDrawer(gateway, { tab: "files", onClose });

    fireEvent.click(await screen.findByRole("button", { name: "Uninstall…" }));
    const confirmation = screen.getByRole("alertdialog");
    expect(gateway.uninstallInstallation).not.toHaveBeenCalled();

    fireEvent.click(within(confirmation).getByRole("button", { name: "Uninstall" }));
    await waitFor(() => expect(gateway.uninstallInstallation).toHaveBeenCalledWith("installation-1"));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("closes with Escape", async () => {
    const onClose = vi.fn();
    renderDrawer(libraryGateway(), { onClose });

    await screen.findByRole("dialog", { name: "Kunoichi Red" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("recommends the most confident start action, the first among equals", () => {
    const candidates = gameInstallation().detection.launchCandidates;
    expect(recommendedKey(candidates)).toBe("launch_executable|path:KUNOICHI RED/Game.exe");
    expect(recommendedKey([])).toBeNull();
  });
});

const works: Record<string, CatalogWork> = {
  RJ01000002: work("RJ01000002", "Kunoichi Red"),
  RJ01000004: work("RJ01000004", "Her Secret"),
};

function renderDrawer(
  gateway: LibraryGateway,
  options: { tab?: ManageTab; onClose?: () => void; installs?: ScanInstallGateway } = {},
) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <LibraryManageDrawer
          installationId="installation-1"
          initialTab={options.tab}
          gateway={gateway}
          catalogGateway={{
            readWorks: vi.fn(async (codes: string[]) => codes.flatMap((code) => works[code] ? [works[code]] : [])),
          }}
          installs={options.installs}
          onClose={options.onClose ?? (() => undefined)}
        />
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function savedRequest(gateway: LibraryGateway): InstallationReviewRequest {
  return vi.mocked(gateway.saveReview).mock.calls[0]![0];
}

function scanInstalls(): ScanInstallGateway {
  const view = { revision: 1, installFolder: "/home/developer/DLA Library", installs: [] };
  return {
    readInstalls: vi.fn().mockResolvedValue(view),
    subscribeInstalls: vi.fn().mockResolvedValue(() => undefined),
    selectInstallFolder: vi.fn().mockResolvedValue(view),
    installArchive: vi.fn().mockResolvedValue({ ...view, revision: 2 }),
    cancelInstall: vi.fn().mockResolvedValue(view),
  };
}

function libraryGateway(overrides: {
  installation?: Installation;
  health?: InstallationHealthReport;
  own?: OwnWorkDetails;
} = {}): LibraryGateway {
  const installation = overrides.installation ?? gameInstallation();
  return {
    readInstallation: vi.fn().mockResolvedValue(installation),
    readOwnWorkDetails: vi.fn().mockResolvedValue(overrides.own ?? null),
    ownWorkArtworkUrl: vi.fn((installationId: string, artworkId: string) => `art:${installationId}:${artworkId}`),
    readInstallationHealth: vi.fn().mockResolvedValue(overrides.health ?? healthReport()),
    readPreparedPackage: vi.fn().mockResolvedValue(null),
    readPackagePreparationProgress: vi.fn().mockResolvedValue(null),
    subscribePackagePreparationProgress: vi.fn().mockResolvedValue(() => undefined),
    saveReview: vi.fn(async (request: InstallationReviewRequest) => ({
      ...installation,
      overrides: { ...installation.overrides, ...request, reviewedAt: "2026-10-08T10:00:00Z" },
    })),
    verifyInstallation: vi.fn().mockResolvedValue(overrides.health ?? healthReport()),
    rescanInstallation: vi.fn().mockResolvedValue(healthReport()),
    repairInstallation: vi.fn().mockResolvedValue(healthReport()),
    locateInstallation: vi.fn().mockResolvedValue(healthReport()),
    selectInstallationLocation: vi.fn().mockResolvedValue(null),
    cleanupMaintenance: vi.fn(),
    removeInstallation: vi.fn().mockResolvedValue(undefined),
    uninstallInstallation: vi.fn().mockResolvedValue(undefined),
    openInstallationFolder: vi.fn().mockResolvedValue(undefined),
  } as unknown as LibraryGateway;
}

function gameInstallation(): Installation {
  return {
    id: "installation-1",
    scanRootId: "root-1",
    rootPath: "/home/developer/My Works",
    platform: "linux",
    status: "ready",
    detection: {
      sourceScanSessionId: "session-1",
      catalogIdentity: { workCode: "RJ01000002", confidence: "strong", reasonCodes: ["code_in_directory_name"] },
      suggestedStatus: "needs_review",
      contentItems: [
        contentItem("KUNOICHI RED/Config.exe"),
        contentItem("KUNOICHI RED/Game.exe"),
      ],
      launchCandidates: [
        {
          id: "launch-game",
          action: "launch_executable",
          target: { kind: "relative_path", path: "KUNOICHI RED/Game.exe" },
          supportedPlatforms: ["windows"],
          confidence: "high",
          reasonCodes: ["preferred_executable_name"],
        },
        {
          id: "launch-config",
          action: "launch_executable",
          target: { kind: "relative_path", path: "KUNOICHI RED/Config.exe" },
          supportedPlatforms: ["windows"],
          confidence: "medium",
          reasonCodes: ["executable_candidate"],
        },
      ],
      packageInspection: null,
    },
    overrides: {
      catalogIdentity: null,
      customTitle: null,
      preferredAction: {
        action: "launch_executable",
        target: { kind: "relative_path", path: "KUNOICHI RED/Game.exe" },
      },
      contentItems: [],
      reviewedAt: "2026-10-08T09:00:00Z",
    },
    discoveredAt: "2026-10-08T09:00:00Z",
    updatedAt: "2026-10-08T09:00:00Z",
  };
}

function contentItem(relativePath: string): Installation["detection"]["contentItems"][number] {
  return {
    relativePath,
    pathKey: relativePath,
    mediaType: "executable",
    sizeBytes: 100,
    modifiedAt: null,
    confidence: "high",
    reasonCodes: ["executable_extension"],
  };
}

function packageInspection(): NonNullable<Installation["detection"]["packageInspection"]> {
  return {
    source: {
      scanEntryId: "archive-1",
      kind: "archive",
      relativePath: "RJ01000002.zip",
      sizeBytes: 512,
      sha256: "a".repeat(64),
    },
    format: "zip",
    safety: "safe",
    entryCount: 2,
    fileCount: 2,
    directoryCount: 1,
    totalCompressedBytes: 512,
    totalUncompressedBytes: 1_024,
    commonRoot: null,
    issues: [],
    classification: {
      contentKind: "windows_game",
      engine: null,
      platform: "windows",
      confidence: "high",
      reasonCodes: ["executable_dominant"],
      contentRoot: null,
      launchCandidates: [],
    },
    installPlan: {
      requiresExtraction: true,
      contentRoot: null,
      preferredAction: null,
      archiveRetention: "keep",
    },
    inspectedAt: "2026-10-08T09:00:00Z",
  };
}

function healthReport(overrides: Partial<InstallationHealthReport> = {}): InstallationHealthReport {
  return {
    installationId: "installation-1",
    state: "healthy",
    managed: true,
    repairable: false,
    checkedRoot: "/home/developer/DLA Library/RJ01000002",
    checkedAt: "2026-10-08T09:30:00Z",
    expectedFiles: 14,
    presentFiles: 14,
    missingFiles: 0,
    modifiedFiles: 0,
    inaccessibleFiles: 0,
    unexpectedFiles: 0,
    issues: [],
    ...overrides,
  };
}

function work(code: string, title: string): CatalogWork {
  return {
    code,
    sourceCode: code,
    title,
    titleEnglish: "",
    addedDate: "2026-10-01",
    releaseDate: "2026-10-01",
    updatedDate: "2026-10-01",
    ageRating: "all_ages",
    releaseType: "game",
    mainImageUrls: [],
    thumbnailUrls: [],
    circles: [],
    categories: [],
    tags: [],
    synthetic: true,
  };
}
