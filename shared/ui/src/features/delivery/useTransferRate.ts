import { useEffect, useRef, useState } from "react";

import type { DeliveryProgress } from "./types";

const WINDOW_MS = 15_000;

interface Sample {
  at: number;
  bytes: number;
}

/**
 * Bytes per second of the running phase, over the last fifteen seconds. It
 * starts over when the job or the phase changes, so unpacking speed is not
 * mixed with download speed.
 */
export function useTransferRate(progress: DeliveryProgress | null): number {
  const samples = useRef<Sample[]>([]);
  const key = useRef("");
  const [rate, setRate] = useState(0);
  useEffect(() => {
    if (!progress) {
      samples.current = [];
      key.current = "";
      setRate(0);
      return;
    }
    const current = `${progress.jobId}:${progress.phase}`;
    const now = Date.now();
    if (current !== key.current) {
      key.current = current;
      samples.current = [];
    }
    samples.current = [...samples.current, { at: now, bytes: progress.receivedBytes }]
      .filter((sample) => now - sample.at <= WINDOW_MS);
    const first = samples.current[0]!;
    const seconds = (now - first.at) / 1000;
    setRate(seconds >= 2 ? Math.max(progress.receivedBytes - first.bytes, 0) / seconds : 0);
  }, [progress]);
  return rate;
}
