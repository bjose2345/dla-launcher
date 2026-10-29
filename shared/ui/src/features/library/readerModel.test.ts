import { describe, expect, it } from "vitest";

import {
  clampReaderZoom,
  defaultReaderPreferences,
  horizontalStep,
  isMangaWork,
  isWidePage,
  nearbyPages,
  nextReaderLayout,
  pageBox,
  readReaderPreferences,
  screenOrder,
  spreadPages,
  stepPage,
  writeReaderPreferences,
} from "./readerModel";

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
  };
}

describe("reader preferences", () => {
  it("reads manga right to left and everything else left to right", () => {
    expect(defaultReaderPreferences(true).direction).toBe("rtl");
    expect(defaultReaderPreferences(false).direction).toBe("ltr");
    expect(isMangaWork([{ code: "manga" }])).toBe(true);
    expect(isMangaWork([{ code: "digital_comic" }])).toBe(true);
    expect(isMangaWork([{ code: "cg_illustrations" }])).toBe(false);
  });

  it("keeps the choices made for a work over the defaults for its kind", () => {
    const storage = memoryStorage();
    const chosen = { ...defaultReaderPreferences(true), layout: "strip" as const, background: "paper" as const, zoom: 1.3 };

    writeReaderPreferences("installation-1", chosen, storage);

    expect(readReaderPreferences("installation-1", defaultReaderPreferences(false), storage)).toEqual(chosen);
    expect(readReaderPreferences("installation-2", defaultReaderPreferences(false), storage))
      .toEqual(defaultReaderPreferences(false));
  });

  it("falls back to the defaults for values it does not know and clamps zoom", () => {
    const storage = {
      getItem: () => JSON.stringify({ layout: "spread", direction: "up", fit: "cover", background: "pink", hideControls: "yes", zoom: 99 }),
      setItem: () => undefined,
    };

    expect(readReaderPreferences("installation-1", defaultReaderPreferences(true), storage))
      .toEqual({ ...defaultReaderPreferences(true), zoom: 4 });
    expect(clampReaderZoom(0.1)).toBe(0.5);
    expect(clampReaderZoom(Number.NaN)).toBe(1);
  });

  it("cycles through the reading modes", () => {
    expect(nextReaderLayout("single")).toBe("double");
    expect(nextReaderLayout("double")).toBe("strip");
    expect(nextReaderLayout("strip")).toBe("single");
  });
});

describe("turning pages", () => {
  const none = new Set<number>();

  it("shows the cover alone so every later pair stays as printed", () => {
    expect(spreadPages(0, 10, none)).toEqual([0]);
    expect(spreadPages(1, 10, none)).toEqual([1, 2]);
    expect(spreadPages(2, 10, none)).toEqual([1, 2]);
    expect(spreadPages(9, 10, none)).toEqual([9]);
  });

  it("gives a wide page the whole screen instead of pairing it", () => {
    const wide = new Set([3]);
    expect(spreadPages(3, 10, wide)).toEqual([3]);
    expect(spreadPages(4, 10, wide)).toEqual([4]);
    expect(spreadPages(2, 10, wide)).toEqual([1, 2]);
  });

  it("steps a whole spread at a time and stops at both ends", () => {
    expect(stepPage(1, 10, 1, "double", none)).toBe(3);
    expect(stepPage(3, 10, -1, "double", none)).toBe(1);
    expect(stepPage(0, 10, -1, "double", none)).toBe(0);
    expect(stepPage(9, 10, 1, "double", none)).toBe(9);
    expect(stepPage(4, 10, 1, "single", none)).toBe(5);
    expect(stepPage(9, 10, 1, "single", none)).toBe(9);
  });

  it("puts the first page on the right when reading right to left", () => {
    expect(screenOrder([1, 2], "rtl")).toEqual([2, 1]);
    expect(screenOrder([1, 2], "ltr")).toEqual([1, 2]);
    expect(horizontalStep("left", "rtl")).toBe(1);
    expect(horizontalStep("right", "rtl")).toBe(-1);
    expect(horizontalStep("left", "ltr")).toBe(-1);
  });

  it("keeps the thumbnail strip full near both ends", () => {
    expect(nearbyPages(0, 26)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
    expect(nearbyPages(12, 26)).toEqual([8, 9, 10, 11, 12, 13, 14, 15, 16]);
    expect(nearbyPages(25, 26)).toEqual([17, 18, 19, 20, 21, 22, 23, 24, 25]);
    expect(nearbyPages(1, 3)).toEqual([0, 1, 2]);
  });

  it("detects a wide page from its size", () => {
    expect(isWidePage(1600, 1000)).toBe(true);
    expect(isWidePage(800, 1200)).toBe(false);
    expect(isWidePage(800, 0)).toBe(false);
  });
});

describe("page size", () => {
  const stage = { width: 1200, height: 800 };
  const portrait = { width: 700, height: 1000 };

  it("fits the whole page in the window's height", () => {
    expect(pageBox(portrait, stage, "height", 1, 1)).toEqual({ width: 560, height: 800 });
  });

  it("narrows two pages so both fit side by side", () => {
    const box = pageBox(portrait, { width: 1000, height: 800 }, "height", 2, 1);
    expect(box.width).toBe(500);
    expect(Math.round(box.height)).toBe(714);
  });

  it("fills the width, or keeps the page's own size, and zooms on top", () => {
    expect(pageBox(portrait, stage, "width", 1, 1).width).toBe(1200);
    expect(pageBox(portrait, stage, "original", 1, 1).width).toBe(700);
    expect(Math.round(pageBox({ width: 595, height: 842 }, stage, "original", 1, 1, true).width)).toBe(793);
    expect(pageBox(portrait, stage, "height", 1, 1.5)).toEqual({ width: 840, height: 1200 });
  });
});
