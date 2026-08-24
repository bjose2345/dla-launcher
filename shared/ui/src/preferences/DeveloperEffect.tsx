import { useEffect, useRef } from "react";

import { usePresentation } from "./PresentationProvider";
import type { Developer } from "./systemReport";

type EffectKind = Developer["effect"];
type EffectSide = "left" | "right";
type Channels = readonly [number, number, number];

interface Palette {
  core: Channels;
  mid: Channels;
  spark: Channels;
}

const palettes: Record<EffectKind, { dark: Palette; light: Palette }> = {
  aura: {
    dark: { core: [255, 209, 102], mid: [255, 93, 143], spark: [255, 244, 214] },
    light: { core: [194, 91, 6], mid: [190, 24, 93], spark: [124, 45, 18] },
  },
  arcane: {
    dark: { core: [167, 139, 250], mid: [34, 211, 238], spark: [226, 214, 255] },
    light: { core: [91, 33, 182], mid: [14, 116, 144], spark: [76, 29, 149] },
  },
};

const lightThemes = new Set(["paper-pastel", "lumen-accessible"]);
const moteCount = 54;
const sparkCount = 30;
const intensity = 1.75;

interface Mote {
  x: number;
  y: number;
  speed: number;
  size: number;
  phase: number;
  drift: number;
}

interface Spark {
  radius: number;
  angle: number;
  speed: number;
  size: number;
  phase: number;
}

const painters = new Set<(time: number) => void>();
let frameHandle = 0;

function schedule(paint: (time: number) => void): () => void {
  painters.add(paint);
  if (!frameHandle) frameHandle = requestAnimationFrame(tick);
  return () => {
    painters.delete(paint);
    if (painters.size === 0 && frameHandle) {
      cancelAnimationFrame(frameHandle);
      frameHandle = 0;
    }
  };
}

function tick(time: number): void {
  painters.forEach((paint) => paint(time));
  frameHandle = painters.size ? requestAnimationFrame(tick) : 0;
}

