// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useRef, useState } from "react";

import { AnchoredPopover, placeAnchoredPopover } from "./AnchoredPopover";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("placeAnchoredPopover", () => {
  it("keeps a centered calendar inside the horizontal viewport edges", () => {
    const position = placeAnchoredPopover(
      rectangle({ left: 290, top: 30, width: 40, height: 40 }),
      { width: 310, height: 220 },
      { top: 0, left: 0, width: 360, height: 640 },
      { align: "center" },
    );

    expect(position.left).toBe(38);
    expect(position.left + 310).toBe(348);
  });

  it("flips above the trigger when the lower edge has less room", () => {
    const position = placeAnchoredPopover(
      rectangle({ left: 120, top: 500, width: 40, height: 40 }),
      { width: 220, height: 260 },
      { top: 0, left: 0, width: 800, height: 600 },
      { gap: 8 },
    );

    expect(position.side).toBe("top");
    expect(position.top).toBe(232);
    expect(position.maxHeight).toBe(480);
  });

  it("uses visual viewport offsets while aligning to the end", () => {
    const position = placeAnchoredPopover(
      rectangle({ left: 650, top: 300, width: 40, height: 40 }),
      { width: 240, height: 100 },
      { top: 200, left: 400, width: 300, height: 400 },
      { align: "end" },
    );

    expect(position.left).toBe(448);
    expect(position.top).toBe(348);
  });
});

describe("AnchoredPopover", () => {
  it("portals out of clipping containers and closes from an outside pointer", () => {
    vi.stubGlobal("innerWidth", 360);
    vi.stubGlobal("innerHeight", 300);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.getAttribute("aria-label") === "Open calendar") {
        return domRectangle({ left: 290, top: 40, width: 40, height: 40 });
      }
      if (this.getAttribute("role") === "dialog") {
        return domRectangle({ left: 0, top: 0, width: 310, height: 260 });
      }
      return domRectangle({ left: 0, top: 0, width: 0, height: 0 });
    });

    render(<PopoverHarness />);

    const popover = screen.getByRole("dialog", { name: "Calendar" });
    expect(popover.parentElement).toBe(document.body);
    expect(popover.style.position).toBe("fixed");
    expect(popover.style.visibility).toBe("visible");
    expect(popover.style.left).toBe("38px");

    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("dialog", { name: "Calendar" })).toBeNull();
  });
});

function PopoverHarness() {
  const anchorRef = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(true);
  return (
    <div style={{ overflow: "hidden" }}>
      <button ref={anchorRef} type="button" aria-label="Open calendar">Open</button>
      {open ? (
        <AnchoredPopover
          anchorRef={anchorRef}
          role="dialog"
          ariaLabel="Calendar"
          align="center"
          onClose={() => setOpen(false)}
        >
          Calendar contents
        </AnchoredPopover>
      ) : null}
    </div>
  );
}

function rectangle({
  left,
  top,
  width,
  height,
}: {
  left: number;
  top: number;
  width: number;
  height: number;
}) {
  return { left, top, width, height, right: left + width, bottom: top + height };
}

function domRectangle(values: Parameters<typeof rectangle>[0]): DOMRect {
  const bounds = rectangle(values);
  return {
    ...bounds,
    x: bounds.left,
    y: bounds.top,
    toJSON: () => bounds,
  };
}
