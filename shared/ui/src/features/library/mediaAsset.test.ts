import { afterEach, describe, expect, it, vi } from "vitest";

import { fetchMediaAsset, probeMediaAsset } from "./mediaAsset";

describe("fetchMediaAsset", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("materializes private media bytes without caching", async () => {
    const blob = new Blob(["audio"], { type: "audio/mpeg" });
    const response = {
      ok: true,
      status: 200,
      blob: vi.fn().mockResolvedValue(blob),
    };
    const request = vi.fn().mockResolvedValue(response);
    vi.stubGlobal("fetch", request);
    const controller = new AbortController();

    await expect(fetchMediaAsset("dla-media://localhost/media-1%2F0", controller.signal))
      .resolves.toBe(blob);
    expect(request).toHaveBeenCalledWith(
      "dla-media://localhost/media-1%2F0",
      { cache: "no-store", signal: controller.signal },
    );
  });

  it("rejects unsuccessful private media responses", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
      ok: false,
      status: 416,
      blob: vi.fn(),
    }));

    await expect(fetchMediaAsset(
      "dla-media://localhost/media-1%2F0",
      new AbortController().signal,
    )).rejects.toThrow("Media request returned 416");
  });

  it("probes a streaming asset without materializing its bytes", async () => {
    const request = vi.fn().mockResolvedValue({ ok: true, status: 200 });
    vi.stubGlobal("fetch", request);
    const controller = new AbortController();

    await expect(probeMediaAsset(
      "dla-media://localhost/media-1%2F2",
      controller.signal,
    )).resolves.toBeUndefined();
    expect(request).toHaveBeenCalledWith(
      "dla-media://localhost/media-1%2F2",
      { cache: "no-store", method: "HEAD", signal: controller.signal },
    );
  });
});
