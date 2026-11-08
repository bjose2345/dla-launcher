import { describe, expect, it } from "vitest";

import { basename } from "./basename";

describe("basename", () => {
  it("takes the last segment with either separator and keeps a bare name", () => {
    expect(basename("disc/track01.flac")).toBe("track01.flac");
    expect(basename("C:\\Games\\RJ01\\Game.exe")).toBe("Game.exe");
    expect(basename("/library/RJ01/")).toBe("RJ01");
    expect(basename("Game.exe")).toBe("Game.exe");
    expect(basename("")).toBe("");
  });
});
