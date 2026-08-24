import { describe, expect, it } from "vitest";

import { formatByteSize } from "./CatalogImportPage";
import { catalogImportIsIndeterminate, catalogImportIsTerminal } from "./types";

describe("catalog import presentation", () => {
  it("formats package sizes without decimal noise", () => {
    expect(formatByteSize(0)).toBe("0 B");
    expect(formatByteSize(1024)).toBe("1 KiB");
    expect(formatByteSize(3 * 1024 * 1024 * 1024)).toBe("3 GiB");
  });

  it("keeps only completed, cancelled and failed operations terminal", () => {
    expect(catalogImportIsTerminal("completed")).toBe(true);
    expect(catalogImportIsTerminal("cancelled")).toBe(true);
    expect(catalogImportIsTerminal("failed")).toBe(true);
    expect(catalogImportIsTerminal("rebuilding_search")).toBe(false);
  });

  it("uses indeterminate progress after package bytes have been consumed", () => {
    expect(catalogImportIsIndeterminate("finalizing_catalog")).toBe(true);
    expect(catalogImportIsIndeterminate("checkpointing_catalog")).toBe(true);
    expect(catalogImportIsIndeterminate("validating_catalog")).toBe(true);
    expect(catalogImportIsIndeterminate("rebuilding_search")).toBe(true);
    expect(catalogImportIsIndeterminate("building_catalog")).toBe(false);
  });
});
