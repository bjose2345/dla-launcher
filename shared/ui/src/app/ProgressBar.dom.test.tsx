// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { ProgressBar, timeLeftText } from "./ProgressBar";

afterEach(cleanup);

describe("ProgressBar", () => {
  it("fills to the percent and tells screen readers how far it got", () => {
    render(<ProgressBar percent={41.6} label="Download of RJ01678657" />);
    const bar = screen.getByRole("progressbar", { name: "Download of RJ01678657" });
    expect(bar.getAttribute("aria-valuenow")).toBe("42");
    expect(bar.className).toBe("app-progress is-card is-running");
    expect(bar.querySelector("i")?.style.width).toBe("41.6%");
  });

  it("keeps the fill inside the bar", () => {
    render(<ProgressBar percent={130} label="Installing" />);
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("100");
  });

  it("sweeps while the amount is not known, without a value", () => {
    render(<ProgressBar percent={null} size="slim" label="Scanning" />);
    const bar = screen.getByRole("progressbar");
    expect(bar.classList.contains("is-unknown")).toBe(true);
    expect(bar.classList.contains("is-slim")).toBe(true);
    expect(bar.getAttribute("aria-valuenow")).toBeNull();
    expect(bar.querySelector("i")).toBeNull();
  });

  it("keeps what is done visible while it waits or is paused", () => {
    const { rerender } = render(<ProgressBar percent={88} state="waiting" label="Translation" />);
    expect(screen.getByRole("progressbar").className).toBe("app-progress is-card is-waiting");
    rerender(<ProgressBar percent={88} state="paused" label="Translation" />);
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("88");
  });

  it("stays out of the way of screen readers when the text beside it says it all", () => {
    const { container } = render(<ProgressBar percent={10} />);
    expect(screen.queryByRole("progressbar")).toBeNull();
    expect(container.firstElementChild?.getAttribute("aria-hidden")).toBe("true");
  });
});

describe("timeLeftText", () => {
  const t = (key: string, values?: Record<string, string | number>) =>
    `${key}${values ? ` ${JSON.stringify(values)}` : ""}`;

  it("rounds up to whole minutes, then hours and minutes", () => {
    expect(timeLeftText(t, 0)).toBeNull();
    expect(timeLeftText(t, 45)).toBe("progress.timeLeft.soon");
    expect(timeLeftText(t, 61)).toBe('progress.timeLeft.minutes {"minutes":2}');
    expect(timeLeftText(t, 125 * 60)).toBe('progress.timeLeft.hours {"hours":2,"minutes":5}');
  });
});
