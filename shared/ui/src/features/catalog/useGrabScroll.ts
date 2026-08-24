import { useEffect, type RefObject } from "react";

interface DragState {
  pointerId: number;
  originY: number;
  originScrollTop: number;
  dragging: boolean;
}

const dragThreshold = 4;
const interactiveSelector = [
  "a",
  "button",
  "input",
  "select",
  "textarea",
  "[role='button']",
  "[data-grab-scroll-ignore]",
].join(",");

export function useGrabScroll(viewport: RefObject<HTMLDivElement | null>): void {
  useEffect(() => {
    const element = viewport.current;
    if (!element) return;

    let drag: DragState | null = null;
    let suppressClick = false;
    let suppressTimer = 0;

    const handlePointerDown = (event: PointerEvent) => {
      if (
        event.button !== 0
        || event.pointerType === "touch"
        || isInteractiveTarget(event.target)
      ) return;

      event.preventDefault();
      drag = {
        pointerId: event.pointerId,
        originY: event.clientY,
        originScrollTop: element.scrollTop,
        dragging: false,
      };
      element.setPointerCapture(event.pointerId);
    };

    const handlePointerMove = (event: PointerEvent) => {
      if (!drag || drag.pointerId !== event.pointerId) return;
      const distance = event.clientY - drag.originY;
      if (!drag.dragging && Math.abs(distance) < dragThreshold) return;
      if (!drag.dragging) {
        drag.dragging = true;
        element.classList.add("grab-scroll-dragging");
      }

      event.preventDefault();
      element.scrollTop = draggedScrollTop(
        drag.originScrollTop,
        drag.originY,
        event.clientY,
        element.scrollHeight - element.clientHeight,
      );
    };

    const finishPointer = (event: PointerEvent) => {
      if (!drag || drag.pointerId !== event.pointerId) return;
      if (drag.dragging && event.type === "pointerup") {
        suppressClick = true;
        window.clearTimeout(suppressTimer);
        suppressTimer = window.setTimeout(() => {
          suppressClick = false;
        }, 0);
      }

      drag = null;
      element.classList.remove("grab-scroll-dragging");
      if (element.hasPointerCapture(event.pointerId)) {
        element.releasePointerCapture(event.pointerId);
      }
    };

    const handleClick = (event: MouseEvent) => {
      if (!suppressClick) return;
      suppressClick = false;
      event.preventDefault();
      event.stopImmediatePropagation();
    };

    element.classList.add("grab-scroll-enabled");
    element.addEventListener("pointerdown", handlePointerDown);
    element.addEventListener("pointermove", handlePointerMove);
    element.addEventListener("pointerup", finishPointer);
    element.addEventListener("pointercancel", finishPointer);
    element.addEventListener("lostpointercapture", finishPointer);
    element.addEventListener("click", handleClick, true);

    return () => {
      window.clearTimeout(suppressTimer);
      element.classList.remove("grab-scroll-enabled", "grab-scroll-dragging");
      element.removeEventListener("pointerdown", handlePointerDown);
      element.removeEventListener("pointermove", handlePointerMove);
      element.removeEventListener("pointerup", finishPointer);
      element.removeEventListener("pointercancel", finishPointer);
      element.removeEventListener("lostpointercapture", finishPointer);
      element.removeEventListener("click", handleClick, true);
    };
  }, [viewport]);
}

export function draggedScrollTop(
  originScrollTop: number,
  originY: number,
  currentY: number,
  maximumScrollTop: number,
): number {
  const requested = originScrollTop + originY - currentY;
  return Math.min(Math.max(0, requested), Math.max(0, maximumScrollTop));
}

function isInteractiveTarget(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(interactiveSelector) !== null;
}
