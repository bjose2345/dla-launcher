import { describe, expect, it } from "vitest";

import {
  coverColor,
  readVisualColor,
  readVisualStyle,
  readingLevel,
  showsWholeTrack,
  visualPaint,
  writeVisualColor,
  writeVisualStyle,
} from "./audioVisual";

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
  };
}

function pixels(...colors: Array<[number, number, number]>): number[] {
  return colors.flatMap(([red, green, blue]) => [red, green, blue, 255]);
}

describe("audio visual", () => {
  it("starts with mirror bars in the theme's colour and keeps what was chosen", () => {
    const storage = memoryStorage();
    expect(readVisualStyle(storage)).toBe("mirror");
    expect(readVisualColor(storage)).toBe("theme");

    writeVisualStyle("ring", storage);
    writeVisualColor("rainbow", storage);
    expect(readVisualStyle(storage)).toBe("ring");
    expect(readVisualColor(storage)).toBe("rainbow");

    storage.setItem("dla.library.audio-visual-style", "waveform");
    expect(readVisualStyle(storage)).toBe("mirror");
  });

  it("knows which styles show the whole track", () => {
    expect(["bars", "mirror", "area"].every((style) => showsWholeTrack(style as "bars"))).toBe(true);
    expect(["spectrum", "mirrorSpectrum", "oscilloscope", "ring"].some((style) => showsWholeTrack(style as "ring")))
      .toBe(false);
  });

  it("draws the part still to come fainter than the part already heard", () => {
    const theme = { primary: "pink", content: "white" };
    for (const color of ["theme", "cover", "white", "ice", "rainbow"] as const) {
      const paint = visualPaint(color, theme, "hsl(270 70% 68%)");
      expect(paint.played(3, 10).alpha).toBe(1);
      expect(paint.rest(3, 10).alpha).toBeLessThan(1);
    }
    expect(visualPaint("theme", theme, null).played(0, 1).color).toBe("pink");
    expect(visualPaint("theme", theme, null).rest(0, 1).color).toBe("white");
  });

  it("takes the cover's colour, and the theme's while the cover gives none", () => {
    const theme = { primary: "pink", content: "white" };
    expect(visualPaint("cover", theme, "hsl(270 70% 68%)").played(0, 1).color).toBe("hsl(270 70% 68%)");
    expect(visualPaint("cover", theme, null).played(0, 1).color).toBe("pink");
  });

  it("runs the rainbow along the marks", () => {
    const paint = visualPaint("rainbow", { primary: "pink", content: "white" }, null);
    expect(paint.played(0, 10).color).not.toBe(paint.played(9, 10).color);
  });

  it("finds the hue most of a cover's vivid pixels share", () => {
    const purple: [number, number, number] = [150, 70, 220];
    const color = coverColor(pixels(purple, purple, purple, [240, 240, 240], [20, 20, 20], [200, 180, 60]));
    const hue = Number(/^hsl\((\d+) /.exec(color ?? "")?.[1]);
    expect(hue).toBeGreaterThan(250);
    expect(hue).toBeLessThan(290);
    expect(color).toMatch(/ 68%\)$/);
  });

  it("gives no colour for a cover in greys", () => {
    expect(coverColor(pixels([10, 10, 10], [128, 128, 128], [250, 250, 250]))).toBeNull();
    expect(coverColor([])).toBeNull();
  });

  it("keeps the reading pulse low and moving", () => {
    const levels = Array.from({ length: 64 }, (_, index) => readingLevel(index, 1.2));
    expect(Math.min(...levels)).toBeGreaterThanOrEqual(0.06);
    expect(Math.max(...levels)).toBeLessThanOrEqual(0.26);
    expect(levels).not.toEqual(Array.from({ length: 64 }, (_, index) => readingLevel(index, 1.5)));
  });
});