export function DeveloperEffect({ effect, side }: { effect: EffectKind; side: EffectSide }) {
  const { theme } = usePresentation();
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;

    const palette = palettes[effect][lightThemes.has(theme) ? "light" : "dark"];
    const dim = lightThemes.has(theme) ? (effect === "aura" ? 0.62 : 0.66) : 1;
    let seed = effect === "aura" ? 11 : 29;
    let width = 0;
    let height = 0;
    let visible = true;

    const random = () => {
      seed = (seed * 1664525 + 1013904223) % 4294967296;
      return seed / 4294967296;
    };

    const motes: Mote[] = Array.from({ length: moteCount }, () => ({
      x: random(),
      y: random(),
      speed: 0.25 + random() * 0.75,
      size: 0.6 + random() * 1.9,
      phase: random() * Math.PI * 2,
      drift: (random() - 0.5) * 0.35,
    }));
    const sparks: Spark[] = Array.from({ length: sparkCount }, () => ({
      radius: 0.28 + random() * 0.62,
      angle: random() * Math.PI * 2,
      speed: (0.18 + random() * 0.5) * (random() > 0.5 ? 1 : -1),
      size: 0.5 + random() * 1.5,
      phase: random() * Math.PI * 2,
    }));

    const tint = (key: keyof Palette, alpha: number) => {
      const [r, g, b] = palette[key];
      return `rgba(${r},${g},${b},${Math.max(0, Math.min(1, alpha * intensity * dim))})`;
    };

    const anchor = () => {
      const portrait = canvas.parentElement?.querySelector(".settings-portrait");
      if (portrait) {
        const portraitBox = portrait.getBoundingClientRect();
        const canvasBox = canvas.getBoundingClientRect();
        if (canvasBox.width) return portraitBox.left - canvasBox.left + portraitBox.width / 2;
      }
      return side === "right" ? width * 0.86 : width * 0.14;
    };

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      if (!rect.width || !rect.height) return;
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      width = rect.width;
      height = rect.height;
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
    };

    const mask = () => {
      const gradient = context.createLinearGradient(0, 0, width, 0);
      const stops: Array<[number, number]> = side === "right"
        ? [[0, 0], [0.44, 0], [0.56, 0.4], [0.74, 0.9], [1, 1]]
        : [[0, 1], [0.16, 1], [0.38, 0.5], [0.6, 0], [1, 0]];
      stops.forEach(([offset, alpha]) => gradient.addColorStop(offset, `rgba(0,0,0,${alpha})`));
      context.globalCompositeOperation = "destination-in";
      context.fillStyle = gradient;
      context.fillRect(0, 0, width, height);
      context.globalCompositeOperation = "source-over";
    };

    const drawAura = (time: number) => {
      const focusX = anchor();
      const glow = context.createRadialGradient(focusX, height * 0.62, 0, focusX, height * 0.62, height * 0.95);
      const pulse = 0.5 + 0.5 * Math.sin(time * 0.0012);
      glow.addColorStop(0, tint("core", 0.34 + pulse * 0.22));
      glow.addColorStop(0.5, tint("mid", 0.16));
      glow.addColorStop(1, "rgba(0,0,0,0)");
      context.fillStyle = glow;
      context.fillRect(0, 0, width, height);

      const surge = Math.pow(0.5 + 0.5 * Math.sin(time * 0.00045), 6);
      motes.forEach((mote, index) => {
        const life = ((time * 0.00006 * mote.speed) + mote.y) % 1;
        const eased = life * life;
        const y = height * (1.05 - eased * 1.15);
        const sway = Math.sin(time * 0.0011 + mote.phase) * 10 * mote.drift;
        const x = focusX + (mote.x - 0.5) * width * 0.56 + sway;
        const fade = Math.sin(Math.min(1, life) * Math.PI);
        const alpha = fade * (0.5 + surge * 0.55);
        if (alpha <= 0.004) return;
        context.fillStyle = tint(index % 5 === 0 ? "spark" : "core", Math.min(0.95, alpha));
        context.beginPath();
        context.ellipse(x, y, mote.size, mote.size * (1 + eased * 7 * mote.speed), 0, 0, Math.PI * 2);
        context.fill();
      });

      if (surge > 0.05) {
        context.strokeStyle = tint("mid", surge * 0.45);
        context.lineWidth = 1.4;
        context.beginPath();
        const waveY = height * (1 - surge * 1.1);
        for (let x = 0; x <= width; x += 6) {
          const offset = Math.sin(x * 0.035 + time * 0.004) * 5;
          if (x === 0) context.moveTo(x, waveY + offset);
          else context.lineTo(x, waveY + offset);
        }
        context.stroke();
      }
    };

    const drawRuneRing = (
      cx: number,
      cy: number,
      radius: number,
      ticks: number,
      rotation: number,
      alpha: number,
      key: keyof Palette,
      time: number,
    ) => {
      context.save();
      context.translate(cx, cy);
      context.rotate(rotation);
      context.strokeStyle = tint(key, alpha * 0.65);
      context.lineWidth = 1;
      context.beginPath();
      context.arc(0, 0, radius, 0, Math.PI * 2);
      context.stroke();
      for (let i = 0; i < ticks; i += 1) {
        const angle = (i / ticks) * Math.PI * 2;
        const long = i % 3 === 0;
        const inner = radius - (long ? 9 : 4);
        const flicker = long ? 0.45 + 0.55 * Math.abs(Math.sin(time * 0.001 + i)) : 0.5;
        context.strokeStyle = tint(key, alpha * flicker);
        context.lineWidth = long ? 1.6 : 1;
        context.beginPath();
        context.moveTo(Math.cos(angle) * inner, Math.sin(angle) * inner);
        context.lineTo(Math.cos(angle) * radius, Math.sin(angle) * radius);
        context.stroke();
      }
      context.restore();
    };

    const drawArcane = (time: number) => {
      const focusX = anchor();
      const focusY = height * 0.5;
      const scale = height * 0.62;
      const spin = time * 0.00016;

      const halo = context.createRadialGradient(focusX, focusY, 0, focusX, focusY, scale * 2.8);
      halo.addColorStop(0, tint("core", 0.34));
      halo.addColorStop(0.45, tint("mid", 0.12));
      halo.addColorStop(1, "rgba(0,0,0,0)");
      context.fillStyle = halo;
      context.fillRect(0, 0, width, height);

      drawRuneRing(focusX, focusY, scale, 36, spin, 0.62, "core", time);
      drawRuneRing(focusX, focusY, scale * 0.72, 24, -spin * 1.45, 0.5, "mid", time);

      context.save();
      context.translate(focusX, focusY);
      context.rotate(spin * 0.6);
      context.strokeStyle = tint("core", 0.42);
      context.lineWidth = 1;
      context.beginPath();
      for (let point = 0; point <= 5; point += 1) {
        const angle = (point * 4 * Math.PI) / 5 - Math.PI / 2;
        const x = Math.cos(angle) * scale * 0.56;
        const y = Math.sin(angle) * scale * 0.56;
        if (point === 0) context.moveTo(x, y);
        else context.lineTo(x, y);
      }
      context.stroke();
      context.restore();

      sparks.forEach((spark, index) => {
        const angle = spark.angle + time * 0.0002 * spark.speed;
        const radius = scale * spark.radius + Math.sin(time * 0.0016 + spark.phase) * scale * 0.05;
        const twinkle = 0.35 + 0.65 * Math.abs(Math.sin(time * 0.0021 + spark.phase));
        context.fillStyle = tint(index % 4 === 0 ? "spark" : "mid", Math.min(0.95, twinkle * 0.9));
        context.beginPath();
        context.arc(focusX + Math.cos(angle) * radius, focusY + Math.sin(angle) * radius, spark.size, 0, Math.PI * 2);
        context.fill();
      });

      // the pulse stops just short of the card's midpoint, whatever the card is wide
      const reach = Math.max(scale, Math.abs(focusX - width * 0.5) * 0.92);
      [0, 0.5].forEach((offset, index) => {
        const progress = ((time / 5200) + offset) % 1;
        if (progress >= 0.86) return;
        const eased = progress / 0.86;
        const falloff = Math.pow(1 - eased, 0.85) * (1 - Math.pow(eased, 8));
        context.strokeStyle = tint(index === 0 ? "core" : "mid", falloff * 0.75);
        context.lineWidth = 2.4 * falloff + 0.5;
        context.beginPath();
        context.arc(focusX, focusY, scale * 0.4 + eased * reach, 0, Math.PI * 2);
        context.stroke();
      });
    };

    const paint = (time: number) => {
      if (!width || !height || !visible) return;
      context.clearRect(0, 0, width, height);
      context.globalCompositeOperation = lightThemes.has(theme) ? "source-over" : "lighter";
      if (effect === "aura") drawAura(time);
      else drawArcane(time);
      context.globalCompositeOperation = "source-over";
      mask();
    };

    resize();

    const resizeObserver = new ResizeObserver(() => {
      resize();
      paint(performance.now());
    });
    resizeObserver.observe(canvas);

    const intersection = new IntersectionObserver(
      (entries) => entries.forEach((entry) => { visible = entry.isIntersecting; }),
      { rootMargin: "120px" },
    );
    intersection.observe(canvas);

    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      paint(1200);
      return () => {
        resizeObserver.disconnect();
        intersection.disconnect();
      };
    }

    let release = schedule(paint);
    const onVisibility = () => {
      release();
      release = document.hidden ? () => undefined : schedule(paint);
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      release();
      document.removeEventListener("visibilitychange", onVisibility);
      resizeObserver.disconnect();
      intersection.disconnect();
    };
  }, [effect, side, theme]);

  return <canvas className="settings-developer-canvas" ref={canvasRef} aria-hidden="true" />;
}
