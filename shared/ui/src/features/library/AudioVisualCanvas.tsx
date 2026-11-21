import { useEffect, useRef, useState } from "react";

import { useResizeObserver } from "../../app/useResizeObserver";
import { usePresentation } from "../../preferences/PresentationProvider";
import {
  readingLevel,
  visualPaint,
  type Ink,
  type VisualColor,
  type VisualPaint,
  type VisualStyle,
} from "./audioVisual";
import { useMediaPlayback } from "./MediaPlaybackProvider";
import { envelopeLevel, spectrumBarCount, spectrumBarHeights } from "./NowPlayingBars";

const MARK_GAP_PX = 3;
const FLOOR = 0.05;
const RELEASE_PER_FRAME = 0.82;
const RING_MARKS = 72;
const SILENCE = 128;

interface Surface {
  context: CanvasRenderingContext2D;
  width: number;
  height: number;
}

/** The canvas sized to its box at the screen's pixel density, cleared, in CSS pixels. */
function surface(canvas: HTMLCanvasElement): Surface | null {
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  if (width <= 0 || height <= 0) return null;
  let context: CanvasRenderingContext2D | null = null;
  try {
    context = canvas.getContext("2d");
  } catch {
    return null;
  }
  if (!context) return null;
  const ratio = window.devicePixelRatio || 1;
  const pixelWidth = Math.round(width * ratio);
  const pixelHeight = Math.round(height * ratio);
  if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
    canvas.width = pixelWidth;
    canvas.height = pixelHeight;
  }
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  context.clearRect(0, 0, width, height);
  return { context, width, height };
}

function paintFor(canvas: HTMLCanvasElement, color: VisualColor, cover: string | null): VisualPaint {
  const style = getComputedStyle(canvas);
  return visualPaint(color, {
    primary: style.getPropertyValue("--color-primary").trim() || "#ff5c8a",
    content: style.getPropertyValue("--color-base-content").trim() || "#ffffff",
  }, cover);
}

function reducedMotion(): boolean {
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}

function mark(context: CanvasRenderingContext2D, ink: ReturnType<Ink>, x: number, y: number, width: number, height: number) {
  context.globalAlpha = ink.alpha;
  context.fillStyle = ink.color;
  context.fillRect(x, y, width, height);
}

/** One colour along the width for a shape drawn in one piece, so Rainbow runs through it. */
function sweep(context: CanvasRenderingContext2D, ink: Ink, width: number): CanvasGradient {
  const gradient = context.createLinearGradient(0, 0, width, 0);
  for (let stop = 0; stop <= 6; stop += 1) gradient.addColorStop(stop / 6, ink(stop, 6).color);
  return gradient;
}

function drawTrack(
  { context, width, height }: Surface,
  levels: number[],
  progress: number,
  style: VisualStyle,
  paint: VisualPaint,
) {
  const count = levels.length;
  if (count === 0) return;
  const pitch = width / count;
  const thick = Math.max(1.5, pitch - MARK_GAP_PX);
  if (style === "area") {
    const middle = height / 2;
    const shape = () => {
      context.beginPath();
      for (let index = 0; index < count; index += 1) {
        context.lineTo(index * pitch + pitch / 2, middle - (Math.max(0.04, levels[index] ?? 0) * height) / 2);
      }
      for (let index = count - 1; index >= 0; index -= 1) {
        context.lineTo(index * pitch + pitch / 2, middle + (Math.max(0.04, levels[index] ?? 0) * height) / 2);
      }
      context.closePath();
    };
    shape();
    context.globalAlpha = paint.rest(0, count).alpha;
    context.fillStyle = sweep(context, paint.rest, width);
    context.fill();
    if (progress > 0) {
      context.save();
      context.beginPath();
      context.rect(0, 0, width * progress, height);
      context.clip();
      context.clearRect(0, 0, width, height);
      shape();
      context.globalAlpha = 1;
      context.fillStyle = sweep(context, paint.played, width);
      context.fill();
      context.restore();
    }
    return;
  }
  for (let index = 0; index < count; index += 1) {
    const tall = Math.max(0.04, Math.min(1, levels[index] ?? 0)) * height;
    const ink = (index + 1) / count <= progress ? paint.played(index, count) : paint.rest(index, count);
    mark(context, ink, index * pitch, style === "mirror" ? (height - tall) / 2 : height - tall, thick, tall);
  }
}

