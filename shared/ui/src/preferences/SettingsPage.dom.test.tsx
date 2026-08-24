// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { KeyBindingsProvider } from "./KeyBindingsProvider";
import { PresentationProvider } from "./PresentationProvider";
import {
  SettingsPage,
  parseSettingsSearch,
  parseSettingsTab,
  type SettingsTab,
  type WorkPreferenceGateway,
} from "./SettingsPage";
import type { WindowGateway, WindowMetrics } from "./windowSizing";
import type { CoverCacheGateway, CoverCacheSummary } from "./coverCache";

afterEach(() => {
  cleanup();
});

describe("parseSettingsTab", () => {
  it("falls back to general for anything unknown", () => {
    expect(parseSettingsTab("controls")).toBe("controls");
    expect(parseSettingsTab("about")).toBe("general");
    expect(parseSettingsTab("nonsense")).toBe("general");
    expect(parseSettingsTab(undefined)).toBe("general");
    expect(parseSettingsSearch({ tab: "about" })).toEqual({ tab: "general", legacyAbout: true });
    expect(parseSettingsSearch({ tab: "general" })).toEqual({ tab: "general" });
  });
});

describe("SettingsPage", () => {
  it("keeps informational About content out of the Settings tabs", async () => {
    renderSettings();

    const tabs = await screen.findAllByRole("tab");
    expect(tabs).toHaveLength(4);
    expect(screen.queryByRole("tab", { name: "About" })).toBeNull();
  });

  it("draws locale flags as artwork rather than emoji text", async () => {
    renderSettings();

    const english = await screen.findByRole("button", { name: /English/ });
    expect(english.querySelector("svg.locale-flag")).toBeTruthy();
    expect(document.body.textContent).not.toContain("🇺🇸");
    const masks = [...document.querySelectorAll("svg.locale-flag mask")].map((mask) => mask.id);
    expect(new Set(masks).size).toBe(masks.length);
  });

  it("reports the tab change instead of holding it locally", async () => {
    const onTabChange = vi.fn();
    renderSettings({ onTabChange });

    fireEvent.click(await screen.findByRole("tab", { name: /Controls/ }));

    expect(onTabChange).toHaveBeenCalledWith("controls");
  });

  it("implements keyboard navigation for the tablist", async () => {
    renderSettings();

    const general = await screen.findByRole("tab", { name: /General/ });
    general.focus();
    fireEvent.keyDown(general, { key: "ArrowRight" });

    const library = screen.getByRole("tab", { name: /Library/ });
    expect(document.activeElement).toBe(library);
    expect(library.getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("tabpanel").getAttribute("aria-labelledby")).toBe(library.id);
  });

  it("marks the live window size and disables presets larger than the screen", async () => {
    renderSettings({ tab: "display", windowGateway: windowGateway() });

    const current = await screen.findByRole("button", { name: /1440 × 900/ });
    expect(current.getAttribute("aria-pressed")).toBe("true");

    const oversized = screen.getByRole("button", { name: /3840 × 2160/ }) as HTMLButtonElement;
    expect(oversized.disabled).toBe(true);
    expect(oversized.textContent).toContain("Too large");
  });

  it("shows a localized error when window metrics cannot be read", async () => {
    const gateway = windowGateway();
    vi.mocked(gateway.readWindowMetrics).mockRejectedValue(new Error("native window unavailable"));
    renderSettings({ tab: "display", windowGateway: gateway });

    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      expect.stringContaining("Request failed"),
    );
  });

  it("configures artwork retention and capacity independently", async () => {
    const gateway = coverCacheGateway();
    renderSettings({ tab: "library", coverCacheGateway: gateway });

    expect(await screen.findByText("12 cached images · 48 MiB")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Never evict by age" }));
    await waitFor(() => {
      expect(gateway.configure).toHaveBeenCalledWith("never", "standard");
    });
    fireEvent.click(screen.getByRole("button", { name: /Unlimited/ }));
    await waitFor(() => {
      expect(gateway.configure).toHaveBeenCalledWith("never", "unlimited");
    });
  });

  it("rebinds a shortcut from a captured key press", async () => {
    renderSettings({ tab: "controls" });

    const row = (await screen.findByText("Toggle sidebar")).closest(".settings-binding");
    if (!row) throw new Error("binding row was not rendered");
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "Ctrl B" }));

    expect(within(row as HTMLElement).getByRole("button", { name: "Press a key…" })).toBeTruthy();
    fireEvent.keyDown(window, { key: "d", ctrlKey: true });

    await waitFor(() => {
      expect(within(row as HTMLElement).getByRole("button", { name: "Ctrl D" })).toBeTruthy();
    });
  });

  it("does not trigger application shortcuts while capturing a binding", async () => {
    renderSettings({ tab: "controls" });

    const row = (await screen.findByText("Toggle sidebar")).closest(".settings-binding");
    if (!row) throw new Error("binding row was not rendered");
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "Ctrl B" }));
    const leakedShortcut = vi.fn();
    window.addEventListener("keydown", leakedShortcut);
    try {
      fireEvent.keyDown(window, { key: "d", ctrlKey: true });
      expect(leakedShortcut).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("keydown", leakedShortcut);
    }
  });

  it("names the action a key was taken from", async () => {
    renderSettings({ tab: "controls" });

    const row = (await screen.findByText("Toggle sidebar")).closest(".settings-binding");
    if (!row) throw new Error("binding row was not rendered");
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "Ctrl B" }));
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });

    expect(await screen.findByRole("status")).toHaveProperty(
      "textContent",
      expect.stringContaining("Search catalog"),
    );
  });

  it("removes a global key from every overlapping action", async () => {
    renderSettings({ tab: "controls" });

    const row = (await screen.findByText("Toggle sidebar")).closest(".settings-binding");
    if (!row) throw new Error("binding row was not rendered");
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "Ctrl B" }));
    fireEvent.keyDown(window, { key: "ArrowLeft" });

    const status = await screen.findByRole("status");
    expect(status.textContent).toContain("Skip back 10s");
    expect(status.textContent).toContain("Previous page");
  });

  it("cancels a capture on Escape without changing the binding", async () => {
    renderSettings({ tab: "controls" });

    const row = (await screen.findByText("Toggle sidebar")).closest(".settings-binding");
    if (!row) throw new Error("binding row was not rendered");
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "Ctrl B" }));
    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => {
      expect(within(row as HTMLElement).getByRole("button", { name: "Ctrl B" })).toBeTruthy();
    });
  });

  it("keeps a fixed shortcut visible but not rebindable", async () => {
    renderSettings({ tab: "controls" });

    const row = (await screen.findByText("Close reader")).closest(".settings-binding");
    if (!row) throw new Error("binding row was not rendered");
    const chip = within(row as HTMLElement).getByRole("button", { name: "Esc" }) as HTMLButtonElement;
    expect(chip.disabled).toBe(true);
    expect(row.textContent).toContain("Fixed");
  });

  it("manages favorites and hidden works from the Library tab", async () => {
    const replaceWorkPreference = vi.fn().mockResolvedValue(null);
    renderSettings({
      tab: "library",
      workPreferenceGateway: {
        listWorkPreferences: vi.fn().mockResolvedValue([
          { workCode: "RJ00000001", preference: "favorite", updatedAt: "2026-01-02T00:00:00Z" },
          { workCode: "RJ00000002", preference: "not_interested", updatedAt: "2026-01-03T00:00:00Z" },
        ]),
        replaceWorkPreference,
      },
    });

    expect(await screen.findByText("RJ00000001")).toBeTruthy();
    expect(screen.getByText("RJ00000002")).toBeTruthy();

    const remove = screen.getAllByRole("button", { name: "Remove preference" })[0];
    fireEvent.click(remove!);
    await waitFor(() => {
      expect(replaceWorkPreference).toHaveBeenCalledWith("RJ00000001", null);
    });
  });

  it("says so plainly when no preferences are stored", async () => {
    renderSettings({
      tab: "library",
      workPreferenceGateway: {
        listWorkPreferences: vi.fn().mockResolvedValue([]),
        replaceWorkPreference: vi.fn(),
      },
    });

    expect(await screen.findByText("No favorites or hidden works yet.")).toBeTruthy();
  });

  it("opens a preferred work from the Library settings list", async () => {
    const onOpenWork = vi.fn();
    renderSettings({
      tab: "library",
      onOpenWork,
      workPreferenceGateway: {
        listWorkPreferences: vi.fn().mockResolvedValue([
          { workCode: "RJ00000001", preference: "favorite", updatedAt: "2026-01-02T00:00:00Z" },
        ]),
        replaceWorkPreference: vi.fn(),
      },
    });

    fireEvent.click(await screen.findByRole("button", { name: "RJ00000001" }));
    expect(onOpenWork).toHaveBeenCalledWith("RJ00000001");
  });
});

