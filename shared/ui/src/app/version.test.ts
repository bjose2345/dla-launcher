import { describe, expect, it } from "vitest";

import { isPreReleaseVersion } from "./version";

describe("isPreReleaseVersion", () => {
  it("treats pre-release markers as developer builds", () => {
    expect(isPreReleaseVersion("v0.1.0-alpha.1")).toBe(true);
    expect(isPreReleaseVersion("v1.2.0-beta.3")).toBe(true);
    expect(isPreReleaseVersion("2.0.0-rc.1")).toBe(true);
    expect(isPreReleaseVersion("beta.2")).toBe(true);
  });

  it("treats a clean release as stable", () => {
    expect(isPreReleaseVersion("v1.0.0")).toBe(false);
    expect(isPreReleaseVersion("v2.3.4")).toBe(false);
  });

  it("does not mistake a build number for a pre-release", () => {
    expect(isPreReleaseVersion("v1.0.0+20260820")).toBe(false);
  });
});
