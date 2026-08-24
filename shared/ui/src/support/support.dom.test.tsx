// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../preferences/PresentationProvider";
import { ApplicationErrorBoundary, type ApplicationErrorLabels } from "./ApplicationErrorBoundary";
import { installGlobalFaultCapture } from "./globalFaultCapture";
import { SupportRecoveryNotice } from "./SupportRecoveryNotice";
import { SupportPage } from "./SupportPage";
import type { SupportGateway, SupportStatus } from "./types";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("diagnostic recovery", () => {
  it("keeps report creation on a standalone support page", async () => {
    const gateway = supportGateway({ previousShutdownUnclean: false });
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <PresentationProvider>
          <SupportPage gateway={gateway} />
        </PresentationProvider>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("heading", { name: "Send us a report" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Create a diagnostic report" })).toBeTruthy();
    expect(screen.getByText(/Databases, catalogs, packages, media/)).toBeTruthy();
    const copy = screen.getByRole("button", { name: "Copy summary" }) as HTMLButtonElement;
    await waitFor(() => expect(copy.disabled).toBe(false));
    fireEvent.click(copy);
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("safe diagnostic summary"));
    fireEvent.click(screen.getByRole("button", { name: "Save diagnostic report" }));
    await waitFor(() => expect(gateway.saveBundle).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: "Report on GitHub" }));
    await waitFor(() => expect(gateway.openIssue).toHaveBeenCalledOnce());
  });

  it("offers recovery actions only after an unclean shutdown and can dismiss the notice", async () => {
    const gateway = supportGateway();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(
      <PresentationProvider>
        <SupportRecoveryNotice gateway={gateway} onOpenSupport={vi.fn()} />
      </PresentationProvider>,
    );

    expect(await screen.findByText("DLA Launcher did not shut down normally")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Copy summary" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("safe diagnostic summary"));
    fireEvent.click(screen.getByRole("button", { name: "Save diagnostic report" }));
    expect(await screen.findByText("Diagnostic report saved.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(screen.queryByText("DLA Launcher did not shut down normally")).toBeNull());
    expect(gateway.acknowledgeUncleanShutdown).toHaveBeenCalledOnce();
  });

  it("does not show a recovery notice after a clean shutdown", async () => {
    const gateway = supportGateway({ previousShutdownUnclean: false });
    render(
      <PresentationProvider>
        <SupportRecoveryNotice gateway={gateway} onOpenSupport={vi.fn()} />
      </PresentationProvider>,
    );

    await waitFor(() => expect(gateway.readStatus).toHaveBeenCalledOnce());
    expect(screen.queryByRole("complementary")).toBeNull();
  });

  it("records a render failure and keeps copy, save, and reload recovery available", async () => {
    const gateway = supportGateway();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(
      <ApplicationErrorBoundary gateway={gateway} labels={labels}>
        <Broken />
      </ApplicationErrorBoundary>,
    );

    expect(await screen.findByText(labels.title)).toBeTruthy();
    await waitFor(() => expect(gateway.recordFrontendFault).toHaveBeenCalledOnce());
    const report = vi.mocked(gateway.recordFrontendFault).mock.calls[0]![0];
    expect(report.kind).toBe("frontendRender");
    expect(report.message).toBe("render exploded");
    fireEvent.click(screen.getByRole("button", { name: labels.save }));
    await waitFor(() => expect(gateway.saveBundle).toHaveBeenCalledOnce());
    expect(await screen.findByRole("button", { name: labels.saved })).toBeTruthy();
    expect(screen.getByRole("button", { name: labels.reload })).toBeTruthy();
  });

  it("deduplicates repeated global errors and removes its listeners", async () => {
    const gateway = supportGateway();
    const uninstall = installGlobalFaultCapture(gateway);
    const error = new Error("async exploded");
    window.dispatchEvent(new ErrorEvent("error", { error, message: error.message }));
    window.dispatchEvent(new ErrorEvent("error", { error, message: error.message }));
    await waitFor(() => expect(gateway.recordFrontendFault).toHaveBeenCalledOnce());
    expect(vi.mocked(gateway.recordFrontendFault).mock.calls[0]![0].kind).toBe("frontendError");

    uninstall();
    window.dispatchEvent(new Event("error"));
    expect(gateway.recordFrontendFault).toHaveBeenCalledOnce();
  });
});

function Broken(): never {
  throw new Error("render exploded");
}

const labels: ApplicationErrorLabels = {
  title: "Interface stopped",
  help: "Save a report and reload.",
  copy: "Copy summary",
  copied: "Copied",
  save: "Save report",
  saving: "Saving",
  saved: "Saved",
  reload: "Reload",
  actionFailed: "Failed",
};

function supportGateway(overrides: Partial<SupportStatus> = {}): SupportGateway {
  const status: SupportStatus = {
    schemaVersion: 1,
    previousShutdownUnclean: true,
    previousRunId: "previous-run",
    lastFault: null,
    retainedLogFiles: 2,
    retainedFaultFiles: 1,
    estimatedBundleBytes: 4096,
    maxBundleBytes: 15 * 1024 * 1024,
    summary: "safe diagnostic summary",
    ...overrides,
  };
  return {
    readStatus: vi.fn().mockResolvedValue(status),
    acknowledgeUncleanShutdown: vi.fn().mockResolvedValue(undefined),
    recordFrontendFault: vi.fn().mockResolvedValue(undefined),
    saveBundle: vi.fn().mockResolvedValue({ outcome: "saved", fileName: "report.zip", bytes: 1200 }),
    openIssue: vi.fn().mockResolvedValue(undefined),
    openProject: vi.fn().mockResolvedValue(undefined),
  };
}
