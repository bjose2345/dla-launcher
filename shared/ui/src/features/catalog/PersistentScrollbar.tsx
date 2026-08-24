import {
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type RefObject,
  type WheelEvent,
} from "react";

import {
  calculateScrollbarGeometry,
  scrollTopForThumbOffset,
  type ScrollMetrics,
} from "./scrollbarMath";

interface PersistentScrollbarProps {
  viewport: RefObject<HTMLDivElement | null>;
  controls: string;
  label: string;
}

interface DragState {
  pointerId: number;
  startY: number;
  startScrollTop: number;
}

const emptyMetrics: Readonly<ScrollMetrics> = {
  scrollTop: 0,
  scrollHeight: 0,
  clientHeight: 0,
};

export function PersistentScrollbar({ viewport, controls, label }: PersistentScrollbarProps) {
  const track = useRef<HTMLDivElement>(null);
  const drag = useRef<DragState | null>(null);
  const [metrics, setMetrics] = useState(emptyMetrics);
  const [trackHeight, setTrackHeight] = useState(0);
  const [dragging, setDragging] = useState(false);
  const geometry = calculateScrollbarGeometry(metrics, trackHeight);

  useLayoutEffect(() => {
    const element = viewport.current;
    const trackElement = track.current;
    if (!element || !trackElement) return;

    let frame = 0;
    const update = () => {
      frame = 0;
      setMetrics({
        scrollTop: element.scrollTop,
        scrollHeight: element.scrollHeight,
        clientHeight: element.clientHeight,
      });
      setTrackHeight(trackElement.clientHeight);
    };
    const scheduleUpdate = () => {
      if (frame === 0) frame = window.requestAnimationFrame(update);
    };
    const observer = new ResizeObserver(scheduleUpdate);

    element.addEventListener("scroll", scheduleUpdate, { passive: true });
    observer.observe(element);
    observer.observe(trackElement);
    const content = element.firstElementChild;
    if (content instanceof HTMLElement) observer.observe(content);
    scheduleUpdate();

    return () => {
      element.removeEventListener("scroll", scheduleUpdate);
      observer.disconnect();
      if (frame !== 0) window.cancelAnimationFrame(frame);
    };
  }, [viewport]);

  const setScrollTop = (scrollTop: number) => {
    const element = viewport.current;
    if (element) element.scrollTop = scrollTop;
  };

  const handleTrackPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget || geometry.maxScroll === 0) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const thumbOffset = event.clientY - bounds.top - geometry.thumbHeight / 2;
    setScrollTop(scrollTopForThumbOffset(thumbOffset, geometry));
  };

  const handleThumbPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (geometry.maxScroll === 0) return;
    event.stopPropagation();
    drag.current = {
      pointerId: event.pointerId,
      startY: event.clientY,
      startScrollTop: metrics.scrollTop,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragging(true);
  };

  const handleThumbPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    if (!current || current.pointerId !== event.pointerId || geometry.maxThumbOffset === 0) return;
    const scrollDelta = (event.clientY - current.startY) * geometry.maxScroll / geometry.maxThumbOffset;
    setScrollTop(current.startScrollTop + scrollDelta);
  };

  const finishDragging = (event: PointerEvent<HTMLDivElement>) => {
    if (drag.current?.pointerId !== event.pointerId) return;
    drag.current = null;
    setDragging(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const page = Math.max(80, metrics.clientHeight * 0.85);
    const targets: Partial<Record<string, number>> = {
      ArrowUp: metrics.scrollTop - 48,
      ArrowDown: metrics.scrollTop + 48,
      PageUp: metrics.scrollTop - page,
      PageDown: metrics.scrollTop + page,
      Home: 0,
      End: geometry.maxScroll,
    };
    const target = targets[event.key];
    if (target === undefined) return;
    event.preventDefault();
    setScrollTop(target);
  };

  const handleWheel = (event: WheelEvent<HTMLDivElement>) => {
    if (geometry.maxScroll === 0) return;
    setScrollTop(metrics.scrollTop + event.deltaY);
  };

  return (
    <div
      aria-controls={controls}
      aria-label={label}
      aria-orientation="vertical"
      aria-valuemax={Math.round(geometry.maxScroll)}
      aria-valuemin={0}
      aria-valuenow={Math.round(metrics.scrollTop)}
      className={`persistent-scrollbar ${dragging ? "dragging" : ""}`}
      onKeyDown={handleKeyDown}
      onPointerDown={handleTrackPointerDown}
      onWheel={handleWheel}
      ref={track}
      role="scrollbar"
      tabIndex={geometry.maxScroll > 0 ? 0 : -1}
    >
      <div
        aria-hidden="true"
        className="persistent-scrollbar-thumb"
        onLostPointerCapture={finishDragging}
        onPointerCancel={finishDragging}
        onPointerDown={handleThumbPointerDown}
        onPointerMove={handleThumbPointerMove}
        onPointerUp={finishDragging}
        style={{
          height: geometry.thumbHeight,
          transform: `translateY(${geometry.thumbOffset}px)`,
        }}
      />
    </div>
  );
}