function drawLive(
  { context, width, height }: Surface,
  levels: ArrayLike<number>,
  wave: Uint8Array | null,
  style: VisualStyle,
  paint: VisualPaint,
) {
  const count = levels.length;
  if (style === "oscilloscope") {
    const middle = height / 2;
    context.globalAlpha = 1;
    context.lineWidth = 2.5;
    context.lineJoin = "round";
    context.strokeStyle = sweep(context, paint.played, width);
    context.beginPath();
    const points = wave?.length ?? 2;
    for (let point = 0; point < points; point += 1) {
      const value = wave ? ((wave[point] ?? SILENCE) - SILENCE) / SILENCE : 0;
      context.lineTo((point / (points - 1)) * width, middle + value * height * 0.46);
    }
    context.stroke();
    return;
  }
  if (style === "ring") {
    const centerX = width / 2;
    const centerY = height / 2;
    const inner = Math.min(width, height) * 0.24;
    context.lineWidth = 3;
    context.lineCap = "round";
    for (let index = 0; index < RING_MARKS; index += 1) {
      const half = index < RING_MARKS / 2 ? index : RING_MARKS - 1 - index;
      const level = levels[Math.min(count - 1, Math.floor((half / (RING_MARKS / 2)) * count))] ?? FLOOR;
      const angle = (index / RING_MARKS) * Math.PI * 2 - Math.PI / 2;
      const outer = inner + Math.max(FLOOR, level) * inner * 1.25;
      const ink = paint.played(index, RING_MARKS);
      context.globalAlpha = ink.alpha;
      context.strokeStyle = ink.color;
      context.beginPath();
      context.moveTo(centerX + Math.cos(angle) * inner, centerY + Math.sin(angle) * inner);
      context.lineTo(centerX + Math.cos(angle) * outer, centerY + Math.sin(angle) * outer);
      context.stroke();
    }
    return;
  }
  if (count === 0) return;
  const pitch = width / count;
  const thick = Math.max(1.5, pitch - MARK_GAP_PX);
  for (let index = 0; index < count; index += 1) {
    const tall = Math.max(FLOOR, Math.min(1, levels[index] ?? 0)) * height;
    mark(
      context,
      paint.played(index, count),
      index * pitch,
      style === "mirrorSpectrum" ? (height - tall) / 2 : height - tall,
      thick,
      tall,
    );
  }
}

/**
 * The whole track as bars, mirrored bars or one smooth shape, with the part
 * already heard in the stronger colour. While the track is still being read
 * (`levels` null) a pulse travels along `count` placeholder marks.
 */
export function TrackPicture({
  levels,
  count,
  progress,
  style,
  color,
  cover,
}: {
  levels: number[] | null;
  count: number;
  progress: number;
  style: VisualStyle;
  color: VisualColor;
  cover: string | null;
}) {
  const { theme } = usePresentation();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState("");
  useResizeObserver(canvasRef, (canvas) => setSize(`${canvas.clientWidth}x${canvas.clientHeight}`));

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return undefined;
    const paint = paintFor(canvas, color, cover);
    const draw = (shown: number[], heard: number) => {
      const target = surface(canvas);
      if (target) drawTrack(target, shown, heard, style, paint);
    };
    if (levels) {
      draw(levels, progress);
      return undefined;
    }
    const placeholder = (seconds: number) =>
      Array.from({ length: count }, (_, index) => readingLevel(index, seconds));
    if (reducedMotion()) {
      draw(Array.from({ length: count }, () => 0.06), 0);
      return undefined;
    }
    let frame = requestAnimationFrame(function tick(now) {
      draw(placeholder(now / 1_000), 0);
      frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  }, [color, count, cover, levels, progress, size, style, theme]);

  return <canvas ref={canvasRef} className="audio-visual-canvas" aria-hidden="true" />;
}

/**
 * The sound as it plays: its spectrum as bars, mirrored bars or a ring, or
 * its wave as an oscilloscope line. Still while nothing plays, and for
 * reduced motion.
 */
export function LiveVisual({
  installationId,
  style,
  color,
  cover,
  active,
}: {
  installationId: string;
  style: VisualStyle;
  color: VisualColor;
  cover: string | null;
  active: boolean;
}) {
  const { theme } = usePresentation();
  const playback = useMediaPlayback();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState("");
  const playing = active
    && playback.playing
    && playback.session?.action === "play_audio"
    && playback.session.installationId === installationId;
  const analyser = playing ? playback.analyser : null;
  const enableAnalyser = playback.enableAnalyser;

  useEffect(() => {
    if (playing) enableAnalyser();
  }, [enableAnalyser, playing]);

  useResizeObserver(canvasRef, (canvas) => setSize(`${canvas.clientWidth}x${canvas.clientHeight}`));

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !active) return undefined;
    const paint = paintFor(canvas, color, cover);
    const count = spectrumBarCount(canvas.clientWidth);
    const levels = new Float32Array(count).fill(FLOOR);
    const still = () => {
      const target = surface(canvas);
      if (target) drawLive(target, levels, null, style, paint);
    };
    if (!analyser || reducedMotion()) {
      still();
      return undefined;
    }
    const frequencies = new Uint8Array(analyser.frequencyBinCount);
    const wave = new Uint8Array(analyser.fftSize);
    let frame = requestAnimationFrame(function tick() {
      const target = surface(canvas);
      if (target) {
        if (style === "oscilloscope") {
          analyser.getByteTimeDomainData(wave);
        } else {
          analyser.getByteFrequencyData(frequencies);
          const heights = spectrumBarHeights(frequencies, count);
          for (let index = 0; index < count; index += 1) {
            levels[index] = envelopeLevel(levels[index] ?? 0, heights[index] ?? 0, RELEASE_PER_FRAME);
          }
        }
        drawLive(target, levels, wave, style, paint);
      }
      frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  }, [active, analyser, color, cover, size, style, theme]);

  return <canvas ref={canvasRef} className="audio-visual-canvas" aria-hidden="true" />;
}
