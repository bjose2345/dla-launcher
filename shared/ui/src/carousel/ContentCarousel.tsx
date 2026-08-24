import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { motion, useReducedMotion } from "motion/react";

import { usePresentation } from "../preferences/PresentationProvider";

const EDGE_FADE = "36px";
const FLING_FRICTION = 0.94;
const FLING_BOOST = 1.8;
const FLING_MIN_START = 4;
const FLING_STOP = 1.5;
const DOT_SPRING = { type: "spring", stiffness: 320, damping: 32 } as const;

interface RailGeometry {
  maxScroll: number;
  pageTargets: number[];
}

function cardStep(track: HTMLDivElement): number {
  const first = track.children[0] as HTMLElement | undefined;
  const second = track.children[1] as HTMLElement | undefined;
  return first && second ? second.offsetLeft - first.offsetLeft : track.clientWidth;
}

export function railPageTargets(step: number, clientWidth: number, maxScroll: number): number[] {
  if (!step || maxScroll <= 5) return [];
  const pageStep = step * Math.max(1, Math.floor(clientWidth / step));
  const targets = [0];
  for (let offset = pageStep; offset < maxScroll - 4; offset += pageStep) {
    targets.push(Math.round(offset));
  }
  targets.push(Math.round(maxScroll));
  return targets;
}

export function nearestTargetIndex(targets: number[], value: number): number {
  let best = 0;
  let bestDistance = Number.POSITIVE_INFINITY;
  targets.forEach((target, index) => {
    const distance = Math.abs(target - value);
    if (distance < bestDistance) {
      best = index;
      bestDistance = distance;
    }
  });
  return best;
}

export function railEdgeMask(canScrollBack: boolean, canScrollForward: boolean): string | undefined {
  if (canScrollBack && canScrollForward) {
    return `linear-gradient(to right, transparent, #000 ${EDGE_FADE}, #000 calc(100% - ${EDGE_FADE}), transparent)`;
  }
  if (canScrollForward) return `linear-gradient(to right, #000 calc(100% - ${EDGE_FADE}), transparent)`;
  if (canScrollBack) return `linear-gradient(to right, transparent, #000 ${EDGE_FADE})`;
  return undefined;
}

