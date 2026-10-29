import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { installRenderWatchdog } from "./renderWatchdog";

interface FakeWindow {
  target: Window;
  input: () => void;
  /** Whether each frame request is answered, in order; later requests repeat the last answer. */
  frames: boolean[];
  visibility: DocumentVisibilityState;
}

function fakeWindow(frames: boolean[]): FakeWindow {
  const events = new EventTarget();
  const fake: FakeWindow = {
    frames,
    visibility: "visible",
    input: () => events.dispatchEvent(new Event("pointerdown")),
    target: undefined as unknown as Window,
  };
  let requests = 0;
  fake.target = {
    document: {
      get visibilityState() {
        return fake.visibility;
      },
    },
    performance: { now: () => Date.now() },
    setTimeout: (callback: () => void, delay: number) => setTimeout(callback, delay),
    requestAnimationFrame: (callback: FrameRequestCallback) => {
      const answered = fake.frames[Math.min(requests, fake.frames.length - 1)];
      requests += 1;
      return answered ? setTimeout(() => callback(Date.now()), 16) : -1;
    },
    cancelAnimationFrame: () => undefined,
    addEventListener: events.addEventListener.bind(events),
    removeEventListener: events.removeEventListener.bind(events),
  } as unknown as Window;
  return fake;
}

describe("installRenderWatchdog", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(100_000);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("asks for recovery when a visible page in use draws no frame", async () => {
    const page = fakeWindow([false]);
    const recover = vi.fn();
    installRenderWatchdog(recover, page.target);

    page.input();
    await vi.advanceTimersByTimeAsync(2_100);

    expect(recover).toHaveBeenCalledTimes(1);
  });

  it("leaves a page alone that draws", async () => {
    const page = fakeWindow([true]);
    const recover = vi.fn();
    installRenderWatchdog(recover, page.target);

    page.input();
    await vi.advanceTimersByTimeAsync(2_100);

    expect(recover).not.toHaveBeenCalled();
  });

  it("does not check a hidden page", async () => {
    const page = fakeWindow([false]);
    page.visibility = "hidden";
    const recover = vi.fn();
    installRenderWatchdog(recover, page.target);

    page.input();
    await vi.advanceTimersByTimeAsync(2_100);

    expect(recover).not.toHaveBeenCalled();
  });

  it("forgives a page that misses one frame while busy", async () => {
    const page = fakeWindow([false, true]);
    const recover = vi.fn();
    installRenderWatchdog(recover, page.target);

    page.input();
    await vi.advanceTimersByTimeAsync(2_100);

    expect(recover).not.toHaveBeenCalled();
  });

  it("waits before asking for recovery again", async () => {
    const page = fakeWindow([false]);
    const recover = vi.fn();
    installRenderWatchdog(recover, page.target);

    page.input();
    await vi.advanceTimersByTimeAsync(2_100);
    page.input();
    await vi.advanceTimersByTimeAsync(4_000);
    expect(recover).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(8_000);
    page.input();
    await vi.advanceTimersByTimeAsync(2_100);
    expect(recover).toHaveBeenCalledTimes(2);
  });

  it("stops checking once removed", async () => {
    const page = fakeWindow([false]);
    const recover = vi.fn();
    const remove = installRenderWatchdog(recover, page.target);

    remove();
    page.input();
    await vi.advanceTimersByTimeAsync(2_100);

    expect(recover).not.toHaveBeenCalled();
  });
});
