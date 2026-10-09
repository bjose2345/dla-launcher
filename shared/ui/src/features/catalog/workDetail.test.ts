import { describe, expect, it } from "vitest";

import type { CatalogRom, CatalogWork } from "./types";
import {
  ageRatingLabel,
  archiveTotals,
  dlsiteWorkUrl,
  formatArchiveSize,
  heroImageUrls,
  sampleImageChains,
} from "./workDetail";

const work: CatalogWork = {
  code: "RJ01234567",
  sourceCode: "dlsite",
  title: "Title",
  titleEnglish: "",
  addedDate: "2026-01-03",
  releaseDate: "2026-01-02",
  updatedDate: "2026-01-04",
  ageRating: "r18",
  releaseType: "digital",
  mainImageUrls: ["main.webp"],
  thumbnailUrls: ["small.webp", "large.webp"],
  circles: [],
  categories: [],
  tags: [],
  synthetic: false,
};

describe("work detail presentation", () => {
  it("normalizes known age ratings and preserves unknown values", () => {
    const labels = { allAges: "All ages", notCataloged: "Not cataloged" };
    expect(ageRatingLabel("all-ages", labels)).toBe("All ages");
    expect(ageRatingLabel("r18", labels)).toBe("R18");
    expect(ageRatingLabel("custom", labels)).toBe("custom");
  });

  it("tries the main artwork before its thumbnail fallbacks", () => {
    expect(heroImageUrls(work)).toEqual(["main.webp", "large.webp", "small.webp"]);
    expect(work.mainImageUrls).toEqual(["main.webp"]);
    expect(work.thumbnailUrls).toEqual(["small.webp", "large.webp"]);
  });

  it("groups matching sample assets into ordered fallback chains", () => {
    expect(sampleImageChains([
      "https://img.dlsitearchive.com/works/RJ1/images/RJ1_img_smp1.webp",
      "https://img.dlsitearchive.com/works/RJ1/images/RJ1_img_smp2.webp",
      "https://img.dlsite.jp/modpub/images2/work/RJ1_img_smp1.webp",
      "https://img.dlsite.jp/modpub/images2/work/RJ1_img_smp2.webp",
      "https://img.dlsite.jp/modpub/images2/work/RJ1_img_smp2.webp",
    ])).toEqual([
      [
        "https://img.dlsitearchive.com/works/RJ1/images/RJ1_img_smp1.webp",
        "https://img.dlsite.jp/modpub/images2/work/RJ1_img_smp1.webp",
      ],
      [
        "https://img.dlsitearchive.com/works/RJ1/images/RJ1_img_smp2.webp",
        "https://img.dlsite.jp/modpub/images2/work/RJ1_img_smp2.webp",
      ],
    ]);
  });

  it("preserves sample fallback groups after native cache URL wrapping", () => {
    const cacheUrl = (source: string) => `dla-cover://localhost/${encodeURIComponent(source)}`;
    const mirrorOne = "https://img.dlsitearchive.com/works/RJ1/images/RJ1_img_smp1.webp";
    const mirrorTwo = "https://img.dlsitearchive.com/works/RJ1/images/RJ1_img_smp2.webp";
    const originalOne = "https://img.dlsite.jp/modpub/images2/work/RJ1_img_smp1.webp";
    const originalTwo = "https://img.dlsite.jp/modpub/images2/work/RJ1_img_smp2.webp";

    expect(sampleImageChains([
      cacheUrl(mirrorOne),
      cacheUrl(mirrorTwo),
      cacheUrl(originalOne),
      cacheUrl(originalTwo),
    ])).toEqual([
      [cacheUrl(mirrorOne), cacheUrl(originalOne)],
      [cacheUrl(mirrorTwo), cacheUrl(originalTwo)],
    ]);
  });

  it("builds only recognized DLsite product links", () => {
    expect(dlsiteWorkUrl(work)).toBe("https://www.dlsite.com/maniax/work/=/product_id/RJ01234567.html");
    expect(dlsiteWorkUrl({ ...work, code: "BJ123", sourceCode: "other" })).toBeNull();
    expect(dlsiteWorkUrl({ ...work, synthetic: true })).toBeNull();
    expect(dlsiteWorkUrl({ ...work, sourceCode: "DL", code: "BJ123" })).toBe("https://www.dlsite.com/books/work/=/product_id/BJ123.html");
  });
});

describe("work data panel", () => {
  const rom = (size: string, updateDate: string, version: string): CatalogRom => ({
    name: "archive",
    size,
    crc: "",
    md5: "",
    sha1: "",
    sha256: "",
    fileCount: null,
    updateDate,
    version,
  });

  it("adds every archive and counts the releases they form", () => {
    expect(archiveTotals([
      rom("300000000", "2026-09-16", "1.1"),
      rom("118000000", "2026-09-16", "1.1"),
      rom("410000000", "2026-09-04", ""),
    ])).toEqual({ bytes: 828000000, versions: 2 });
    expect(archiveTotals([])).toEqual({ bytes: 0, versions: 0 });
  });

  it("shows sizes in short decimal units", () => {
    expect(formatArchiveSize(418_000_000, "en-US")).toBe("418 MB");
    expect(formatArchiveSize(1_974_776_145, "en-US")).toBe("2 GB");
    expect(formatArchiveSize(512, "en-US")).toBe("512 B");
  });
});
