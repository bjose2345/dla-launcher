import { describe, expect, it } from "vitest";

import {
  mergePackagePreparationProgress,
  packagePreparationCanCancel,
  packagePreparationIsIndeterminate,
  packagePreparationIsTerminal,
  packagePreparationNeedsPreparedRefresh,
  type PackagePreparationProgress,
  type PackagePreparationStage,
} from "./types";

function progress(
  stage: PackagePreparationStage,
  overrides: Partial<PackagePreparationProgress> = {},
): PackagePreparationProgress {
  return {
    operationId: "operation-1",
    installationId: "installation-1",
    stage,
    counters: {
      totalBytes: 1_000,
      processedBytes: 0,
      totalFiles: 10,
      processedFiles: 0,
    },
    currentPath: null,
    detail: stage,
    ...overrides,
  };
}

describe("package preparation presentation state", () => {
  it.each<PackagePreparationStage>(["completed", "cancelled", "failed"])(
    "treats %s as terminal",
    (stage) => expect(packagePreparationIsTerminal(stage)).toBe(true),
  );

  it.each<PackagePreparationStage>(["queued", "validating", "extracting", "activating", "cleaning_sources"])(
    "keeps %s visibly active without fabricated progress",
    (stage) => expect(packagePreparationIsIndeterminate(stage)).toBe(true),
  );

  it("reserves determinate progress for verification and terminal states", () => {
    expect(packagePreparationIsIndeterminate("verifying")).toBe(false);
    expect(packagePreparationIsTerminal("verifying")).toBe(false);
  });

  it.each<PackagePreparationStage>(["queued", "validating", "extracting", "verifying", "activating"])(
    "allows cancellation during %s",
    (stage) => expect(packagePreparationCanCancel(stage)).toBe(true),
  );

  it.each<PackagePreparationStage>(["cleaning_sources", "completed", "cancelled", "failed"])(
    "does not offer cancellation after activation during %s",
    (stage) => expect(packagePreparationCanCancel(stage)).toBe(false),
  );

  it("does not let a delayed start response replace newer operation progress", () => {
    const extracting = progress("extracting", {
      counters: {
        totalBytes: 1_000,
        processedBytes: 500,
        totalFiles: 10,
        processedFiles: 4,
      },
    });

    expect(mergePackagePreparationProgress(extracting, progress("queued"))).toBe(extracting);
  });

  it("does not regress counters when progress events arrive out of order", () => {
    const newer = progress("extracting", {
      counters: {
        totalBytes: 1_000,
        processedBytes: 500,
        totalFiles: 10,
        processedFiles: 4,
      },
    });
    const older = progress("extracting", {
      counters: {
        totalBytes: 1_000,
        processedBytes: 400,
        totalFiles: 10,
        processedFiles: 3,
      },
    });

    expect(mergePackagePreparationProgress(newer, older)).toBe(newer);
  });

  it("accepts terminal progress and a new operation", () => {
    const extracting = progress("extracting");
    const completed = progress("completed");
    const next = progress("queued", { operationId: "operation-2" });

    expect(mergePackagePreparationProgress(extracting, completed)).toBe(completed);
    expect(mergePackagePreparationProgress(completed, next)).toBe(next);
  });

  it("refreshes the durable prepared record for every matching terminal result", () => {
    for (const stage of ["completed", "cancelled", "failed"] as const) {
      expect(packagePreparationNeedsPreparedRefresh(progress(stage), "installation-1")).toBe(true);
    }
    expect(packagePreparationNeedsPreparedRefresh(progress("completed"), "installation-2")).toBe(false);
    expect(packagePreparationNeedsPreparedRefresh(progress("activating"), "installation-1")).toBe(false);
  });
});
