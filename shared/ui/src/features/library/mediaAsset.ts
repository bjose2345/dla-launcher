import { useEffect, useState } from "react";

import { errorMessage } from "../../app/errorMessage";
import type { MediaDelivery } from "./types";

export interface MaterializedMediaSource {
  source: string;
  url: string;
  loading: boolean;
  error: string;
  failure: MediaAssetFailure | null;
}

export interface MediaAssetProbe {
  source: string;
  loading: boolean;
  error: string;
  failure: MediaAssetFailure | null;
}

export type MediaAssetFailure = "missing" | "forbidden" | "unavailable";

export class MediaAssetError extends Error {
  constructor(readonly failure: MediaAssetFailure, readonly status: number) {
    super(`Media request returned ${status}`);
    this.name = "MediaAssetError";
  }
}

export function mediaAssetFailure(status: number): MediaAssetFailure {
  if (status === 410) return "missing";
  if (status === 403) return "forbidden";
  return "unavailable";
}

export function assetFailureMessageKey(failure: MediaAssetFailure) {
  if (failure === "missing") return "media.assetMissing" as const;
  if (failure === "forbidden") return "media.assetForbidden" as const;
  return "media.assetUnavailable" as const;
}

export async function fetchMediaAsset(source: string, signal: AbortSignal): Promise<Blob> {
  const response = await fetch(source, { cache: "no-store", signal });
  if (!response.ok) throw new MediaAssetError(mediaAssetFailure(response.status), response.status);
  return response.blob();
}

export async function probeMediaAsset(source: string, signal: AbortSignal): Promise<void> {
  const response = await fetch(source, { cache: "no-store", method: "HEAD", signal });
  if (!response.ok) throw new MediaAssetError(mediaAssetFailure(response.status), response.status);
}

export function useMediaAssetProbe(source: string, retryVersion = 0): MediaAssetProbe {
  const [state, setState] = useState<MediaAssetProbe>({
    source,
    loading: source !== "",
    error: "",
    failure: null,
  });

  useEffect(() => {
    if (!source) {
      setState({ source, loading: false, error: "", failure: null });
      return;
    }
    const controller = new AbortController();
    let disposed = false;
    setState({ source, loading: true, error: "", failure: null });
    void probeMediaAsset(source, controller.signal)
      .then(() => {
        if (!disposed) setState({ source, loading: false, error: "", failure: null });
      })
      .catch((error: unknown) => {
        if (disposed || (error instanceof DOMException && error.name === "AbortError")) return;
        setState({
          source,
          loading: false,
          error: errorMessage(error),
          failure: error instanceof MediaAssetError ? error.failure : "unavailable",
        });
      });
    return () => {
      disposed = true;
      controller.abort();
    };
  }, [retryVersion, source]);

  return state.source === source
    ? state
    : { source, loading: true, error: "", failure: null };
}

/**
 * The URL the audio element plays: the media URL itself once a HEAD request
 * has confirmed the asset (stream delivery), or a blob of the whole track
 * (blob delivery, for a webview that cannot stream the protocol). Either way a
 * missing or refused asset reports its failure before anything plays.
 */
export function useMaterializedMediaSource(source: string, delivery: MediaDelivery = "blob"): MaterializedMediaSource {
  const [state, setState] = useState<MaterializedMediaSource>({
    source,
    url: "",
    loading: source !== "",
    error: "",
    failure: null,
  });

  useEffect(() => {
    if (!source) {
      setState({ source, url: "", loading: false, error: "", failure: null });
      return;
    }
    const controller = new AbortController();
    let disposed = false;
    let objectUrl = "";
    setState({ source, url: "", loading: true, error: "", failure: null });
    const materialize = delivery === "stream"
      ? probeMediaAsset(source, controller.signal).then(() => source)
      : fetchMediaAsset(source, controller.signal).then((blob) => {
        objectUrl = URL.createObjectURL(blob);
        return objectUrl;
      });
    void materialize
      .then((url) => {
        if (!disposed) setState({ source, url, loading: false, error: "", failure: null });
      })
      .catch((error: unknown) => {
        if (disposed || (error instanceof DOMException && error.name === "AbortError")) return;
        setState({
          source,
          url: "",
          loading: false,
          error: errorMessage(error),
          failure: error instanceof MediaAssetError ? error.failure : "unavailable",
        });
      });

    return () => {
      disposed = true;
      controller.abort();
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [delivery, source]);

  return state.source === source
    ? state
    : { source, url: "", loading: true, error: "", failure: null };
}
