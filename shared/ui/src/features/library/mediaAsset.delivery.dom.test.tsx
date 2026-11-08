// @vitest-environment jsdom

import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useMaterializedMediaSource } from "./mediaAsset";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("useMaterializedMediaSource", () => {
  it("streams from the media URL after one HEAD request when the webview can", async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, status: 200 });
    vi.stubGlobal("fetch", fetchMock);

    const view = renderHook(() => useMaterializedMediaSource("http://dla-media.localhost/s/0", "stream"));

    await waitFor(() => expect(view.result.current.loading).toBe(false));
    expect(view.result.current.url).toBe("http://dla-media.localhost/s/0");
    expect(view.result.current.failure).toBeNull();
    expect(fetchMock).toHaveBeenCalledOnce();
    expect(fetchMock.mock.calls[0]?.[1]).toMatchObject({ method: "HEAD" });
  });

  it("reports a missing asset the same way in both deliveries", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: false, status: 410 }));

    const streamed = renderHook(() => useMaterializedMediaSource("http://dla-media.localhost/s/1", "stream"));
    await waitFor(() => expect(streamed.result.current.loading).toBe(false));
    expect(streamed.result.current).toMatchObject({ url: "", failure: "missing" });

    const copied = renderHook(() => useMaterializedMediaSource("http://dla-media.localhost/s/1", "blob"));
    await waitFor(() => expect(copied.result.current.loading).toBe(false));
    expect(copied.result.current).toMatchObject({ url: "", failure: "missing" });
  });

  it("copies the whole track into a blob URL by default", async () => {
    const blob = new Blob(["audio"]);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, status: 200, blob: async () => blob }));
    const createObjectURL = vi.fn(() => "blob:track");
    const revokeObjectURL = vi.fn();
    vi.stubGlobal("URL", Object.assign(Object.create(URL), { createObjectURL, revokeObjectURL }));

    const view = renderHook(() => useMaterializedMediaSource("http://dla-media.localhost/s/2"));
    await waitFor(() => expect(view.result.current.url).toBe("blob:track"));
    expect(createObjectURL).toHaveBeenCalledWith(blob);

    view.unmount();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:track");
  });
});
