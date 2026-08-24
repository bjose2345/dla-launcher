// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { CatalogImportPage } from "./CatalogImportPage";
import type {
  CatalogImportGateway,
  CatalogGenerationSummary,
  CatalogImportPreview,
  CatalogImportProgress,
} from "./types";

afterEach(() => {
  cleanup();
});

describe("CatalogImportPage feedback", () => {
  it("keeps package internals closed and shows a plain-language safety message", async () => {
    const gateway = importerGateway();
    renderImporter(gateway);

    fireEvent.click(screen.getByRole("button", { name: "Choose package" }));

    await screen.findByText("catalog.dla");
    const details = screen.getByText("Advanced details").closest("details");
    expect(details?.open).toBe(false);
    expect(screen.getByText("Your current catalog will stay available if the import fails.")).toBeTruthy();
    expect(document.body.textContent).not.toContain("SQLite");
    expect(document.body.textContent).not.toContain("Tantivy");
  });

  it("shows startup state and errors beside the import action", async () => {
    let rejectStart: (reason: Error) => void = () => undefined;
    const startResult = new Promise<CatalogImportProgress>((_resolve, reject) => {
      rejectStart = reject;
    });
    const gateway = importerGateway();
    gateway.start = vi.fn().mockReturnValue(startResult);
    renderImporter(gateway);

    fireEvent.click(screen.getByRole("button", { name: "Choose package" }));
    await screen.findByText("catalog.dla");
    const preview = screen.getByText("catalog.dla").closest(".import-preview");
    if (!preview) throw new Error("import preview was not rendered");

    fireEvent.click(within(preview as HTMLElement).getByRole("button", { name: "Import catalog" }));
    const startingButton = await within(preview as HTMLElement).findByRole("button", { name: "Starting import…" }) as HTMLButtonElement;
    expect(startingButton.disabled).toBe(true);

    rejectStart(new Error("bridge unavailable"));
    const alert = await within(preview as HTMLElement).findByRole("alert");
    expect(alert.textContent).toContain("The import could not be started.");
    expect(alert.textContent).toContain("bridge unavailable");
  });

  it("reveals progress after the import starts", async () => {
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView,
    });
    const gateway = importerGateway();
    gateway.start = vi.fn().mockResolvedValue(queuedProgress());
    renderImporter(gateway);

    fireEvent.click(screen.getByRole("button", { name: "Choose package" }));
    await screen.findByText("catalog.dla");
    fireEvent.click(screen.getByRole("button", { name: "Import catalog" }));

    const progressbar = await screen.findByRole("progressbar", { name: "Catalog import progress" });
    expect(progressbar.classList.contains("is-indeterminate")).toBe(true);
    expect(progressbar.getAttribute("aria-valuenow")).toBeNull();
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
  });

  it("shows live entity and byte progress while building", async () => {
    const gateway = importerGateway();
    gateway.start = vi.fn().mockResolvedValue({
      ...queuedProgress(),
      stage: "building_catalog",
      counters: {
        ...queuedProgress().counters,
        processedBytes: 512,
        uniqueWorks: 128,
      },
    });
    renderImporter(gateway);

    fireEvent.click(screen.getByRole("button", { name: "Choose package" }));
    await screen.findByText("catalog.dla");
    fireEvent.click(screen.getByRole("button", { name: "Import catalog" }));

    const progressbar = await screen.findByRole("progressbar", { name: "Catalog import progress" });
    expect(progressbar.getAttribute("aria-valuenow")).toBe("25");
    expect(screen.getByText("Works read")).toBeTruthy();
    expect(screen.getByText("128")).toBeTruthy();
    expect(screen.getByText("512 B / 2 KiB")).toBeTruthy();
    expect(screen.getByText("Elapsed")).toBeTruthy();
  });

  it("shows package filenames, falls back for legacy rows, and confirms removal", async () => {
    const gateway = importerGateway();
    gateway.listGenerations = vi.fn().mockResolvedValue([
      catalogGeneration({
        id: "embedded",
        kind: "embedded",
        state: "available",
        profile: "custom",
        sourceName: "Bundled test catalog",
        packageName: "",
      }),
      catalogGeneration({
        id: "legacy-import",
        state: "active",
        sourceName: "DLsite",
        packageName: "",
        importedAt: "2026-08-18T22:03:26Z",
      }),
      catalogGeneration({
        id: "imported-1",
        state: "available",
        sourceName: "DLsite",
        packageName: "catalog-full-20260818T220326Z.dla",
      }),
    ]);
    renderImporter(gateway);

    const packageName = await screen.findByText("catalog-full-20260818T220326Z.dla");
    expect(packageName.tagName).toBe("STRONG");
    expect(screen.getByText("Built-in catalog").tagName).toBe("STRONG");
    expect(screen.queryByText(/Bundled test catalog/)).toBeNull();
    const legacyLabel = screen.getByText(/^Imported · /);
    expect(legacyLabel.tagName).toBe("STRONG");
    expect(screen.getAllByRole("button", { name: "Remove" })).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    const confirmation = await screen.findByRole("alert");
    expect(confirmation.textContent).toContain("Remove catalog-full-20260818T220326Z.dla?");
    expect(confirmation.textContent).toContain("Your original .dla file is not deleted.");
    expect(gateway.removeGeneration).not.toHaveBeenCalled();

    expect(within(confirmation).getByRole("button", { name: "Keep it" })).toBeTruthy();
    fireEvent.click(within(confirmation).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(gateway.removeGeneration).toHaveBeenCalledWith("imported-1"));
  });

  it("does not present the empty internal catalog baseline as an installation", async () => {
    const gateway = importerGateway();
    gateway.listGenerations = vi.fn().mockResolvedValue([
      catalogGeneration({
        id: "embedded",
        kind: "embedded",
        state: "active",
        profile: "custom",
        sourceName: "Empty catalog baseline",
        packageName: "",
        workCount: 0,
        romCount: 0,
      }),
    ]);
    renderImporter(gateway);

    await screen.findByText("Installed catalogs");
    expect(screen.queryByText("Built-in catalog")).toBeNull();
    expect(screen.queryByText("1 installed")).toBeNull();
  });

  it("reports a stopped import without claiming the catalog changed", async () => {
    const gateway = importerGateway();
    gateway.readProgress = vi.fn().mockResolvedValue({
      ...queuedProgress(),
      stage: "failed",
      counters: { ...queuedProgress().counters, processedBytes: 1024 },
      detail: "disk quota exceeded while writing generation 0004",
    });
    renderImporter(gateway);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("changes were made to your catalog");
    expect(alert.textContent).not.toContain("50%");
    expect(alert.textContent).toContain("Any incomplete catalog is not in use.");
    const details = screen.getByText("Advanced details").closest("details");
    expect(details?.open).toBe(false);
    expect(details?.textContent).toContain("disk quota exceeded while writing generation 0004");
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    expect(screen.getByRole("button", { name: "Choose another" })).toBeTruthy();
  });

  it("returns to review when a different package is chosen after a stopped import", async () => {
    const gateway = importerGateway();
    gateway.readProgress = vi.fn().mockResolvedValue({
      ...queuedProgress(),
      stage: "failed",
      detail: "disk quota exceeded",
    });
    renderImporter(gateway);

    fireEvent.click(await screen.findByRole("button", { name: "Choose another" }));

    await screen.findByText("catalog.dla");
    expect(screen.queryByText("changes were made to your catalog")).toBeNull();
  });

  it("retries a failed catalog activation without importing the reviewed package", async () => {
    const gateway = importerGateway();
    gateway.listGenerations = vi.fn().mockResolvedValue([
      catalogGeneration({ id: "active", state: "active" }),
      catalogGeneration({ id: "available", state: "available", packageName: "available.dla" }),
    ]);
    gateway.activate = vi.fn().mockResolvedValue({
      ...queuedProgress(),
      operationKind: "activation",
      stage: "failed",
      detail: "search rebuild failed",
    });
    renderImporter(gateway);

    fireEvent.click(screen.getByRole("button", { name: "Choose package" }));
    await screen.findByRole("button", { name: "Import catalog" });
    fireEvent.click(await screen.findByRole("button", { name: "Use this catalog" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("Catalog switch stopped");
    expect(alert.textContent).toContain("Your previous catalog is still active.");
    expect(screen.queryByText("Choose file")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(gateway.activate).toHaveBeenCalledTimes(2));
    expect(gateway.activate).toHaveBeenNthCalledWith(1, "available");
    expect(gateway.activate).toHaveBeenNthCalledWith(2, "available");
    expect(gateway.start).not.toHaveBeenCalled();
  });

  it("does not offer an unusable retry for a restored catalog activation", async () => {
    const gateway = importerGateway();
    gateway.readProgress = vi.fn().mockResolvedValue({
      ...queuedProgress(),
      operationKind: "activation",
      stage: "failed",
      detail: "search rebuild failed",
    });
    renderImporter(gateway);

    await screen.findByRole("alert");
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Back" }));

    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(gateway.activate).not.toHaveBeenCalled();
    expect(gateway.start).not.toHaveBeenCalled();
  });

  it("offers the catalog as the next step once the import finishes", async () => {
    const onOpenCatalog = vi.fn();
    const gateway = importerGateway();
    gateway.readProgress = vi.fn().mockResolvedValue({
      ...queuedProgress(),
      stage: "completed",
      counters: { ...queuedProgress().counters, uniqueWorks: 128431 },
    });
    gateway.listGenerations = vi.fn().mockResolvedValue([
      catalogGeneration({
        id: "imported-1",
        state: "active",
        packageName: "catalog-full-20260818T220326Z.dla",
      }),
    ]);
    renderImporter(gateway, onOpenCatalog);

    const browse = await screen.findByRole("button", { name: "Browse catalog" });
    expect(screen.getByText("128,431")).toBeTruthy();
    expect(screen.getByText("works are ready to browse")).toBeTruthy();

    fireEvent.click(browse);
    expect(onOpenCatalog).toHaveBeenCalledTimes(1);
  });
});

function renderImporter(gateway: CatalogImportGateway, onOpenCatalog?: () => void) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <CatalogImportPage gateway={gateway} onOpenCatalog={onOpenCatalog} />
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function importerGateway(): CatalogImportGateway {
  return {
    selectPackage: vi.fn().mockResolvedValue({ accessHandle: "package-1", displayName: "catalog.dla" }),
    inspect: vi.fn().mockResolvedValue(packagePreview()),
    start: vi.fn().mockResolvedValue(queuedProgress()),
    cancel: vi.fn().mockResolvedValue(true),
    readProgress: vi.fn().mockResolvedValue(null),
    listGenerations: vi.fn().mockResolvedValue([]),
    activate: vi.fn().mockResolvedValue(queuedProgress()),
    removeGeneration: vi.fn().mockResolvedValue(undefined),
    subscribeProgress: vi.fn().mockResolvedValue(() => undefined),
  };
}

function packagePreview(): CatalogImportPreview {
  return {
    accessHandle: "package-1",
    displayName: "catalog.dla",
    compressedBytes: 1024,
    uncompressedBytes: 2048,
    requiredDiskBytes: 4096,
    availableDiskBytes: 8192,
    compatible: true,
    blockingIssues: [],
    warnings: [],
    manifest: {
      format: "dla.catalog-package",
      formatVersion: 1,
      catalogSchemaVersion: 1,
      minimumLauncherVersion: "0.1.0",
      snapshotId: "snapshot-1",
      createdAt: "2026-08-19T00:00:00Z",
      profile: "compact",
      source: { id: "fixture", name: "Fixture catalog" },
      fields: ["work.code"],
      counts: {
        workEntries: 10,
        uniqueWorks: 10,
        roms: 20,
        files: 0,
        relations: 0,
      },
      payloads: [],
    },
    omittedFields: [],
  };
}

function queuedProgress(): CatalogImportProgress {
  return {
    operationId: "operation-1",
    operationKind: "import",
    snapshotId: "snapshot-1",
    stage: "queued",
    counters: {
      processedBytes: 0,
      totalBytes: 2048,
      workEntries: 0,
      uniqueWorks: 0,
      roms: 0,
      files: 0,
      relations: 0,
    },
    currentPayload: "",
    detail: "",
  };
}

function catalogGeneration(
  overrides: Partial<CatalogGenerationSummary> = {},
): CatalogGenerationSummary {
  return {
    id: "generation-1",
    snapshotId: "snapshot-1",
    kind: "imported",
    state: "available",
    profile: "full",
    sourceName: "Fixture catalog",
    packageName: "catalog.dla",
    importedAt: "2026-08-19T00:00:00Z",
    workCount: 10,
    romCount: 20,
    databaseBytes: 1024,
    fields: ["work.code"],
    failureDetail: "",
    ...overrides,
  };
}
