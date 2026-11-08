// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { MediaDockMenu, MediaDockMenuGroup, MediaDockMenuItem } from "./MediaDockMenu";

afterEach(cleanup);

describe("MediaDockMenu", () => {
  it("opens above the dock without requesting layout space", () => {
    render(<MenuHarness />);
    const dock = screen.getByTestId("dock");

    fireEvent.click(screen.getByRole("button", { name: "More options" }));

    expect(dock.className).toBe("media-dock is-video");
    expect(screen.getByRole("menu", { name: "More options" })).toBeTruthy();
    // The setting shows its value and opens its choices in place.
    fireEvent.click(screen.getByRole("menuitem", { name: "Playback speed, 1×" }));
    expect(screen.getByRole("menu", { name: "More options" })).toBeTruthy();

    fireEvent.click(screen.getByRole("menuitemradio", { name: "1×" }));

    expect(dock.className).toBe("media-dock is-video");
    expect(screen.queryByRole("menu", { name: "More options" })).toBeNull();
  });

  it("closes when a click moves focus to the native video surface", () => {
    render(<MenuHarness />);

    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    expect(screen.getByRole("menu", { name: "More options" })).toBeTruthy();

    fireEvent.blur(window);

    expect(screen.queryByRole("menu", { name: "More options" })).toBeNull();
  });

  it("stays open through the blur that follows a press on one of its items", () => {
    const selected = vi.fn();
    render(<MenuHarness onSelect={selected} />);

    fireEvent.click(screen.getByRole("button", { name: "More options" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Playback speed, 1×" }));
    const item = screen.getByRole("menuitemradio", { name: "1×" });
    fireEvent.pointerDown(item);
    fireEvent.blur(window);

    expect(screen.getByRole("menu", { name: "More options" })).toBeTruthy();

    fireEvent.click(item);

    expect(selected).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu", { name: "More options" })).toBeNull();
  });
});

function MenuHarness({ onSelect = () => undefined }: { onSelect?: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <PresentationProvider>
      <section className="media-dock is-video" data-testid="dock">
        <MediaDockMenu label="More options" open={open} gap={-16} onOpenChange={setOpen}>
          <MediaDockMenuGroup labelKey="media.menu.speed">
            <MediaDockMenuItem active label="1×" onSelect={onSelect} />
          </MediaDockMenuGroup>
        </MediaDockMenu>
      </section>
    </PresentationProvider>
  );
}
