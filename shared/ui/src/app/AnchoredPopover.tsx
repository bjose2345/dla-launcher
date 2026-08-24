import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  type AriaRole,
  type MouseEventHandler,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";

export type AnchoredPopoverSide = "top" | "bottom";
export type AnchoredPopoverAlign = "start" | "center" | "end";

interface Rectangle {
  top: number;
  right: number;
  bottom: number;
  left: number;
  width: number;
  height: number;
}

interface ViewportRectangle {
  top: number;
  left: number;
  width: number;
  height: number;
}

export interface AnchoredPopoverPosition {
  top: number;
  left: number;
  maxHeight: number;
  side: AnchoredPopoverSide;
}

export function placeAnchoredPopover(
  anchor: Rectangle,
  popover: Pick<Rectangle, "width" | "height">,
  viewport: ViewportRectangle,
  {
    side = "bottom",
    align = "start",
    gap = 8,
    padding = 12,
  }: {
    side?: AnchoredPopoverSide;
    align?: AnchoredPopoverAlign;
    gap?: number;
    padding?: number;
  } = {},
): AnchoredPopoverPosition {
  const viewportLeft = viewport.left + padding;
  const viewportRight = viewport.left + viewport.width - padding;
  const viewportTop = viewport.top + padding;
  const viewportBottom = viewport.top + viewport.height - padding;
  const visibleWidth = Math.min(popover.width, Math.max(0, viewportRight - viewportLeft));
  const desiredLeft = align === "center"
    ? anchor.left + (anchor.width - visibleWidth) / 2
    : align === "end"
      ? anchor.right - visibleWidth
      : anchor.left;
  const left = clamp(desiredLeft, viewportLeft, viewportRight - visibleWidth);
  const belowTop = anchor.bottom + gap;
  const aboveBottom = anchor.top - gap;
  const spaceBelow = Math.max(0, viewportBottom - belowTop);
  const spaceAbove = Math.max(0, aboveBottom - viewportTop);
  const preferredSpace = side === "bottom" ? spaceBelow : spaceAbove;
  const alternateSpace = side === "bottom" ? spaceAbove : spaceBelow;
  const resolvedSide = popover.height > preferredSpace && alternateSpace > preferredSpace
    ? side === "bottom" ? "top" : "bottom"
    : side;
  const maxHeight = resolvedSide === "bottom" ? spaceBelow : spaceAbove;
  const visibleHeight = Math.min(popover.height, maxHeight);
  const desiredTop = resolvedSide === "bottom"
    ? belowTop
    : aboveBottom - visibleHeight;
  const top = clamp(desiredTop, viewportTop, viewportBottom - visibleHeight);

  return { top, left, maxHeight, side: resolvedSide };
}

export function AnchoredPopover({
  anchorRef,
  children,
  className,
  id,
  role,
  ariaLabel,
  onClose,
  onClick,
  side = "bottom",
  align = "start",
  gap = 8,
  viewportPadding = 12,
  maximumWidth,
  minimumWidth = 0,
  matchAnchorWidth = false,
  zIndex = 110,
}: {
  anchorRef: RefObject<HTMLElement | null>;
  children: ReactNode;
  className?: string;
  id?: string;
  role?: AriaRole;
  ariaLabel?: string;
  onClose: () => void;
  onClick?: MouseEventHandler<HTMLDivElement>;
  side?: AnchoredPopoverSide;
  align?: AnchoredPopoverAlign;
  gap?: number;
  viewportPadding?: number;
  maximumWidth?: number;
  minimumWidth?: number;
  matchAnchorWidth?: boolean;
  zIndex?: number;
}) {
  const popoverRef = useRef<HTMLDivElement>(null);

  const updatePosition = useCallback(() => {
    const anchor = anchorRef.current;
    const popover = popoverRef.current;
    if (!anchor || !popover) return;

    const viewport = readViewport();
    const availableWidth = Math.max(0, viewport.width - viewportPadding * 2);
    const anchorBounds = anchor.getBoundingClientRect();
    const resolvedMinimumWidth = Math.min(
      Math.max(minimumWidth, matchAnchorWidth ? anchorBounds.width : 0),
      availableWidth,
    );
    popover.style.minWidth = resolvedMinimumWidth > 0 ? `${resolvedMinimumWidth}px` : "";
    popover.style.maxWidth = `${Math.min(maximumWidth ?? availableWidth, availableWidth)}px`;

    const popoverBounds = popover.getBoundingClientRect();
    const borderHeight = Math.max(0, popoverBounds.height - popover.clientHeight);
    const naturalHeight = Math.max(popoverBounds.height, popover.scrollHeight + borderHeight);
    const position = placeAnchoredPopover(
      anchorBounds,
      { width: popoverBounds.width, height: naturalHeight },
      viewport,
      { side, align, gap, padding: viewportPadding },
    );

    popover.style.top = `${position.top}px`;
    popover.style.left = `${position.left}px`;
    popover.style.maxHeight = `${position.maxHeight}px`;
    popover.style.visibility = "visible";
    popover.dataset.side = position.side;
  }, [align, anchorRef, gap, matchAnchorWidth, maximumWidth, minimumWidth, side, viewportPadding]);

  useLayoutEffect(() => {
    let frame = 0;
    const scheduleUpdate = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(updatePosition);
    };
    updatePosition();

    const observer = typeof ResizeObserver === "undefined"
      ? null
      : new ResizeObserver(scheduleUpdate);
    if (anchorRef.current) observer?.observe(anchorRef.current);
    if (popoverRef.current) observer?.observe(popoverRef.current);
    window.addEventListener("resize", scheduleUpdate);
    window.addEventListener("scroll", scheduleUpdate, true);
    window.visualViewport?.addEventListener("resize", scheduleUpdate);
    window.visualViewport?.addEventListener("scroll", scheduleUpdate);
    return () => {
      cancelAnimationFrame(frame);
      observer?.disconnect();
      window.removeEventListener("resize", scheduleUpdate);
      window.removeEventListener("scroll", scheduleUpdate, true);
      window.visualViewport?.removeEventListener("resize", scheduleUpdate);
      window.visualViewport?.removeEventListener("scroll", scheduleUpdate);
    };
  }, [anchorRef, updatePosition]);

  useEffect(() => {
    const closeOutside = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (anchorRef.current?.contains(target) || popoverRef.current?.contains(target)) return;
      onClose();
    };
    const closeWithEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("pointerdown", closeOutside, true);
    window.addEventListener("keydown", closeWithEscape, true);
    return () => {
      document.removeEventListener("pointerdown", closeOutside, true);
      window.removeEventListener("keydown", closeWithEscape, true);
    };
  }, [anchorRef, onClose]);

  return createPortal(
    <div
      ref={popoverRef}
      className={className}
      id={id}
      role={role}
      aria-label={ariaLabel}
      onClick={onClick}
      style={{ position: "fixed", zIndex, visibility: "hidden" }}
    >
      {children}
    </div>,
    document.body,
  );
}

function readViewport(): ViewportRectangle {
  const viewport = window.visualViewport;
  return {
    top: viewport?.offsetTop ?? 0,
    left: viewport?.offsetLeft ?? 0,
    width: viewport?.width ?? window.innerWidth,
    height: viewport?.height ?? window.innerHeight,
  };
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(Math.max(value, minimum), Math.max(minimum, maximum));
}
