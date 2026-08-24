// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KeyBindingsProvider } from "../../preferences/KeyBindingsProvider";
import { PresentationProvider } from "../../preferences/PresentationProvider";
import { LibraryReviewPage } from "./LibraryReviewPage";
import type {
  Installation,
  InstallationHealthReport,
  LibraryGateway,
} from "./types";

afterEach(() => {
  cleanup();
});

describe("LibraryReviewPage", () => {
  it("opens collapsed for a reviewed installation and shows a summary per decision", async () => {
    renderReview(libraryGateway());

    expect(await screen.findByRole("heading", { name: "RJ01678999" })).toBeTruthy();
    expect(screen.getAllByRole("button", { name: /^Change / })).toHaveLength(3);
    expect(screen.queryByRole("radio", { name: /Use detected/ })).toBeNull();
  });

  it("opens the identity editor when the installation still needs review", async () => {
    const installation = readyInstallation();
    installation.status = "needs_review";
    installation.detection.catalogIdentity = null;
    renderReview(libraryGateway({ installation }));

    await screen.findByRole("heading", { name: "My Works" });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Done with Catalog identity/i })).toBeTruthy();
    });
  });

  it("opens the action editor when an identified installation needs an action", async () => {
    const installation = readyInstallation();
    installation.status = "needs_review";
    installation.detection.suggestedStatus = "needs_review";
    renderReview(libraryGateway({ installation }));

    await screen.findByRole("heading", { name: "RJ01678999" });
    expect(await screen.findByRole("button", { name: /Done with Opens with/i })).toBeTruthy();
    expect(screen.getByRole("radio", { name: /No preferred action/i })).toBeTruthy();
  });

  it("opens the action editor when the saved launch target disappeared", async () => {
    const installation = readyInstallation();
    installation.status = "needs_review";
    installation.overrides.preferredAction = {
      action: "launch_executable",
      target: { kind: "relative_path", path: "missing.exe" },
    };
    renderReview(libraryGateway({ installation }));

    expect(await screen.findByRole("button", { name: /Done with Opens with/i })).toBeTruthy();
  });

  it("expands one decision at a time", async () => {
    renderReview(libraryGateway());

    const changes = await screen.findAllByRole("button", { name: /^Change / });
    expect(changes[0]?.getAttribute("aria-controls")).toBe("library-review-identity-editor");
    fireEvent.click(changes[0]!);
    expect(screen.getAllByRole("button", { name: /^Done with / })).toHaveLength(1);
    expect(document.getElementById("library-review-identity-editor")).toBeTruthy();

    fireEvent.click(screen.getAllByRole("button", { name: /^Change / })[0]!);
    expect(screen.getAllByRole("button", { name: /^Done with / })).toHaveLength(1);
  });

  it("labels every zone with a real heading", async () => {
    renderReview(libraryGateway());

    await screen.findByRole("heading", { name: "RJ01678999" });
    for (const zone of ["Decide", "Library maintenance", "Danger zone"]) {
      expect(screen.getByRole("heading", { name: zone })).toBeTruthy();
    }
  });

  it("drops the invented step numbers", async () => {
    renderReview(libraryGateway());

    await screen.findByRole("heading", { name: "RJ01678999" });
    for (const step of ["01", "02", "03", "04", "05", "06"]) {
      expect(screen.queryByText(step)).toBeNull();
    }
  });

  it("hides Order for a single row but always exposes Ignore", async () => {
    renderReview(libraryGateway());

    fireEvent.click(await screen.findByRole("button", { name: /Change .*content/i }));

    expect(screen.getByRole("columnheader", { name: "Path" })).toBeTruthy();
    expect(screen.queryByRole("columnheader", { name: "Order" })).toBeNull();
    expect(screen.getByRole("columnheader", { name: "Ignore" })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: /RJ01678999\.zip/ })).toBeTruthy();
  });

  it("lets a single ignored launch target be un-ignored", async () => {
    const installation = readyInstallation();
    installation.status = "needs_review";
    installation.overrides.preferredAction = {
      action: "play_audio",
      target: { kind: "relative_path", path: "RJ01678999.zip" },
    };
    installation.overrides.contentItems = [
      { relativePath: "RJ01678999.zip", mediaType: null, ignored: true, order: null },
    ];
    renderReview(libraryGateway({ installation }));

    const checkbox = await screen.findByRole("checkbox", { name: /RJ01678999\.zip/ }) as HTMLInputElement;
    expect(checkbox.checked).toBe(true);
    fireEvent.click(checkbox);
    expect((screen.getByRole("checkbox", { name: /RJ01678999\.zip/ }) as HTMLInputElement).checked).toBe(false);
  });

  it("shows Order and Ignore once more than one row exists", async () => {
    const installation = readyInstallation();
    installation.detection.contentItems = [
      contentItem("first.mp3"),
      contentItem("second.mp3"),
    ];
    renderReview(libraryGateway({ installation }));

    fireEvent.click(await screen.findByRole("button", { name: /Change .*content/i }));

    expect(screen.getByRole("columnheader", { name: "Order" })).toBeTruthy();
    expect(screen.getByRole("columnheader", { name: "Ignore" })).toBeTruthy();
  });

  it("keeps the source archive, format, and proposed-action evidence in package inspection", async () => {
    const installation = readyInstallation();
    installation.detection.packageInspection = packageInspection();
    renderReview(libraryGateway({ installation }));

    expect(await screen.findByText("RJ01678999.rar · RAR")).toBeTruthy();
    expect(screen.getByText("01.mp3")).toBeTruthy();
    expect(screen.getAllByText("High").length).toBeGreaterThan(0);
  });

  it("offers a safe keep-both destination without enabling overwrite", async () => {
    const installation = readyInstallation();
    installation.detection.packageInspection = packageInspection();
    const gateway = libraryGateway({ installation });
    gateway.selectInstallationDestination = vi.fn().mockResolvedValue({
      accessHandle: "destination-handle",
      displayPath: "/home/developer/DLA Library",
    });
    gateway.inspectPackageDestination = vi.fn().mockResolvedValue({
      state: "occupied_unknown",
      destinationName: "RJ01678999",
      keepBothDestinationName: "RJ01678999 (2)",
    });
    gateway.startPackagePreparation = vi.fn().mockResolvedValue({
      operationId: "preparation-1",
      installationId: "installation-1",
      stage: "queued",
      counters: { totalBytes: 0, processedBytes: 0, totalFiles: 0, processedFiles: 0 },
      currentPath: null,
      detail: "queued",
    });
    renderReview(gateway);

    fireEvent.click(await screen.findByRole("button", { name: "Choose destination" }));
    expect(await screen.findByText("That name is already in use")).toBeTruthy();
    const prepare = screen.getByRole("button", { name: "Prepare and verify" }) as HTMLButtonElement;
    expect(prepare.disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Keep both" }));
    expect(screen.getByText(/RJ01678999 \(2\)/)).toBeTruthy();
    expect(prepare.disabled).toBe(false);
    fireEvent.click(prepare);

    await waitFor(() => expect(gateway.startPackagePreparation).toHaveBeenCalledWith(
      "installation-1",
      "destination-handle",
      "keep_both",
      "delete_after_verified_install",
    ));
  });

  it("routes a destination owned by the same installation away from duplication", async () => {
    const installation = readyInstallation();
    installation.detection.packageInspection = packageInspection();
    const gateway = libraryGateway({ installation });
    gateway.selectInstallationDestination = vi.fn().mockResolvedValue({
      accessHandle: "destination-handle",
      displayPath: "/home/developer/DLA Library",
    });
    gateway.inspectPackageDestination = vi.fn().mockResolvedValue({
      state: "managed_same_installation",
      destinationName: "RJ01678999",
      keepBothDestinationName: null,
    });
    renderReview(gateway);

    fireEvent.click(await screen.findByRole("button", { name: "Choose destination" }));

    expect(await screen.findByText("This work already owns that destination")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Keep both" })).toBeNull();
    expect((screen.getByRole("button", { name: "Prepare and verify" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("does not offer Repair when the report says it cannot be repaired", async () => {
    renderReview(libraryGateway());

    await screen.findByRole("button", { name: /Verify/ });
    expect(screen.queryByRole("button", { name: /Repair/ })).toBeNull();
  });

  it("offers Repair only when the report is repairable", async () => {
    renderReview(libraryGateway({ health: healthReport({ state: "repairable", repairable: true }) }));

    expect(await screen.findByRole("button", { name: /Repair/ })).toBeTruthy();
  });

  it("promotes Locate when the installation has moved", async () => {
    renderReview(libraryGateway({ health: healthReport({ state: "moved" }) }));

    const locate = await screen.findAllByRole("button", { name: /Locate/ });
    expect(locate).toHaveLength(1);
  });

  it("confirms before uninstalling and names what is deleted", async () => {
    const gateway = libraryGateway();
    renderReview(gateway);

    fireEvent.click(await screen.findByRole("button", { name: /Uninstall/ }));

    const confirmation = await screen.findByRole("alertdialog");
    expect(gateway.uninstallInstallation).not.toHaveBeenCalled();
    expect(confirmation.textContent).toContain("/home/developer/DLA Library/RJ01678999");
    expect(confirmation.textContent).toContain("14");

    fireEvent.click(within(confirmation).getByRole("button", { name: /Yes|Confirm|Remove/ }));
    await waitFor(() => expect(gateway.uninstallInstallation).toHaveBeenCalledWith("installation-1"));
  });
});

function renderReview(gateway: LibraryGateway) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <KeyBindingsProvider>
          <LibraryReviewPage installationId="installation-1" gateway={gateway} onBack={() => undefined} />
        </KeyBindingsProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function libraryGateway(overrides: {
  installation?: Installation;
  health?: InstallationHealthReport;
} = {}): LibraryGateway {
  const installation = overrides.installation ?? readyInstallation();
  return {
    readInstallation: vi.fn().mockResolvedValue(installation),
    readInstallationHealth: vi.fn().mockResolvedValue(overrides.health ?? healthReport()),
    readPreparedPackage: vi.fn().mockResolvedValue(null),
    readPackagePreparationProgress: vi.fn().mockResolvedValue(null),
    subscribePackagePreparationProgress: vi.fn().mockResolvedValue(() => undefined),
    saveReview: vi.fn().mockResolvedValue(installation),
    verifyInstallation: vi.fn().mockResolvedValue(healthReport()),
    rescanInstallation: vi.fn().mockResolvedValue(healthReport()),
    repairInstallation: vi.fn().mockResolvedValue(healthReport()),
    locateInstallation: vi.fn().mockResolvedValue(healthReport()),
    selectInstallationLocation: vi.fn().mockResolvedValue(null),
    selectInstallationDestination: vi.fn().mockResolvedValue(null),
    inspectPackageDestination: vi.fn().mockResolvedValue({
      state: "available",
      destinationName: "RJ01678999",
      keepBothDestinationName: null,
    }),
    startPackagePreparation: vi.fn(),
    cancelPackagePreparation: vi.fn(),
    cleanupMaintenance: vi.fn(),
    removeInstallation: vi.fn().mockResolvedValue(undefined),
    uninstallInstallation: vi.fn().mockResolvedValue(undefined),
  } as unknown as LibraryGateway;
}

function readyInstallation(): Installation {
  return {
    id: "installation-1",
    scanRootId: "root-1",
    rootPath: "/home/developer/My Works",
    platform: "linux",
    status: "ready",
    detection: {
      sourceScanSessionId: "session-1",
      catalogIdentity: { workCode: "RJ01678999", confidence: "exact", reasonCodes: ["archive_sha256_match"] },
      suggestedStatus: "ready",
      contentItems: [contentItem("RJ01678999.zip")],
      launchCandidates: [],
      packageInspection: null,
    },
    overrides: {
      catalogIdentity: null,
      customTitle: null,
      preferredAction: null,
      contentItems: [],
    },
    discoveredAt: "2026-08-19T19:32:00Z",
    updatedAt: "2026-08-19T19:33:00Z",
  } as unknown as Installation;
}

function contentItem(relativePath: string) {
  return {
    relativePath,
    mediaType: "archive",
    confidence: "high",
    reasonCodes: ["archive_extension"],
  } as unknown as Installation["detection"]["contentItems"][number];
}

function packageInspection(): NonNullable<Installation["detection"]["packageInspection"]> {
  return {
    source: {
      scanEntryId: "archive-1",
      kind: "archive",
      relativePath: "RJ01678999.rar",
      sizeBytes: 512,
      sha256: "a".repeat(64),
    },
    format: "rar",
    safety: "safe",
    entryCount: 1,
    fileCount: 1,
    directoryCount: 0,
    totalCompressedBytes: 512,
    totalUncompressedBytes: 1_024,
    commonRoot: null,
    issues: [],
    classification: {
      contentKind: "audio_collection",
      engine: null,
      platform: "unknown",
      confidence: "high",
      reasonCodes: ["audio_dominant"],
      contentRoot: null,
      launchCandidates: [],
    },
    installPlan: {
      requiresExtraction: true,
      contentRoot: null,
      preferredAction: {
        action: "play_audio",
        relativePath: "01.mp3",
        supportedPlatforms: ["linux"],
        confidence: "high",
        reasonCodes: ["audio_dominant"],
      },
      archiveRetention: "delete_after_verified_install",
    },
    inspectedAt: "2026-08-19T19:35:00Z",
  };
}

function healthReport(overrides: Partial<InstallationHealthReport> = {}): InstallationHealthReport {
  return {
    installationId: "installation-1",
    state: "healthy",
    managed: true,
    repairable: false,
    checkedRoot: "/home/developer/DLA Library/RJ01678999",
    checkedAt: "2026-08-19T19:40:00Z",
    expectedFiles: 14,
    presentFiles: 14,
    missingFiles: 0,
    modifiedFiles: 0,
    inaccessibleFiles: 0,
    unexpectedFiles: 0,
    issues: [],
    ...overrides,
  } as unknown as InstallationHealthReport;
}
