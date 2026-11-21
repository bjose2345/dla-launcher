// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useWarmWaveforms } from "./TrackWaveform";
import type { AudioWaveform, LibraryGateway } from "./types";

afterEach(() => {
  cleanup();
});

function Warm({ gateway, ordinals }: { gateway: Pick<LibraryGateway, "readAudioWaveform">; ordinals: number[] }) {
  useWarmWaveforms(gateway, "installation-1", ordinals);
  return null;
}

describe("useWarmWaveforms", () => {
  it("reads the coming tracks one at a time, in order, and each only once", async () => {
    const finish: Array<(waveform: AudioWaveform) => void> = [];
    const readAudioWaveform = vi.fn(() => new Promise<AudioWaveform>((resolve) => { finish.push(resolve); }));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const view = render(
      <QueryClientProvider client={client}>
        <Warm gateway={{ readAudioWaveform }} ordinals={[2, 3, 0]} />
      </QueryClientProvider>,
    );

    await waitFor(() => expect(readAudioWaveform).toHaveBeenCalledTimes(1));
    expect(readAudioWaveform).toHaveBeenLastCalledWith("installation-1", 2, 256);

    finish[0]!({ peaks: [], durationMs: 1_000 });
    await waitFor(() => expect(readAudioWaveform).toHaveBeenCalledTimes(2));
    expect(readAudioWaveform).toHaveBeenLastCalledWith("installation-1", 3, 256);

    view.rerender(
      <QueryClientProvider client={client}>
        <Warm gateway={{ readAudioWaveform }} ordinals={[3, 0, 2]} />
      </QueryClientProvider>,
    );
    finish[1]!({ peaks: [], durationMs: 1_000 });
    await waitFor(() => expect(readAudioWaveform).toHaveBeenCalledTimes(3));
    expect(readAudioWaveform).toHaveBeenLastCalledWith("installation-1", 0, 256);

    finish[2]!({ peaks: [], durationMs: 1_000 });
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(readAudioWaveform).toHaveBeenCalledTimes(3);
  });
});
