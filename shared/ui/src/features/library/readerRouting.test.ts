import { describe, expect, it } from "vitest";

import { isReaderAction } from "./ImageReaderProvider";

describe("isReaderAction", () => {
  it("claims image sets for the in-place reader overlay", () => {
    expect(isReaderAction("read_images")).toBe(true);
  });

  it("claims documents too, since much DLsite manga ships as PDF", () => {
    expect(isReaderAction("open_document")).toBe(true);
  });

  it("never claims playback actions", () => {
    expect(isReaderAction("play_audio")).toBe(false);
    expect(isReaderAction("play_video")).toBe(false);
    expect(isReaderAction("launch_executable")).toBe(false);
  });
});