function renderSettings(props: {
  tab?: SettingsTab;
  onTabChange?: (tab: SettingsTab) => void;
  windowGateway?: WindowGateway;
  coverCacheGateway?: CoverCacheGateway;
  workPreferenceGateway?: WorkPreferenceGateway;
  onOpenWork?: (code: string) => void | Promise<void>;
} = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <KeyBindingsProvider>
          <SettingsPage
            tab={props.tab ?? "general"}
            onTabChange={props.onTabChange}
            windowGateway={props.windowGateway}
            coverCacheGateway={props.coverCacheGateway}
            workPreferenceGateway={props.workPreferenceGateway}
            onOpenWork={props.onOpenWork}
          />
        </KeyBindingsProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function coverCacheGateway(): CoverCacheGateway {
  let summary: CoverCacheSummary = {
    retention: "days_180",
    capacity: "standard",
    entryCount: 12,
    storedBytes: 48 * 1024 * 1024,
    maximumBytes: 512 * 1024 * 1024,
    maximumEntries: 4_000,
  };
  return {
    readSummary: vi.fn().mockImplementation(async () => summary),
    configure: vi.fn().mockImplementation(async (retention, capacity) => {
      summary = { ...summary, retention, capacity };
      return summary;
    }),
  };
}

function windowGateway(): WindowGateway {
  const metrics: WindowMetrics = {
    width: 1440,
    height: 900,
    workAreaWidth: 1920,
    workAreaHeight: 1053,
    scaleFactor: 1,
    maximized: false,
    supportsWindowControls: true,
  };
  return {
    readWindowMetrics: vi.fn().mockResolvedValue(metrics),
    resizeWindow: vi.fn().mockResolvedValue(metrics),
    maximizeWindow: vi.fn().mockResolvedValue({ ...metrics, maximized: true }),
  };
}
