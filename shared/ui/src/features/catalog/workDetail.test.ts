import { describe, expect, it } from "vitest";

import type { CatalogWork } from "./types";
import { ageRatingLabel, dlsiteWorkUrl, heroImageUrls, sampleImageChains } from "./workDetail";

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

  it("builds only recognized DLsite product links", () => {
    expect(dlsiteWorkUrl(work)).toBe("https://www.dlsite.com/maniax/work/=/product_id/RJ01234567.html");
    expect(dlsiteWorkUrl({ ...work, code: "BJ123", sourceCode: "other" })).toBeNull();
    expect(dlsiteWorkUrl({ ...work, synthetic: true })).toBeNull();
    expect(dlsiteWorkUrl({ ...work, sourceCode: "DL", code: "BJ123" })).toBe("https://www.dlsite.com/books/work/=/product_id/BJ123.html");
  });
});