export function ContentCarousel({ label, children }: { label: string; children: ReactNode }) {
  const { t } = usePresentation();
  const trackRef = useRef<HTMLDivElement>(null);
  const geometryRef = useRef<RailGeometry | null>(null);
  const glideRef = useRef(0);
  const draggedRef = useRef(false);
  const [pageTargets, setPageTargets] = useState<number[]>([]);
  const [activePage, setActivePage] = useState(0);
  const [canScrollBack, setCanScrollBack] = useState(false);
  const [canScrollForward, setCanScrollForward] = useState(false);
  const dotsId = useId();
  const reduceMotion = useReducedMotion();

  const syncFromScroll = useCallback(() => {
    const track = trackRef.current;
    const geometry = geometryRef.current;
    if (!track || !geometry) return;
    const scrollLeft = track.scrollLeft;
    setCanScrollBack(scrollLeft > 5);
    setCanScrollForward(scrollLeft < geometry.maxScroll - 5);
    setActivePage(geometry.pageTargets.length ? nearestTargetIndex(geometry.pageTargets, scrollLeft) : 0);
  }, []);

  const measure = useCallback(() => {
    const track = trackRef.current;
    if (!track || !track.firstElementChild) {
      geometryRef.current = null;
      setPageTargets((current) => (current.length ? [] : current));
      setCanScrollBack(false);
      setCanScrollForward(false);
      return;
    }
    const maxScroll = track.scrollWidth - track.clientWidth;
    const targets = railPageTargets(cardStep(track), track.clientWidth, maxScroll);
    geometryRef.current = { maxScroll, pageTargets: targets };
    setPageTargets((current) => (
      current.length === targets.length && current[current.length - 1] === targets[targets.length - 1]
        ? current
        : targets
    ));
    syncFromScroll();
  }, [syncFromScroll]);

  const stopGlide = useCallback(() => {
    if (glideRef.current) {
      cancelAnimationFrame(glideRef.current);
      glideRef.current = 0;
    }
  }, []);

  const goToPage = (index: number) => {
    const track = trackRef.current;
    const targets = geometryRef.current?.pageTargets ?? [];
    const target = targets[Math.min(Math.max(index, 0), targets.length - 1)];
    if (!track || target === undefined) return;
    stopGlide();
    track.scrollTo({ left: target, behavior: "smooth" });
  };

  const page = (direction: -1 | 1) => {
    const track = trackRef.current;
    const targets = geometryRef.current?.pageTargets ?? [];
    if (!track || targets.length < 2) return;
    const current = nearestTargetIndex(targets, track.scrollLeft);
    const target = targets[(current + (direction === 1 ? 1 : targets.length - 1)) % targets.length];
    if (target === undefined) return;
    stopGlide();
    track.scrollTo({ left: target, behavior: "smooth" });
  };

  useEffect(() => {
    const track = trackRef.current;
    if (!track) return;
    track.addEventListener("scroll", syncFromScroll, { passive: true });
    const observer = new ResizeObserver(() => measure());
    observer.observe(track);
    return () => {
      track.removeEventListener("scroll", syncFromScroll);
      observer.disconnect();
    };
  }, [syncFromScroll, measure]);

  useEffect(() => {
    measure();
  }, [children, measure]);

  useEffect(() => {
    if (!window.matchMedia("(hover: hover) and (pointer: fine)").matches) return;
    const track = trackRef.current;
    if (!track) return;

    let down = false;
    let moved = false;
    let startX = 0;
    let startScroll = 0;
    let velocity = 0;
    let lastX = 0;
    let lastMoveAt = 0;

    const settle = () => {
      const step = cardStep(track);
      if (!step) return;
      const maxScroll = track.scrollWidth - track.clientWidth;
      let target = Math.round(track.scrollLeft / step) * step;
      target = Math.max(0, Math.min(target, maxScroll));
      if (maxScroll - target < step) target = maxScroll;
      track.scrollTo({ left: target, behavior: "smooth" });
    };

    const glide = (initial: number) => {
      let speed = initial * FLING_BOOST;
      const maxScroll = track.scrollWidth - track.clientWidth;
      let last = performance.now();
      const tick = (now: number) => {
        glideRef.current = 0;
        const frames = Math.min((now - last) / (1000 / 60), 3);
        last = now;
        speed *= Math.pow(FLING_FRICTION, frames);
        const next = track.scrollLeft + speed * frames;
        if (next <= 0 || next >= maxScroll) {
          track.scrollLeft = Math.max(0, Math.min(next, maxScroll));
          settle();
          return;
        }
        track.scrollLeft = next;
        if (Math.abs(speed) < FLING_STOP) {
          settle();
          return;
        }
        glideRef.current = requestAnimationFrame(tick);
      };
      glideRef.current = requestAnimationFrame(tick);
    };

    const onDown = (event: PointerEvent) => {
      if (event.pointerType !== "mouse") return;
      stopGlide();
      down = true;
      moved = false;
      startX = event.clientX;
      startScroll = track.scrollLeft;
      lastX = event.clientX;
      lastMoveAt = performance.now();
      velocity = 0;
    };
    const onMove = (event: PointerEvent) => {
      if (!down) return;
      const delta = event.clientX - startX;
      if (!moved && Math.abs(delta) > 3) {
        moved = true;
        track.dataset.dragging = "true";
      }
      track.scrollLeft = startScroll - delta;
      const now = performance.now();
      const elapsed = now - lastMoveAt;
      if (elapsed > 0) {
        velocity = velocity * 0.7 + ((lastX - event.clientX) / elapsed) * (1000 / 60) * 0.3;
        lastX = event.clientX;
        lastMoveAt = now;
      }
    };
    const onUp = () => {
      if (!down) return;
      down = false;
      delete track.dataset.dragging;
      draggedRef.current = moved;
      if (!moved) return;
      if (performance.now() - lastMoveAt > 80) velocity = 0;
      const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      if (!reduced && Math.abs(velocity) > FLING_MIN_START) glide(velocity);
      else settle();
    };
    const onDragStart = (event: Event) => event.preventDefault();

    track.addEventListener("pointerdown", onDown, { capture: true, passive: true });
    track.addEventListener("dragstart", onDragStart);
    track.addEventListener("wheel", stopGlide, { passive: true });
    window.addEventListener("pointermove", onMove, { capture: true, passive: true });
    window.addEventListener("pointerup", onUp, { capture: true, passive: true });
    return () => {
      stopGlide();
      delete track.dataset.dragging;
      track.removeEventListener("pointerdown", onDown, true);
      track.removeEventListener("dragstart", onDragStart);
      track.removeEventListener("wheel", stopGlide);
      window.removeEventListener("pointermove", onMove, true);
      window.removeEventListener("pointerup", onUp, true);
    };
  }, [stopGlide]);

  const suppressDragClick = (event: React.MouseEvent) => {
    if (!draggedRef.current) return;
    event.preventDefault();
    event.stopPropagation();
    draggedRef.current = false;
  };

  const mask = railEdgeMask(canScrollBack, canScrollForward);
  const paged = pageTargets.length > 1;

  return (
    <div className="content-carousel">
      {paged ? (
        <>
          <button
            className="content-carousel-nav content-carousel-nav-back"
            type="button"
            aria-label={t("carousel.previous")}
            onClick={() => page(-1)}
          >
            <ChevronLeft aria-hidden="true" />
          </button>
          <button
            className="content-carousel-nav content-carousel-nav-forward"
            type="button"
            aria-label={t("carousel.next")}
            onClick={() => page(1)}
          >
            <ChevronRight aria-hidden="true" />
          </button>
        </>
      ) : null}

      <div
        className="content-carousel-track"
        ref={trackRef}
        role="group"
        aria-label={label}
        style={mask ? { maskImage: mask, WebkitMaskImage: mask } : undefined}
        onClickCapture={suppressDragClick}
      >
        {children}
      </div>

      {paged ? (
        <div className="content-carousel-dots">
          {pageTargets.map((target, index) => (
            <button
              className="content-carousel-dot"
              type="button"
              key={target}
              aria-label={t("carousel.page", { number: index + 1 })}
              aria-current={index === activePage ? "true" : undefined}
              onClick={() => goToPage(index)}
            >
              <i />
              {index === activePage ? (
                <motion.span
                  className="content-carousel-dot-active"
                  layoutId={`content-carousel-dot-${dotsId}`}
                  transition={reduceMotion ? { duration: 0 } : DOT_SPRING}
                />
              ) : null}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
