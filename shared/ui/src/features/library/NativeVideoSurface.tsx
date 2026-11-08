import { videoCapabilities } from "../../app/videoCapabilities";
import { useEffect, useRef } from "react";

import type {
  LibraryGateway,
  NativeVideoRectangle,
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
  | "readVideoSubtitles"
  | "videoSubtitleUrl"
  | "openNativeVideo"
  | "updateNativeVideoViewport"
  | "controlNativeVideo"
  | "closeNativeVideo"
  | "subscribeNativeVideoState"
  | "nativeVideoChrome"
  | "openVideoExternally"
>;

interface NativeVideoSurfaceProps {
  gateway: NativeVideoGateway;
  request: Omit<OpenNativeVideoRequest, "viewport">;
  onState: (state: NativeVideoState) => void;
  onOpen: (response: OpenNativeVideoResponse) => void;
  onError: (error: unknown) => void;
  /** The page's controls lying over the frame right now, which it must show through. */
  obstructions?: () => NativeVideoRectangle[];
  /** Changes when controls appear over or leave the frame, for prompt updates. */
  overlays?: string;
}

/** How long the frame follows controls sliding over it, every animation frame. */
const OVERLAY_FOLLOW_MS = 400;

export function NativeVideoSurface({
  gateway,
  request,
  onState,
  onOpen,
  onError,
  obstructions,
  overlays = "",
}: NativeVideoSurfaceProps) {
  const surfaceRef = useRef<HTMLDivElement>(null);
  const requestRef = useRef(request);
  const onStateRef = useRef(onState);
  const onOpenRef = useRef(onOpen);
  const onErrorRef = useRef(onError);
  const obstructionsRef = useRef(obstructions);
  const updateRef = useRef<() => void>(() => undefined);
  const surfaceIdRef = useRef("");
  onStateRef.current = onState;
  onOpenRef.current = onOpen;
  onErrorRef.current = onError;
  obstructionsRef.current = obstructions;

  useEffect(() => {
    const started = performance.now();
    let frame = 0;
    const follow = () => {
      updateRef.current();
      if (performance.now() - started < OVERLAY_FOLLOW_MS) frame = requestAnimationFrame(follow);
    };
    frame = requestAnimationFrame(follow);
    return () => cancelAnimationFrame(frame);
  }, [overlays]);

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
      const obstructions = obstructionsRef.current?.() ?? [];
      void gateway.updateNativeVideoViewport(requestRef.current.sessionId, surfaceId, viewport, obstructions)
        .catch(onErrorRef.current);
    };
    const scheduleUpdate = () => {
      if (resizeFrame) cancelAnimationFrame(resizeFrame);
      resizeFrame = requestAnimationFrame(update);
    };
    updateRef.current = update;
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
    // Menus and dialogs portal to the body; what lies over the frame can
    // change with them.
    const portals = typeof MutationObserver === "undefined"
      ? null
      : new MutationObserver(scheduleUpdate);
    portals?.observe(document.body, { childList: true });
    window.addEventListener("resize", scheduleUpdate);
    animationFrame = requestAnimationFrame(() => void open());
    return () => {
      disposed = true;
      cancelAnimationFrame(animationFrame);
      cancelAnimationFrame(resizeFrame);
      window.clearInterval(positionTimer);
      updateRef.current = () => undefined;
      observer?.disconnect();
      portals?.disconnect();
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
    !videoCapabilities().nativeVideo
    || !gateway.openNativeVideo
    || !gateway.updateNativeVideoViewport
    || !gateway.controlNativeVideo
    || !gateway.closeNativeVideo
    || !gateway.subscribeNativeVideoState
  ) return null;
  return gateway as NativeVideoGateway;
}
