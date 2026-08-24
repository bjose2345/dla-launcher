// @vitest-environment jsdom

import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useDocumentScrollLock } from "./useDocumentScrollLock";

afterEach(() => {
  cleanup();
  document.body.style.overflow = "";
  document.documentElement.style.overflow = "";
});

describe("useDocumentScrollLock", () => {
  it("keeps scrolling locked until every overlapping owner releases it", () => {
    document.body.style.overflow = "clip";
    document.documentElement.style.overflow = "scroll";
    const view = render(<Locks first second />);

    expect(document.body.style.overflow).toBe("hidden");
    expect(document.documentElement.style.overflow).toBe("hidden");

    view.rerender(<Locks first={false} second />);
    expect(document.body.style.overflow).toBe("hidden");
    expect(document.documentElement.style.overflow).toBe("hidden");

    view.rerender(<Locks first={false} second={false} />);
    expect(document.body.style.overflow).toBe("clip");
    expect(document.documentElement.style.overflow).toBe("scroll");
  });
});

function Locks({ first, second }: { first: boolean; second: boolean }) {
  useDocumentScrollLock(first);
  useDocumentScrollLock(second);
  return null;
}
