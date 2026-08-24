// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CatalogFacetFiltersProvider } from "../catalog";
import { PresentationProvider } from "../../preferences/PresentationProvider";
import { SearchModal } from "./SearchModal";
import type { SearchGateway, SearchIndexStatus, SearchRebuildProgress } from "./types";

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe("SearchModal index lifecycle", () => {
  it("shows measured rebuild progress and cancels the active operation", async () => {
    const progress: SearchRebuildProgress = {
      operationId: "rebuild-1",
      stage: "indexing",
      indexedDocuments: 256,
      totalDocuments: 1_012,
      detail: "Indexing catalog works",
    };
    const gateway = searchGateway({
      status: indexStatus("building"),
      progress,
    });

    renderSearch(gateway);

    const progressbar = await screen.findByRole("progressbar", {
      name: "Search index rebuild progress",
    });
    expect(progressbar.getAttribute("aria-valuenow")).toBe("256");
    expect(progressbar.getAttribute("aria-valuemax")).toBe("1012");
    expect(screen.getByText("256 of 1,012 works indexed")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Cancel rebuild" }));
    await waitFor(() => expect(gateway.cancelRebuild).toHaveBeenCalledWith("rebuild-1"));
  });

  it("cleans old derived generations without hiding the ready index", async () => {
    const gateway = searchGateway({ status: indexStatus("ready"), progress: null });
    renderSearch(gateway);

    fireEvent.click(await screen.findByRole("button", { name: "Clean old search data" }));

    await waitFor(() => expect(gateway.cleanupCache).toHaveBeenCalledOnce());
    expect(await screen.findByText("Removed 2 old search generations")).toBeTruthy();
    expect(screen.getByText("1,012 works indexed")).toBeTruthy();
  });

  it("keeps the cancelled state visible when the previous index is still ready", async () => {
    const gateway = searchGateway({
      status: indexStatus("ready"),
      progress: {
        operationId: "rebuild-2",
        stage: "cancelled",
        indexedDocuments: 512,
        totalDocuments: 1_012,
        detail: "Search rebuild cancelled",
      },
    });
    renderSearch(gateway);

    expect(await screen.findByText("Search preparation cancelled")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Continue rebuild" })).toBeTruthy();
    expect(screen.getByText("1,012 works indexed")).toBeTruthy();
  });
});

function renderSearch(gateway: SearchGateway) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <PresentationProvider>
        <CatalogFacetFiltersProvider>
          <SearchModal
            open
            gateway={gateway}
            onClose={() => undefined}
            onOpenWork={() => undefined}
          />
        </CatalogFacetFiltersProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function indexStatus(state: SearchIndexStatus["state"]): SearchIndexStatus {
  return {
    state,
    schemaVersion: 1,
    catalogSnapshotId: "snapshot",
    indexedDocuments: state === "ready" ? 1_012 : 256,
    generation: "generation-1",
    indexPath: "/cache/search/catalog/generation-1",
    detail: state === "ready" ? "ready" : "building",
  };
}

function searchGateway({
  status,
  progress,
}: {
  status: SearchIndexStatus;
  progress: SearchRebuildProgress | null;
}): SearchGateway {
  return {
    status: vi.fn().mockResolvedValue(status),
    rebuild: vi.fn(),
    cancelRebuild: vi.fn().mockResolvedValue(true),
    readRebuildProgress: vi.fn().mockResolvedValue(progress),
    cleanupCache: vi.fn().mockResolvedValue({
      removedIncompleteGenerations: 1,
      removedCompleteGenerations: 1,
      reclaimedBytes: 1_024,
      retainedCompleteGenerations: 2,
    }),
    subscribeRebuildProgress: vi.fn().mockResolvedValue(() => undefined),
    search: vi.fn(),
    shortcuts: vi.fn(),
  };
}
