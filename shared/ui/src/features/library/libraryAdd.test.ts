import { describe, expect, it } from "vitest";

import type { CatalogWork } from "../catalog";
import type { ScanFinding } from "../scanner/types";
import { addable, addFindingName, addItem, addStatus, folderRelease, jobPercent, type LibraryAddFinding } from "./libraryAdd";

describe("library add", () => {
  it("reads the date and version from a name such as RJ01234567 (2024-05-01-01)", () => {
    expect(folderRelease("RJ01234567 (2024-05-01-01)")).toEqual({ date: "2024-05-01", version: "01" });
    expect(folderRelease("[Circle] Title (2023-11-02)")).toEqual({ date: "2023-11-02", version: null });
    expect(folderRelease("RJ01234567")).toBeNull();
  });

  it("says when the catalog has a newer version than the folder", () => {
    const work = { code: "RJ01234567", latestUpdate: { date: "2024-07-10", version: "1.1" } } as unknown as CatalogWork;
    expect(addStatus(finding(), "RJ01234567 (2024-05-01-01)", work)).toBe("newer");
    expect(addStatus(finding(), "RJ01234567 (2024-08-01-01)", work)).toBe("catalog");
    expect(addStatus(finding(), "RJ01234567", work)).toBe("catalog");
  });

  it("sorts each found work by what adding it needs", () => {
    expect(addStatus(finding({ state: "choose" }), "x", undefined)).toBe("choose");
    expect(addStatus(finding({ state: "unknown" }), "x", undefined)).toBe("own");
    expect(addStatus(finding({ state: "incomplete" }), "x", undefined)).toBe("incomplete");
    expect(addStatus(finding({ state: "mixed" }), "x", undefined)).toBe("mixed");
    expect(addStatus(finding({ inLibrary: { installationId: "i", sameFiles: true, workCode: null } }), "x", undefined)).toBe("library");
    expect(addable("library")).toBe(false);
    expect(addable("own")).toBe(true);
  });

  it("asks for a work only where the catalog can't name one", () => {
    const item = found(finding({ state: "choose" }));
    expect(addItem(item, "choose", undefined)).toBeNull();
    expect(addItem(item, "choose", { kind: "work", workCode: "RJ02" })).toEqual({ sourceId: "s1", key: "root", workCode: "RJ02", unknown: false });
    expect(addItem(item, "choose", { kind: "own" })).toEqual({ sourceId: "s1", key: "root", workCode: null, unknown: true });
    expect(addItem(found(finding()), "catalog", undefined)).toEqual({ sourceId: "s1", key: "root", workCode: null, unknown: false });
    expect(addItem(found(finding({ state: "unknown" })), "own", undefined)).toEqual({ sourceId: "s1", key: "root", workCode: null, unknown: true });
  });

  it("names a work by its folder, or by the picked folder when that is the work", () => {
    const sources = [{ sourceId: "s1", displayName: "RJ01234567 (2024-05-01-01)", kind: "folder" as const, status: "done" as const, error: null, sessionId: "x" }];
    expect(addFindingName(found(finding()), sources)).toBe("RJ01234567 (2024-05-01-01)");
    expect(addFindingName(found(finding({ relativePath: "Works/My set" })), sources)).toBe("My set");
  });

  it("shows how far a copy has come", () => {
    const job = { sourceId: "s1", key: "root", kind: "copy" as const, state: "running" as const, installationId: null, error: null, warning: null, doneBytes: 45, totalBytes: 100 };
    expect(jobPercent(job)).toBe(45);
    expect(jobPercent({ ...job, doneBytes: null })).toBeNull();
  });
});

function found(value: ScanFinding): LibraryAddFinding {
  return { sourceId: "s1", finding: value, placement: "move" };
}

function finding(patch: Partial<ScanFinding> = {}): ScanFinding {
  return {
    key: "root",
    relativePath: "",
    kind: "folder",
    state: "ready",
    workCode: "RJ01234567",
    candidates: [{ workCode: "RJ01234567", confidence: "exact" }],
    fileCount: 10,
    sizeBytes: 1024,
    volumeCount: 0,
    incompleteSet: null,
    inLibrary: null,
    ignored: false,
    ...patch,
  };
}
