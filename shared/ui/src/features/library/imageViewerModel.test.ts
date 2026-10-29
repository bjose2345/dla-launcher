import { describe, expect, it } from "vitest";

import {
  defaultImageViewerPreferences,
  filmstripWindow,
  fitScale,
  formatFileSize,
  imageBox,
  imageFolders,
  minimapSize,
  readImageViewerPreferences,
  scrollToPoint,
  viewerSequence,
  visibleRegion,
  writeImageViewerPreferences,
} from "./imageViewerModel";

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
  };
}

const paths = (...relativePaths: string[]) => relativePaths.map((relativePath) => ({ relativePath }));

describe("image folders", () => {
  it("names each folder below the one all images share, in the order they come", () => {
    const folders = imageFolders(paths("Set/main/01.png", "Set/main/02.png", "Set/notext/01.png", "Set/bonus/a.png"));

    expect(folders.map((folder) => folder.name)).toEqual(["main", "notext", "bonus"]);
    expect(folders.map((folder) => folder.indexes)).toEqual([[0, 1], [2], [3]]);
  });

  it("leaves the shared folder unnamed when images sit in it and below it", () => {
    expect(imageFolders(paths("cover.png", "extra/a.png")).map((folder) => folder.name)).toEqual(["", "extra"]);
    expect(imageFolders(paths("Set/01.png", "Set/02.png")).map((folder) => folder.name)).toEqual([""]);
  });

  it("shows the whole set or one folder of it", () => {
    const folders = imageFolders(paths("a/1.png", "a/2.png", "b/1.png"));

    expect(viewerSequence(folders, 3, null)).toEqual([0, 1, 2]);
    expect(viewerSequence(folders, 3, "b")).toEqual([2]);
    expect(viewerSequence(folders, 3, "missing")).toEqual([0, 1, 2]);
  });
});

describe("filmstrip", () => {
  it("keeps the strip full near both ends of the set", () => {
    expect(filmstripWindow(0, 30)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    expect(filmstripWindow(15, 30)).toEqual([9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21]);
    expect(filmstripWindow(29, 30)).toEqual([17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29]);
    expect(filmstripWindow(1, 3)).toEqual([0, 1, 2]);
  });
});

describe("image size", () => {
  const stage = { width: 1280, height: 800 };
  const landscape = { width: 1920, height: 1080 };
  const portrait = { width: 1000, height: 1500 };

  it("fits the whole image on the screen", () => {
    expect(imageBox(landscape, stage, "screen", null)).toEqual({ width: 1280, height: 720 });
    expect(imageBox(portrait, stage, "screen", null).height).toBe(800);
  });

  it("fills the width, keeps the image's own size, or zooms its pixels", () => {
    expect(imageBox(portrait, stage, "width", null)).toEqual({ width: 1280, height: 1920 });
    expect(imageBox(landscape, stage, "original", null)).toEqual(landscape);
    expect(imageBox(landscape, stage, "screen", 2)).toEqual({ width: 3840, height: 2160 });
    expect(fitScale(landscape, stage, "original")).toBe(1);
  });

  it("holds a wide frame until the image's size is known", () => {
    expect(imageBox(undefined, stage, "screen", null)).toEqual({ width: 1280, height: 720 });
  });
});

describe("panner", () => {
  const viewport = { width: 1000, height: 600 };
  const image = { left: 0, top: 0, width: 4000, height: 2400 };

  it("frames the part of a zoomed image the screen shows", () => {
    expect(visibleRegion({ left: 1500, top: 900 }, viewport, image)).toEqual({ left: 0.375, top: 0.375, width: 0.25, height: 0.25 });
    expect(visibleRegion({ left: 0, top: 0 }, viewport, { left: 200, top: 0, width: 600, height: 1200 }))
      .toEqual({ left: 0, top: 0, width: 1, height: 0.5 });
  });

  it("centres the screen on the point picked in the panner", () => {
    expect(scrollToPoint({ x: 0.5, y: 0.5 }, viewport, image)).toEqual({ left: 1500, top: 900 });
    expect(scrollToPoint({ x: 2, y: -1 }, viewport, image)).toEqual({ left: 3500, top: -300 });
  });

  it("keeps the whole image inside a small frame", () => {
    expect(minimapSize({ width: 1920, height: 1080 }, false)).toEqual({ width: 168, height: 94.5 });
    expect(minimapSize({ width: 1000, height: 1500 }, true).height).toBe(88);
  });
});

describe("viewer preferences", () => {
  it("opens image sets in the CG viewer and keeps the choices made for a work", () => {
    const storage = memoryStorage();
    expect(readImageViewerPreferences("installation-1", storage)).toEqual(defaultImageViewerPreferences());

    const chosen = { ...defaultImageViewerPreferences(), openIn: "reader" as const, interval: 10, background: "ambient" as const };
    writeImageViewerPreferences("installation-1", chosen, storage);

    expect(readImageViewerPreferences("installation-1", storage)).toEqual(chosen);
    expect(readImageViewerPreferences("installation-2", storage).openIn).toBe("viewer");
  });

  it("falls back to the defaults for values it does not know", () => {
    const storage = {
      getItem: () => JSON.stringify({ fit: "cover", background: "pink", interval: 7, loop: "yes", openIn: "elsewhere", gallerySize: "huge" }),
      setItem: () => undefined,
    };

    expect(readImageViewerPreferences("installation-1", storage)).toEqual(defaultImageViewerPreferences());
  });
});

describe("file size", () => {
  it("reads like a file manager", () => {
    expect(formatFileSize(512, "en-US")).toBe("512 B");
    expect(formatFileSize(3_400_000, "en-US")).toBe("3.2 MB");
  });
});
