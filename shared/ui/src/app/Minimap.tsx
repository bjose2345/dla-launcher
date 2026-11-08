import { useRef, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";

export interface MinimapFrame {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * A small picture of the whole image with a frame over the part in view.
 * Pressing or dragging on it reports where the pointer is, as fractions of
 * the picture, for the owner to scroll or pan to. The owner draws the
 * thumbnail as children and works out the frame in minimap pixels.
 */
export function Minimap({
  className,
  size,
  label,
  frame,
  onPoint,
  children,
}: {
  className?: string;
  size: { width: number; height: number };
  label: string;
  frame: MinimapFrame | null;
  onPoint: (point: { x: number; y: number }) => void;
  children: ReactNode;
}) {
  const dragging = useRef(false);
  const point = (event: ReactPointerEvent<HTMLDivElement>) => {
    const area = event.currentTarget.getBoundingClientRect();
    onPoint({ x: (event.clientX - area.left) / area.width, y: (event.clientY - area.top) / area.height });
  };
  return (
    <div className={className ? `app-minimap ${className}` : "app-minimap"}>
      <div
        role="img"
        aria-label={label}
        style={{ width: size.width, height: size.height }}
        onPointerDown={(event) => {
          event.stopPropagation();
          dragging.current = true;
          event.currentTarget.setPointerCapture?.(event.pointerId);
          point(event);
        }}
        onPointerMove={(event) => {
          if (dragging.current) point(event);
        }}
        onPointerUp={() => {
          dragging.current = false;
        }}
        onPointerCancel={() => {
          dragging.current = false;
        }}
      >
        {children}
        {frame ? (
          <span aria-hidden="true" style={{ left: frame.left, top: frame.top, width: frame.width, height: frame.height }} />
        ) : null}
      </div>
    </div>
  );
}
