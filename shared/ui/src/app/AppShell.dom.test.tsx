// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { PresentationProvider } from "../preferences/PresentationProvider";
import { KeyBindingsProvider } from "../preferences/KeyBindingsProvider";
import { AppShell, type AppShellNavItem } from "./AppShell";

const narrowViewportQuery = "(max-width: 900px)";
const mediaQueryListeners = new Set<(event: MediaQueryListEvent) => void>();
let narrowViewport = false;

afterEach(() => {
  cleanup();
  mediaQueryListeners.clear();
});

beforeEach(() => {
  mediaQueryListeners.clear();
  narrowViewport = false;
  window.matchMedia = ((query: string) => ({
    get matches() { return query === narrowViewportQuery && narrowViewport; },
    media: query,
    onchange: null,
    addEventListener: (type: string, listener: (event: MediaQueryListEvent) => void) => {
      if (type === "change") mediaQueryListeners.add(listener);
    },
    removeEventListener: (type: string, listener: (event: MediaQueryListEvent) => void) => {
      if (type === "change") mediaQueryListeners.delete(listener);
    },
    addListener: (listener: (event: MediaQueryListEvent) => void) => mediaQueryListeners.add(listener),
    removeListener: (listener: (event: MediaQueryListEvent) => void) => mediaQueryListeners.delete(listener),
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
});

const navigation: AppShellNavItem[] = [
  { to: "/", label: "Catalog", icon: "catalog", exact: true },
  { to: "/library", label: "Library", icon: "library" },
  { to: "/settings", label: "Settings", icon: "settings", group: "secondary" },
  { to: "/support", label: "Support", icon: "support", group: "secondary" },
  { to: "/about", label: "About", icon: "about", group: "secondary" },
];

describe("AppShell", () => {
  it("renders navigation in the product sidebar", async () => {
    renderShell();

    const sidebar = await screen.findByRole("navigation", { name: "Primary navigation" });
    expect(sidebar.textContent).toContain("Catalog");
    expect(sidebar.textContent).toContain("Library");

    const secondary = screen.getByRole("navigation", { name: "Secondary navigation" });
    const secondaryText = secondary.textContent ?? "";
    expect(secondaryText).toContain("Settings");
    expect(secondaryText).toContain("Support");
    expect(secondaryText).toContain("About");
    expect(secondaryText.indexOf("Settings")).toBeLessThan(secondaryText.indexOf("Support"));
    expect(secondaryText.indexOf("Support")).toBeLessThan(secondaryText.indexOf("About"));
  });

  it("collapses and expands from the toggle and remembers the choice", async () => {
    const { unmount } = renderShell();

    const collapse = await screen.findByRole("button", { name: "Collapse sidebar" });
    expect(collapse.closest(".app-topbar-brand")).toBeTruthy();
    expect(collapse.nextElementSibling?.classList.contains("brand")).toBe(true);
    expect(collapse.querySelector(".lucide-panel-left-close")).toBeTruthy();
    fireEvent.click(collapse);

    const expand = await screen.findByRole("button", { name: "Expand sidebar" });
    expect(expand.getAttribute("aria-expanded")).toBe("false");
    expect(expand.querySelector(".lucide-panel-left-open")).toBeTruthy();
    unmount();

    renderShell();
    expect(await screen.findByRole("button", { name: "Expand sidebar" })).toBeTruthy();
  });

  it("toggles the sidebar with Ctrl+B", async () => {
    renderShell();
    await screen.findByRole("button", { name: "Collapse sidebar" });

    fireEvent.keyDown(window, { key: "b", ctrlKey: true });

    expect(await screen.findByRole("button", { name: "Expand sidebar" })).toBeTruthy();
  });

  it("updates the shortcut behavior and accessible hint after rebinding", async () => {
    window.localStorage.setItem(
      "dla-launcher:key-bindings:v1",
      JSON.stringify({ toggleSidebar: ["ctrl+d"] }),
    );
    renderShell();

    const collapse = await screen.findByRole("button", { name: "Collapse sidebar" });
    expect(collapse.getAttribute("aria-keyshortcuts")).toBe("Control+D Meta+D");

    const oldShortcut = new KeyboardEvent("keydown", {
      key: "b",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    window.dispatchEvent(oldShortcut);
    expect(oldShortcut.defaultPrevented).toBe(false);
    expect(screen.getByRole("button", { name: "Collapse sidebar" })).toBeTruthy();

    fireEvent.keyDown(window, { key: "d", ctrlKey: true });
    expect(await screen.findByRole("button", { name: "Expand sidebar" })).toBeTruthy();
  });

  it("forces the narrow layout without changing the saved desktop preference", async () => {
    narrowViewport = true;
    renderShell();

    const navigation = await screen.findByRole("navigation", { name: "Primary navigation" });
    const sidebar = navigation.closest("aside");
    expect(sidebar?.getAttribute("data-collapsed")).toBe("true");
    expect(screen.queryByRole("button", { name: "Expand sidebar" })).toBeNull();

    fireEvent.keyDown(window, { key: "b", ctrlKey: true });
    act(() => setNarrowViewport(false));

    await waitFor(() => {
      expect(sidebar?.getAttribute("data-collapsed")).toBe("false");
      expect(screen.getByRole("button", { name: "Collapse sidebar" })).toBeTruthy();
    });
  });

  it("labels nav items with a title only while collapsed", async () => {
    renderShell();

    const link = await screen.findByRole("link", { name: "Library" });
    expect(link.getAttribute("aria-label")).toBe("Library");
    expect(link.getAttribute("title")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }));

    await waitFor(() => {
      expect(screen.getByRole("link", { name: "Library" }).getAttribute("title")).toBe("Library");
    });
  });

  it("keeps the corner to the mark alone, with the candidate as its tooltip", async () => {
    renderShell();

    const brand = await screen.findByRole("link", { name: "Home" });
    expect(brand.getAttribute("title")).toBe("Tauri 2 candidate");
    expect(document.body.textContent).not.toContain("DLA Launcher");
    expect(document.body.textContent).not.toContain("v0.1.0-alpha.1");
    expect(document.querySelector(".app-version")).toBeNull();
  });

  it("renders no sidebar when a candidate supplies no navigation", async () => {
    renderShell([]);

    await screen.findByRole("link", { name: "Home" });
    expect(screen.queryByRole("navigation", { name: "Primary navigation" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Collapse sidebar" })).toBeNull();
  });
});

function setNarrowViewport(matches: boolean) {
  narrowViewport = matches;
  const event = { matches, media: narrowViewportQuery } as MediaQueryListEvent;
  for (const listener of mediaQueryListeners) listener(event);
}

function renderShell(items: AppShellNavItem[] = navigation) {
  const rootRoute = createRootRoute({
    component: () => (
      <AppShell candidate="Tauri 2 candidate" navigation={items} />
    ),
  });
  const indexRoute = createRoute({ getParentRoute: () => rootRoute, path: "/", component: () => null });
  const libraryRoute = createRoute({ getParentRoute: () => rootRoute, path: "/library", component: () => null });
  const settingsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/settings", component: () => null });
  const router = createRouter({
    routeTree: rootRoute.addChildren([indexRoute, libraryRoute, settingsRoute]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  return render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <KeyBindingsProvider>
          <RouterProvider router={router as never} />
        </KeyBindingsProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
}
