import { describe, expect, it } from "vitest";

import {
  findingName,
  findingSection,
  findingWorkCodes,
  groupFindings,
  looksLikeWorkCode,
} from "./scanFindings";
import type { ScanFinding } from "./types";

function finding(overrides: Partial<ScanFinding>): ScanFinding {
  return {
    key: "directory:work",
    relativePath: "work",
    kind: "folder",
    state: "ready",
    workCode: "RJ01000001",
    candidates: [{ workCode: "RJ01000001", confidence: "exact" }],
    fileCount: 3,
    sizeBytes: 300,
    volumeCount: 0,
    inLibrary: null,
    ignored: false,
    ...overrides,
  };
}

describe("scan findings", () => {
  it("lists each finding by what adding it needs", () => {
    expect(findingSection(finding({ state: "ready" }), false)).toBe("ready");
    expect(findingSection(finding({ state: "choose" }), false)).toBe("choose");
    expect(findingSection(finding({ state: "unknown" }), false)).toBe("unknown");
    expect(findingSection(finding({ state: "mixed" }), false)).toBe("blocked");
    expect(findingSection(finding({ state: "incomplete" }), false)).toBe("blocked");
  });

  it("moves works in the Library and ignored findings aside unless just acted on", () => {
    const added = finding({ inLibrary: { installationId: "installation-1", sameFiles: true, workCode: "RJ01000001" } });
    const ignored = finding({ key: "entry:old", ignored: true });

    expect(findingSection(added, false)).toBe("library");
    expect(findingSection(ignored, false)).toBe("ignored");
    expect(findingSection(added, true)).toBe("ready");

    const groups = groupFindings([added, ignored], new Set([added.key]));
    expect(groups.ready).toEqual([added]);
    expect(groups.ignored).toEqual([ignored]);
    expect(groups.library).toEqual([]);
  });

  it("collects every named work once", () => {
    expect(findingWorkCodes([
      finding({}),
      finding({
        key: "directory:kanojo",
        workCode: null,
        state: "choose",
        candidates: [
          { workCode: "RJ01000003", confidence: "strong" },
          { workCode: "RJ01000001", confidence: "possible" },
        ],
      }),
    ])).toEqual(["RJ01000001", "RJ01000003"]);
  });

  it("names a finding by its folder, file or scanned folder", () => {
    expect(findingName(finding({ relativePath: "Games/KUNOICHI RED" }), "D:\\Works")).toBe("KUNOICHI RED");
    expect(findingName(finding({ relativePath: "" }), "D:\\Works\\RJ01000001")).toBe("RJ01000001");
  });

  it("recognises a typed work code", () => {
    expect(looksLikeWorkCode(" rj01000001 ")).toBe(true);
    expect(looksLikeWorkCode("kunoichi")).toBe(false);
  });
});
