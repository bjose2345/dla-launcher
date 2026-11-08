import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";

import { catalogKeys } from "../catalog/catalogQueryKeys";
import { formatByteSize, refreshAfterCatalogOperation } from "./CatalogImportPage";
import { catalogImportIsIndeterminate } from "./types";
import { operationIsTerminal } from "../../app/operationStage";

describe("catalog import presentation", () => {
  it("formats package sizes without decimal noise", () => {
    expect(formatByteSize(0)).toBe("0 B");
    expect(formatByteSize(1024)).toBe("1 KiB");
    expect(formatByteSize(3 * 1024 * 1024 * 1024)).toBe("3 GiB");
  });

  it("keeps only completed, cancelled and failed operations terminal", () => {
    expect(operationIsTerminal("completed")).toBe(true);
    expect(operationIsTerminal("cancelled")).toBe(true);
    expect(operationIsTerminal("failed")).toBe(true);
    expect(operationIsTerminal("rebuilding_search")).toBe(false);
  });

  it("uses indeterminate progress after package bytes have been consumed", () => {
    expect(catalogImportIsIndeterminate("finalizing_catalog")).toBe(true);
    expect(catalogImportIsIndeterminate("checkpointing_catalog")).toBe(true);
    expect(catalogImportIsIndeterminate("validating_catalog")).toBe(true);
    expect(catalogImportIsIndeterminate("rebuilding_search")).toBe(true);
    expect(catalogImportIsIndeterminate("building_catalog")).toBe(false);
  });
});

describe("refreshAfterCatalogOperation", () => {
  it("marks every catalog query stale, including the titles the Library and search show", async () => {
    const queryClient = new QueryClient();
    const keys = [
      catalogKeys.works(["RJ000001", "RJ000002"]),
      catalogKeys.work("RJ000001"),
      catalogKeys.recommendations("RJ000001"),
      catalogKeys.romContents("RJ000001", 0),
      catalogKeys.search("text", "", 1),
      catalogKeys.searchShortcuts("te"),
      catalogKeys.searchIndex,
    ];
    for (const key of keys) queryClient.setQueryData(key, {});

    await refreshAfterCatalogOperation(queryClient);

    for (const key of keys) expect(queryClient.getQueryState(key)?.isInvalidated, JSON.stringify(key)).toBe(true);
  });
});
