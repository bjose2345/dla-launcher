// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { developers } from "../preferences/developers";
import { PresentationProvider } from "../preferences/PresentationProvider";
import type { SystemGateway, SystemReport } from "../preferences/systemReport";
import type { WindowGateway, WindowMetrics } from "../preferences/windowSizing";
import { AboutPage } from "./AboutPage";

afterEach(() => {
  cleanup();
});

describe("AboutPage", () => {
  it("renders as a standalone About page instead of a Settings tab", async () => {
    renderAbout();

    expect(await screen.findByRole("heading", { level: 1, name: "About" })).toBeTruthy();
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(screen.queryByRole("heading", { level: 1, name: "Settings" })).toBeNull();
  });

  it("credits every developer in the roster with their quote", async () => {
    renderAbout();

    await screen.findByText(developers[0]!.name);
    for (const developer of developers) {
      expect(screen.getByText(developer.name)).toBeTruthy();
      expect(screen.getByText(developer.quote)).toBeTruthy();
      if (developer.quoteEmoji) expect(screen.getByText(developer.quoteEmoji)).toBeTruthy();
    }
    expect(document.querySelectorAll(".settings-portrait img")).toHaveLength(developers.length);
    expect(document.querySelector(".settings-developer.is-aura")).toBeTruthy();
    expect(document.querySelector(".settings-developer.is-arcane")).toBeTruthy();
  });

  it("gives every developer their own effect canvas behind the card", async () => {
    renderAbout();

    await screen.findByText(developers[0]!.name);
    const canvases = document.querySelectorAll<HTMLCanvasElement>(".settings-developer-canvas");
    expect(canvases).toHaveLength(developers.length);
    canvases.forEach((canvas) => {
      expect(canvas.getAttribute("aria-hidden")).toBe("true");
      expect(canvas.closest(".settings-developer")?.firstElementChild).toBe(canvas);
    });
  });

  it("reports system facts that help with a bug report", async () => {
    renderAbout({ systemGateway: systemGateway(), windowGateway: windowGateway() });

    expect(await screen.findByText("Ubuntu 24.04.1 LTS · x86_64")).toBeTruthy();
    expect(screen.getByText("7.0.0-30-generic")).toBeTruthy();
    expect(screen.getByText(/AMD Ryzen 9 5900X 12-Core Processor · 24 cores/)).toBeTruthy();
    expect(screen.getByText("WebKitGTK 2.44.0")).toBeTruthy();
    expect(screen.getByText("v0.1.0-alpha.1")).toBeTruthy();
    expect(await screen.findByText("1920 × 1053")).toBeTruthy();
    expect(document.body.textContent).not.toContain("Scale");
  });

  it("falls back to Unknown when no native system gateways are supplied", async () => {
    renderAbout();

    expect((await screen.findAllByText("Unknown")).length).toBeGreaterThan(0);
  });

  it("opens the configured project from the developer section without embedding support tools", async () => {
    const onOpenProject = vi.fn().mockResolvedValue(undefined);
    renderAbout({ onOpenProject });

    const credits = (await screen.findByRole("heading", { name: "The people behind the launcher" })).closest("section");
    const build = screen.getByRole("heading", { name: "About this build" }).closest("section");
    if (!credits || !build) throw new Error("About sections were not rendered");

    fireEvent.click(within(credits).getByRole("button", { name: "Visit our GitHub" }));
    await waitFor(() => expect(onOpenProject).toHaveBeenCalledOnce());
    expect(within(build).queryByRole("button", { name: "Visit our GitHub" })).toBeNull();
    expect(screen.queryByText("Create a diagnostic report")).toBeNull();
  });
});

function renderAbout(props: {
  windowGateway?: WindowGateway;
  systemGateway?: SystemGateway;
  onOpenProject?: () => void | Promise<void>;
  version?: string;
} = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <AboutPage
          windowGateway={props.windowGateway}
          systemGateway={props.systemGateway}
          onOpenProject={props.onOpenProject}
          version={props.version ?? "v0.1.0-alpha.1"}
        />
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function systemGateway(): SystemGateway {
  const report: SystemReport = {
    os: "Linux",
    osVersion: "Ubuntu 24.04.1 LTS",
    kernel: "7.0.0-30-generic",
    arch: "x86_64",
    cpu: "AMD Ryzen 9 5900X 12-Core Processor",
    cpuCores: 24,
    memoryBytes: 33_554_432_000,
    webview: "WebKitGTK 2.44.0",
  };
  return { readSystemReport: vi.fn().mockResolvedValue(report) };
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
