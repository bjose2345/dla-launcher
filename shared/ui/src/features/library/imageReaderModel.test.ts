import { describe, expect, it } from "vitest";

import {
  adjacentReaderItems,
  clampImageReaderZoom,
  isWidePage,
  spreadDisplayOrder,
  spreadPageOrdinals,
  spreadStep,
  defaultImageReaderPreferences,
  imageReaderChapters,
  imageReaderProfilePreferences,
  readImageReaderPreferences,
  readerClickStep,
  readerHorizontalStep,
  readerParentPath,
  writeImageReaderPreferences,
} from "./imageReaderModel";
import type { MediaSessionItem } from "./types";

describe("image reader model", () => {
  it("uses deliberate gallery and manga profile defaults", () => {
    expect(imageReaderProfilePreferences("gallery")).toEqual(defaultImageReaderPreferences);
    expect(imageReaderProfilePreferences("manga")).toMatchObject({
      profile: "manga",
      layout: "spread",
      direction: "rtl",
      fit: "height",
    });
  });

  it("persists validated preferences per installation", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    };
    const preferences = {
      profile: "manga" as const,
      layout: "continuous" as const,
      direction: "rtl" as const,
      fit: "width" as const,
      zoom: 1.35,
    };

    writeImageReaderPreferences("installation-1", preferences, storage);

    expect(readImageReaderPreferences("installation-1", storage)).toEqual(preferences);
    expect(readImageReaderPreferences("installation-2", storage)).toEqual(defaultImageReaderPreferences);
  });

  it("rejects corrupt values and clamps zoom", () => {
    const storage = {
      getItem: () => JSON.stringify({
        profile: "unknown",
        layout: "spread",
        direction: "up",
        fit: "cover",
        zoom: 99,
      }),
      setItem: () => undefined,
    };
    expect(readImageReaderPreferences("installation", storage)).toEqual({
      ...defaultImageReaderPreferences,
      zoom: 4,
    });
    expect(clampImageReaderZoom(Number.NaN)).toBe(1);
    expect(clampImageReaderZoom(0.1)).toBe(0.5);
  });

  it("groups naturally ordered pages by their parent folder", () => {
    const chapters = imageReaderChapters([
      item(0, "cover.webp"),
      item(1, "chapter 2/001.webp"),
      item(2, "chapter 2/002.webp"),
      item(3, "chapter 10/001.webp"),
    ]);
    expect(chapters.map((chapter) => [chapter.path, chapter.items.map((page) => page.ordinal)])).toEqual([
      ["", [0]],
      ["chapter 2", [1, 2]],
      ["chapter 10", [3]],
    ]);
    expect(readerParentPath("nested\\chapter/page.png")).toBe("nested/chapter");
  });

  it("finds adjacent preload pages without repeating the current page", () => {
    const items = [0, 1, 2, 3, 4].map((ordinal) => item(ordinal, `${ordinal}.webp`));
    expect(adjacentReaderItems(items, 2).map((page) => page.ordinal)).toEqual([0, 1, 3, 4]);
    expect(adjacentReaderItems(items, 0).map((page) => page.ordinal)).toEqual([1, 2]);
  });

  it("maps horizontal keys and click zones to the reading direction", () => {
    expect(readerHorizontalStep("ArrowRight", "ltr")).toBe(1);
    expect(readerHorizontalStep("ArrowLeft", "ltr")).toBe(-1);
    expect(readerHorizontalStep("ArrowRight", "rtl")).toBe(-1);
    expect(readerClickStep("left", "rtl")).toBe(1);
  });
});

function item(ordinal: number, relativePath: string): MediaSessionItem {
  return {
    ordinal,
    installationId: "installation",
    workCode: null,
    relativePath,
    mediaType: "image",
    sizeBytes: null,
    discNumber: null,
    trackNumber: null,
    bonus: false,
  };
}

describe("spread pairing", () => {
  const pages = (count: number): MediaSessionItem[] => Array.from({ length: count }, (_, i) => ({
    ordinal: i + 1,
    installationId: "work",
    workCode: null,
    relativePath: `p${String(i + 1).padStart(3, "0")}.png`,
    mediaType: "image" as const,
    sizeBytes: null,
    discNumber: null,
    trackNumber: null,
    bonus: false,
  }));
  const none = new Set<string>();

  it("shows the cover alone so every later pair stays aligned", () => {
    const items = pages(8);
    expect(spreadPageOrdinals(items, 1, none)).toEqual([1]);
    expect(spreadPageOrdinals(items, 2, none)).toEqual([2, 3]);
    expect(spreadPageOrdinals(items, 3, none)).toEqual([2, 3]);
    expect(spreadPageOrdinals(items, 4, none)).toEqual([4, 5]);
  });

  it("gives a landscape page the whole screen instead of pairing it", () => {
    const items = pages(8);
    const wide = new Set(["p004.png"]);
    expect(spreadPageOrdinals(items, 4, wide)).toEqual([4]);
    expect(spreadPageOrdinals(items, 5, wide)).toEqual([5]);
  });

  it("does not pair past the end of the book", () => {
    const items = pages(5);
    expect(spreadPageOrdinals(items, 4, none)).toEqual([4, 5]);
    expect(spreadPageOrdinals(items, 5, none)).toEqual([4, 5]);
  });

  it("reverses only the display order for right-to-left reading", () => {
    expect(spreadDisplayOrder([2, 3], "rtl")).toEqual([3, 2]);
    expect(spreadDisplayOrder([2, 3], "ltr")).toEqual([2, 3]);
    expect(spreadDisplayOrder([1], "rtl")).toEqual([1]);
  });

  it("advances a whole spread at a time and stops at the covers", () => {
    const items = pages(8);
    expect(spreadStep(items, 1, 1, none)).toBe(2);
    expect(spreadStep(items, 2, 1, none)).toBe(4);
    expect(spreadStep(items, 4, -1, none)).toBe(2);
    expect(spreadStep(items, 2, -1, none)).toBe(1);
    expect(spreadStep(items, 1, -1, none)).toBe(1);
    expect(spreadStep(items, 8, 1, none)).toBe(8);
  });

  it("detects a landscape page from its natural size", () => {
    expect(isWidePage(1600, 1200)).toBe(true);
    expect(isWidePage(800, 1200)).toBe(false);
    expect(isWidePage(1200, 0)).toBe(false);
  });
});
