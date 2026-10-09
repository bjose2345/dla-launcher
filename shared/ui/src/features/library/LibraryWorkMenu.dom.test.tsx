// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { LibraryWorkMenu } from "./LibraryWorkMenu";

afterEach(() => {
  cleanup();
});

describe("LibraryWorkMenu", () => {
  it("opens the catalog page first when the work is in the catalog", () => {
    const onOpenWork = vi.fn();
    render(
      <PresentationProvider>
        <LibraryWorkMenu title="Goblin Burrow" onOpenWork={onOpenWork} onManage={vi.fn()} onBrowse={vi.fn()} />
      </PresentationProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "More for Goblin Burrow" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Catalog page",
      "Edit details…",
      "Browse files…",
      "Remove from Library…",
    ]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Catalog page" }));
    expect(onOpenWork).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("leaves out what the work or platform cannot do", () => {
    render(
      <PresentationProvider>
        <LibraryWorkMenu title="old_backup" onManage={vi.fn()} />
      </PresentationProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "More for old_backup" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Edit details…",
      "Remove from Library…",
    ]);
  });
});
