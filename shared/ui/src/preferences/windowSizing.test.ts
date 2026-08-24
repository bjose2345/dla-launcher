import { describe, expect, it } from "vitest";

import { clampWindowSize } from "./windowSizing";

describe("shared window sizing", () => {
  it("clamps large presets to the current monitor work area", () => {
    expect(
      clampWindowSize(
        { width: 3840, height: 2160 },
        { width: 1920, height: 1040 },
      ),
    ).toEqual({ width: 1920, height: 1040 });
  });

  it("preserves a supported preset", () => {
    expect(
      clampWindowSize(
        { width: 1280, height: 720 },
        { width: 2560, height: 1400 },
      ),
    ).toEqual({ width: 1280, height: 720 });
  });
});
