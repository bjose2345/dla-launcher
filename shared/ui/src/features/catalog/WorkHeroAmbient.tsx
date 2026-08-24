import { useEffect, useRef, type RefObject } from "react";

export function useHeroBackdropParallax(): RefObject<HTMLElement | null> {
  const heroRef = useRef<HTMLElement>(null);

  useEffect(() => {
    const hero = heroRef.current;
    if (!hero || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;

    let frameId = 0;
    const move = (event: MouseEvent) => {
      if (frameId) return;
      frameId = window.requestAnimationFrame(() => {
        frameId = 0;
        const bounds = hero.getBoundingClientRect();
        const horizontal = (event.clientX - bounds.left) / bounds.width - 0.5;
        const vertical = (event.clientY - bounds.top) / bounds.height - 0.5;
        hero.style.setProperty("--work-hero-parallax-x", `${(horizontal * 28).toFixed(1)}px`);
        hero.style.setProperty("--work-hero-parallax-y", `${(vertical * 18).toFixed(1)}px`);
      });
    };

    hero.addEventListener("mousemove", move);
    return () => {
      hero.removeEventListener("mousemove", move);
      if (frameId) window.cancelAnimationFrame(frameId);
    };
  }, []);

  return heroRef;
}

export function WorkHeroAmbient({ theme }: { theme: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;

    let width = 0;
    let height = 0;
    let frameId = 0;
    let glintAt = 2200;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const primary = getComputedStyle(document.documentElement).getPropertyValue("--color-primary").trim() || "#f43f5e";

    const resize = () => {
      const bounds = canvas.getBoundingClientRect();
      const density = Math.min(window.devicePixelRatio || 1, 2);
      width = bounds.width;
      height = bounds.height;
      canvas.width = Math.max(1, Math.round(width * density));
      canvas.height = Math.max(1, Math.round(height * density));
      context.setTransform(density, 0, 0, density, 0, 0);
    };

    const draw = (time: number) => {
      context.clearRect(0, 0, width, height);
      context.save();
      context.strokeStyle = primary;
      context.globalAlpha = 0.05;
      context.lineWidth = 1;
      const offset = (time / 90) % 26;
      context.beginPath();
      for (let x = width * 0.5 - offset; x < width + height; x += 26) {
        context.moveTo(x, 0);
        context.lineTo(x - height * 0.6, height);
      }
      context.stroke();
      context.restore();

      const progress = (time - glintAt) / 900;
      if (progress >= 0 && progress <= 1) {
        const x = width * 0.45 + progress * width * 0.6;
        const gradient = context.createLinearGradient(x - 60, 0, x + 60, 0);
        gradient.addColorStop(0, "rgba(255,255,255,0)");
        gradient.addColorStop(0.5, "rgba(255,255,255,0.06)");
        gradient.addColorStop(1, "rgba(255,255,255,0)");
        context.fillStyle = gradient;
        context.save();
        context.transform(1, 0, -0.35, 1, 0, 0);
        context.fillRect(x - 60, 0, 120, height);
        context.restore();
      } else if (progress > 1) {
        glintAt = time + 4200 + Math.random() * 3200;
      }
    };

    const frame = (time: number) => {
      draw(time);
      frameId = window.requestAnimationFrame(frame);
    };
    const start = () => {
      if (!frameId) frameId = window.requestAnimationFrame(frame);
    };
    const stop = () => {
      if (frameId) window.cancelAnimationFrame(frameId);
      frameId = 0;
    };
    const visibility = () => {
      if (document.hidden) stop();
      else start();
    };

    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    if (reduced) draw(0);
    else {
      document.addEventListener("visibilitychange", visibility);
      start();
    }

    return () => {
      observer.disconnect();
      document.removeEventListener("visibilitychange", visibility);
      stop();
    };
  }, [theme]);

  return <canvas className="work-hero-ambient" ref={canvasRef} aria-hidden="true" />;
}
