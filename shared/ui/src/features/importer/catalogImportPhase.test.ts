import { describe, expect, it } from "vitest";

import {
  catalogImportPercent,
  catalogImportPhase,
  catalogImportPhases,
  type CatalogImportProgress,
} from "./types";

describe("catalogImportPhase", () => {
  it("treats queueing and validation as one checking phase", () => {
    expect(catalogImportPhase("queued")).toBe("checking");
    expect(catalogImportPhase("validating")).toBe("checking");
  });

  it("keeps the catalog build as its own phase", () => {
    expect(catalogImportPhase("building_catalog")).toBe("building");
  });

  it("groups enrichment and relations as adding details", () => {
    expect(catalogImportPhase("applying_enrichment")).toBe("details");
    expect(catalogImportPhase("applying_relations")).toBe("details");
  });

  it("hides every storage and search step behind finishing", () => {
    for (const stage of [
      "finalizing_catalog",
      "checkpointing_catalog",
      "validating_catalog",
      "activating_catalog",
      "rebuilding_search",
      "completed",
    ] as const) {
      expect(catalogImportPhase(stage)).toBe("finishing");
    }
  });

  it("maps every stage to one of the four phases", () => {
    const stages = [
      "queued", "validating", "building_catalog", "applying_enrichment",
      "applying_relations", "finalizing_catalog", "checkpointing_catalog",
      "validating_catalog", "activating_catalog", "rebuilding_search",
      "completed", "cancelled", "failed",
    ] as const;
    for (const stage of stages) {
      expect(catalogImportPhases).toContain(catalogImportPhase(stage));
    }
  });
});

describe("catalogImportPercent", () => {
  const progress = (
    stage: CatalogImportProgress["stage"],
    processedBytes: number,
    totalBytes: number,
  ): CatalogImportProgress => ({
    operationId: "op",
    operationKind: "import",
    snapshotId: "snap",
    stage,
    counters: { processedBytes, totalBytes, workEntries: 0, uniqueWorks: 0, roms: 0, files: 0, relations: 0 },
    currentPayload: "",
    detail: "",
  });

  it("reports byte progress while a payload is streaming", () => {
    expect(catalogImportPercent(progress("building_catalog", 50, 200))).toBe(25);
  });

  it("returns null for stages with no measurable byte work", () => {
    expect(catalogImportPercent(progress("rebuilding_search", 10, 200))).toBeNull();
    expect(catalogImportPercent(progress("building_catalog", 10, 0))).toBeNull();
    expect(catalogImportPercent(progress("building_catalog", 0, 200))).toBeNull();
    expect(catalogImportPercent(progress("building_catalog", 1, 1_000))).toBeNull();
  });

  it("never reports outside nought to a hundred", () => {
    expect(catalogImportPercent(progress("building_catalog", 500, 200))).toBe(100);
    expect(catalogImportPercent(progress("building_catalog", -5, 200))).toBe(0);
  });
});
