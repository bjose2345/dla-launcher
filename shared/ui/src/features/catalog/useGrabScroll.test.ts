import { describe, expect, it } from "vitest";

import { draggedScrollTop } from "./useGrabScroll";

describe("shared catalog grab scrolling", () => {
  it("moves the viewport opposite to the dragged content", () => {
    expect(draggedScrollTop(400, 300, 220, 1200)).toBe(480);
    expect(draggedScrollTop(400, 220, 300, 1200)).toBe(320);
  });

  it("clamps dragging at the beginning and end", () => {
    expect(draggedScrollTop(20, 100, 300, 1200)).toBe(0);
    expect(draggedScrollTop(1180, 300, 100, 1200)).toBe(1200);
  });
});
