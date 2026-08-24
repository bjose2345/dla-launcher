import { describe, expect, it, vi } from "vitest";

import {
  installReadOnlyDeepLinkNavigation,
  parseReadOnlyDeepLink,
  type ReadOnlyDeepLinkGateway,
} from "./deepLinks";

describe("parseReadOnlyDeepLink", () => {
  it.each([
    ["dla-launcher://works/RJ01326398", "RJ01326398"],
    ["dla-launcher://works/bj12345", "BJ12345"],
    ["dla-launcher://WORKS/vj1234567890", "VJ1234567890"],
  ])("accepts a read-only work route", (value, code) => {
    expect(parseReadOnlyDeepLink(value)).toEqual({ kind: "work", code });
  });

  it.each([
    "https://works/RJ01326398",
    "dla-launcher://scanner/RJ01326398",
    "dla-launcher://import/RJ01326398",
    "dla-launcher://launch/RJ01326398",
    "dla-launcher://works.example/RJ01326398",
    "dla-launcher://user@works/RJ01326398",
    "dla-launcher://works:443/RJ01326398",
    "dla-launcher://works/RJ01326398?launch=true",
    "dla-launcher://works/RJ01326398?",
    "dla-launcher://works/RJ01326398#section",
    "dla-launcher://works/RJ01326398#",
    "dla-launcher://works/RJ01326398/extra",
    "dla-launcher://works/RJ01326398/",
    "dla-launcher://works/%52J01326398",
    "dla-launcher://works/%2e/RJ01326398",
    " dla-launcher://works/RJ01326398",
    "dla-launcher://works/RJ01326398 ",
    "dla-launcher://works/RJ1234",
    "dla-launcher://works/RJ12345678901",
    "dla-launcher://works/RJ12345.exe",
    "dla-launcher://workſ/RJ01326398",
  ])("rejects unsupported or ambiguous input: %s", (value) => {
    expect(parseReadOnlyDeepLink(value)).toBeNull();
  });
});

describe("installReadOnlyDeepLinkNavigation", () => {
  it("delivers cold-start and runtime links while ignoring unsupported routes", async () => {
    let listener: ((urls: readonly string[]) => void) | undefined;
    const unlisten = vi.fn();
    const gateway: ReadOnlyDeepLinkGateway = {
      readCurrent: async () => [
        "dla-launcher://works/RJ01326398",
        "dla-launcher://scanner/RJ01326398",
      ],
      subscribe: async (next) => {
        listener = next;
        return unlisten;
      },
    };
    const navigate = vi.fn();

    const stop = await installReadOnlyDeepLinkNavigation(gateway, navigate);
    listener?.(["dla-launcher://works/BJ12345"]);
    listener?.(["dla-launcher://works/BJ12345"]);

    expect(navigate).toHaveBeenNthCalledWith(1, { kind: "work", code: "RJ01326398" });
    expect(navigate).toHaveBeenNthCalledWith(2, { kind: "work", code: "BJ12345" });
    expect(navigate).toHaveBeenNthCalledWith(3, { kind: "work", code: "BJ12345" });
    stop();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("does not deliver the same event twice when it arrives during initial discovery", async () => {
    const value = "dla-launcher://works/RJ01326398";
    const navigate = vi.fn();
    const gateway: ReadOnlyDeepLinkGateway = {
      subscribe: async (listener) => {
        listener([value]);
        return () => undefined;
      },
      readCurrent: async () => [value],
    };

    await installReadOnlyDeepLinkNavigation(gateway, navigate);

    expect(navigate).toHaveBeenCalledOnce();
  });

  it("does not subscribe before the native readiness read succeeds", async () => {
    const subscribe = vi.fn();
    const gateway: ReadOnlyDeepLinkGateway = {
      subscribe,
      readCurrent: async () => {
        throw new Error("unavailable");
      },
    };

    await expect(installReadOnlyDeepLinkNavigation(gateway, vi.fn())).rejects.toThrow("unavailable");
    expect(subscribe).not.toHaveBeenCalled();
  });

  it("releases the subscription when synchronized discovery fails", async () => {
    const unlisten = vi.fn();
    let reads = 0;
    const gateway: ReadOnlyDeepLinkGateway = {
      subscribe: async () => unlisten,
      readCurrent: async () => {
        reads += 1;
        if (reads === 1) return [];
        throw new Error("unavailable");
      },
    };

    await expect(installReadOnlyDeepLinkNavigation(gateway, vi.fn())).rejects.toThrow("unavailable");
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
