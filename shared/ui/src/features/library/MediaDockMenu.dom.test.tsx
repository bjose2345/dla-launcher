// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useState } from "react";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { MediaDockMenu, MediaDockMenuGroup, MediaDockMenuItem } from "./MediaDockMenu";

afterEach(cleanup);

describe("MediaDockMenu", () => {
  it("lets the dock reserve layout space while the menu is open", () => {
    render(<MenuHarness />);

    fireEvent.click(screen.getByRole("button", { name: "More options" }));

    expect(screen.getByTestId("dock").classList.contains("has-open-menu")).toBe(true);
    expect(screen.getByRole("menu", { name: "More options" })).toBeTruthy();

    fireEvent.click(screen.getByRole("menuitemradio", { name: "1×" }));

    expect(screen.getByTestId("dock").classList.contains("has-open-menu")).toBe(false);
    expect(screen.queryByRole("menu", { name: "More options" })).toBeNull();
  });
});

function MenuHarness() {
  const [open, setOpen] = useState(false);
  return (
    <PresentationProvider>
      <section className={open ? "has-open-menu" : undefined} data-testid="dock">
        <MediaDockMenu label="More options" open={open} onOpenChange={setOpen}>
          <MediaDockMenuGroup labelKey="media.menu.speed">
            <MediaDockMenuItem active label="1×" onSelect={() => undefined} />
          </MediaDockMenuGroup>
        </MediaDockMenu>
      </section>
    </PresentationProvider>
  );
}
