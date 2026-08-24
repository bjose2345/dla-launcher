import { describe, expect, it } from "vitest";

import { assetFailureMessageKey, mediaAssetFailure } from "./mediaAsset";

describe("mediaAssetFailure", () => {
  it("separates a file missing from disk from an unknown item", () => {
    expect(mediaAssetFailure(410)).toBe("missing");
    expect(mediaAssetFailure(404)).toBe("unavailable");
  });

  it("reports a permission problem as its own failure", () => {
    expect(mediaAssetFailure(403)).toBe("forbidden");
  });

  it("treats every other status as unavailable", () => {
    expect(mediaAssetFailure(500)).toBe("unavailable");
    expect(mediaAssetFailure(416)).toBe("unavailable");
  });
});

describe("assetFailureMessageKey", () => {
  it("maps each failure to its own localized message", () => {
    expect(assetFailureMessageKey("missing")).toBe("media.assetMissing");
    expect(assetFailureMessageKey("forbidden")).toBe("media.assetForbidden");
    expect(assetFailureMessageKey("unavailable")).toBe("media.assetUnavailable");
  });
});
