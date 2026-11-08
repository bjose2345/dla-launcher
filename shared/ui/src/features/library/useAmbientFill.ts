import { type RefObject, useEffect } from "react";

const PLAYING_INTERVAL_MS = 250;
const PAUSED_INTERVAL_MS = 1_500;
/** Small enough to cost nothing; the blur hides the pixels. */
const SAMPLE_WIDTH = 32;
const SAMPLE_HEIGHT = 18;

/**
 * Paints a blurred copy of the current frame behind a letterboxed picture, so
 * the bands around it carry the picture's colour instead of black.
 */
export function useAmbientFill(
  video: RefObject<HTMLVideoElement | null>,
  canvas: RefObject<HTMLCanvasElement | null>,
  picture: string,
): void {
  useEffect(() => {
    const element = video.current;
    const target = canvas.current;
    if (!element || !target || !picture) return;
    const context = target.getContext("2d", { alpha: false });
    if (!context) return;
    target.width = SAMPLE_WIDTH;
    target.height = SAMPLE_HEIGHT;
    let timer = 0;
    const paint = () => {
      if (element.readyState < 2) return;
      try {
        context.drawImage(element, 0, 0, SAMPLE_WIDTH, SAMPLE_HEIGHT);
        target.classList.add("is-live");
      } catch {
        // A frame the webview cannot hand over keeps the previous one.
      }
    };
    const tick = () => {
      paint();
      timer = window.setTimeout(tick, element.paused ? PAUSED_INTERVAL_MS : PLAYING_INTERVAL_MS);
    };
    tick();
    element.addEventListener("loadeddata", paint);
    element.addEventListener("seeked", paint);
    return () => {
      window.clearTimeout(timer);
      element.removeEventListener("loadeddata", paint);
      element.removeEventListener("seeked", paint);
      target.classList.remove("is-live");
    };
  }, [canvas, picture, video]);
}
