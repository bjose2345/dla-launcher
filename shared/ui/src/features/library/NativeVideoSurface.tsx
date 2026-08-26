import { useEffect, useRef } from "react";

import type {
  LibraryGateway,
  NativeVideoState,
  NativeVideoViewport,
  OpenNativeVideoRequest,
  OpenNativeVideoResponse,
} from "./types";

export type NativeVideoGateway = Required<Pick<
  LibraryGateway,
  | "openNativeVideo"
  | "updateNativeVideoViewport"
  | "controlNativeVideo"
  | "closeNativeVideo"
  | "subscribeNativeVideoState"
>>;

export type VideoPlayerGateway = Pick<
  LibraryGateway,
  | "mediaAssetUrl"
  | "openNativeVideo"
  | "updateNativeVideoViewport"
  | "controlNativeVideo"
  | "closeNativeVideo"
  | "subscribeNativeVideoState"
>;

interface NativeVideoSurfaceProps {
  gateway: NativeVideoGateway;
  request: Omit<OpenNativeVideoRequest, "viewport">;
  onState: (state: NativeVideoState) => void;
  onOpen: (response: OpenNativeVideoResponse) => void;
  onError: (error: unknown) => void;
}

export function NativeVideoSurface({
  gateway,
  request,
  onState,
  onOpen,
  onError,
}: NativeVideoSurfaceProps) {
  const surfaceRef = useRef<HTMLDivElement>(null);
  const requestRef = useRef(request);
  const onStateRef = useRef(onState);
  const onOpenRef = useRef(onOpen);
  const onErrorRef = useRef(onError);
  const surfaceIdRef = useRef("");
  onStateRef.current = onState;
  onOpenRef.current = onOpen;
  onErrorRef.current = onError;

  useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    void gateway.subscribeNativeVideoState((state) => {
      if (disposed) return;
      if (!surfaceIdRef.current || state.surfaceId !== surfaceIdRef.current) return;
      if (state.sessionId !== requestRef.current.sessionId || state.ordinal !== requestRef.current.ordinal) return;
      if (state.kind === "error") {
        const surfaceId = surfaceIdRef.current;
        surfaceIdRef.current = "";
        void gateway.closeNativeVideo(state.sessionId, surfaceId).then(() => {
          if (!disposed) onStateRef.current(state);
        }).catch((error) => {
          if (disposed) return;
          onErrorRef.current(error);
          onStateRef.current(state);
        });
        return;
      }
      onStateRef.current(state);
    }).then((release) => {
      if (disposed) release();
      else unsubscribe = release;
    }).catch(onErrorRef.current);
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [gateway]);

  useEffect(() => {
    const surface = surfaceRef.current;
    if (!surface) return;
    let disposed = false;
    let opened = false;
    let animationFrame = 0;
    let resizeFrame = 0;
    let positionTimer = 0;
    let surfaceId = "";

    const measure = (): NativeVideoViewport | null => {
      const bounds = surface.getBoundingClientRect();
      if (bounds.width < 1 || bounds.height < 1) return null;
      return {
        x: bounds.left,
        y: bounds.top,
        width: bounds.width,
        height: bounds.height,
      };
    };
    const update = () => {
      resizeFrame = 0;
      if (!opened || disposed) return;
      const viewport = measure();
      if (!viewport) return;
      void gateway.updateNativeVideoViewport(requestRef.current.sessionId, surfaceId, viewport)
        .catch(onErrorRef.current);
    };
    const scheduleUpdate = () => {
      if (resizeFrame) cancelAnimationFrame(resizeFrame);
      resizeFrame = requestAnimationFrame(update);
    };
    const open = async () => {
      if (disposed) return;
      const viewport = measure();
      if (!viewport) {
        animationFrame = requestAnimationFrame(() => void open());
        return;
      }
      try {
        const response = await gateway.openNativeVideo({ ...requestRef.current, viewport });
        surfaceId = response.surfaceId;
        if (disposed) {
          await gateway.closeNativeVideo(requestRef.current.sessionId, surfaceId).catch(() => undefined);
          return;
        }
        surfaceIdRef.current = surfaceId;
        onOpenRef.current(response);
        opened = true;
        scheduleUpdate();
        positionTimer = window.setInterval(scheduleUpdate, 500);
      } catch (error) {
        if (!disposed) onErrorRef.current(error);
      }
    };

    const observer = typeof ResizeObserver === "undefined"
      ? null
      : new ResizeObserver(scheduleUpdate);
    observer?.observe(surface);
    window.addEventListener("resize", scheduleUpdate);
    animationFrame = requestAnimationFrame(() => void open());
    return () => {
      disposed = true;
      cancelAnimationFrame(animationFrame);
      cancelAnimationFrame(resizeFrame);
      window.clearInterval(positionTimer);
      observer?.disconnect();
      window.removeEventListener("resize", scheduleUpdate);
      if (surfaceId) {
        void gateway.closeNativeVideo(requestRef.current.sessionId, surfaceId).catch(() => undefined);
      }
      if (surfaceIdRef.current === surfaceId) surfaceIdRef.current = "";
    };
  }, [gateway]);

  return <div className="video-player-native-surface" ref={surfaceRef} aria-hidden="true" />;
}

export function resolveNativeVideoGateway(gateway: VideoPlayerGateway): NativeVideoGateway | null {
  if (
    !gateway.openNativeVideo
    || !gateway.updateNativeVideoViewport
    || !gateway.controlNativeVideo
    || !gateway.closeNativeVideo
    || !gateway.subscribeNativeVideoState
  ) return null;
  return gateway as NativeVideoGateway;
}
