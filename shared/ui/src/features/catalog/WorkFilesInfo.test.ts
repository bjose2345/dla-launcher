import { describe, expect, it } from "vitest";

import type { CatalogRom } from "./types";
import { formatSize, groupRoms } from "./WorkFilesInfo";

function rom(name: string, updateDate: string, version = ""): CatalogRom {
  return {
    name,
    size: "6574453837",
    crc: "",
    md5: "",
    sha1: "",
    sha256: "",
    fileCount: null,
    updateDate,
    version,
  };
}

describe("file information", () => {
  it("groups ROMs by effective date and version newest first", () => {
    const groups = groupRoms([
      rom("base.zip", "", "1.0"),
      rom("update.zip", "2026-07-30", "1.1"),
    ], "2025-01-02", "Unknown date");

    expect(groups.map(([label]) => label)).toEqual(["2026-07-30 (1.1)", "2025-01-02 (1.0)"]);
  });

  it("shows a readable size while preserving the exact byte count", () => {
    expect(formatSize("6574453837", "en-US", "bytes")).toBe("6.6 GB (6,574,453,837 bytes)");
    expect(formatSize("", "en-US", "bytes")).toBe("—");
  });
});
