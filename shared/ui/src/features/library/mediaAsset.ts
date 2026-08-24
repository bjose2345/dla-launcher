import { useEffect, useState } from "react";

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
          error: error instanceof Error ? error.message : String(error),
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

export function useMaterializedMediaSource(source: string): MaterializedMediaSource {
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
    void fetchMediaAsset(source, controller.signal)
      .then((blob) => {
        if (disposed) return;
        objectUrl = URL.createObjectURL(blob);
        setState({ source, url: objectUrl, loading: false, error: "", failure: null });
      })
      .catch((error: unknown) => {
        if (disposed || (error instanceof DOMException && error.name === "AbortError")) return;
        setState({
          source,
          url: "",
          loading: false,
          error: error instanceof Error ? error.message : String(error),
          failure: error instanceof MediaAssetError ? error.failure : "unavailable",
        });
      });

    return () => {
      disposed = true;
      controller.abort();
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [source]);

  return state.source === source
    ? state
    : { source, url: "", loading: true, error: "", failure: null };
}
