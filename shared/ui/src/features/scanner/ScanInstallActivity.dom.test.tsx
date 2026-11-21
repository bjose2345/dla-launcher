// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { ScanInstallActivity } from "./ScanInstallActivity";
import type { ScanInstall, ScanInstallGateway, ScanInstallView } from "./types";

afterEach(() => {
  cleanup();
});

describe("ScanInstallActivity", () => {
  it("shows the running install and announces it once it is installed", async () => {
    let publish: (view: ScanInstallView) => void = () => undefined;
    const gateway: ScanInstallGateway = {
      readInstalls: vi.fn().mockResolvedValue(view(1, [install("installing"), install("queued", "installation-2")])),
      subscribeInstalls: vi.fn(async (listener) => {
        publish = listener;
        return () => undefined;
      }),
      selectInstallFolder: vi.fn(),
      installArchive: vi.fn(),
      cancelInstall: vi.fn(),
    };
    const onShowInLibrary = vi.fn();
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <PresentationProvider>
          <ScanInstallActivity
            gateway={gateway}
            progressGateway={{
              readPackagePreparationProgress: vi.fn().mockResolvedValue({
                operationId: "operation-1",
                installationId: "installation-1",
                stage: "extracting",
                counters: { totalBytes: 100, processedBytes: 40, totalFiles: 10, processedFiles: 2 },
                currentPath: null,
                detail: "",
              }),
              subscribePackagePreparationProgress: vi.fn().mockResolvedValue(() => undefined),
            }}
            catalogGateway={{ readWorks: vi.fn().mockResolvedValue([]) }}
            onOpenScanner={() => undefined}
            onShowInLibrary={onShowInLibrary}
          />
        </PresentationProvider>
      </QueryClientProvider>,
    );

    expect(await screen.findByRole("button", { name: "SiNiSistar is installing. Open the Scanner." })).toBeTruthy();
    expect(await screen.findByText("Installing · 40%")).toBeTruthy();
    expect(screen.getByText("1 more waiting")).toBeTruthy();

    act(() => publish(view(2, [install("installed"), install("queued", "installation-2")])));

    expect(await screen.findByText("SiNiSistar is installed and in your Library")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Open in Library" }));
    expect(onShowInLibrary).toHaveBeenCalledWith("installation-1");
    await waitFor(() => expect(screen.queryByText("SiNiSistar is installed and in your Library")).toBeNull());
  });

  it("offers to keep both when the install folder already has the work's folder", async () => {
    let publish: (view: ScanInstallView) => void = () => undefined;
    const gateway: ScanInstallGateway = {
      readInstalls: vi.fn().mockResolvedValue(view(1, [install("installing")])),
      subscribeInstalls: vi.fn(async (listener) => {
        publish = listener;
        return () => undefined;
      }),
      selectInstallFolder: vi.fn(),
      installArchive: vi.fn().mockResolvedValue(view(3, [install("queued")])),
      cancelInstall: vi.fn(),
    };
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <PresentationProvider>
          <ScanInstallActivity
            gateway={gateway}
            progressGateway={{
              readPackagePreparationProgress: vi.fn().mockResolvedValue(null),
              subscribePackagePreparationProgress: vi.fn().mockResolvedValue(() => undefined),
            }}
            catalogGateway={{ readWorks: vi.fn().mockResolvedValue([]) }}
            onOpenScanner={() => undefined}
            onShowInLibrary={() => undefined}
          />
        </PresentationProvider>
      </QueryClientProvider>,
    );
    await screen.findByRole("button", { name: "SiNiSistar is installing. Open the Scanner." });

    act(() => publish(view(2, [{
      ...install("failed"),
      detail: "the install folder already has a folder named RJ137267",
      conflict: { folderName: "RJ137267", keepBothName: "RJ137267 (2)", replaceable: true },
    }])));

    expect(await screen.findByText("Your install folder already has a folder named RJ137267.")).toBeTruthy();
    expect(screen.queryByText(/Technical detail/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Keep both" }));
    await waitFor(() => expect(gateway.installArchive).toHaveBeenCalledWith("installation-1", "keep_both"));
  });
});

function view(revision: number, installs: ScanInstall[]): ScanInstallView {
  return { revision, installFolder: "/home/developer/DLA Library", installs };
}

function install(status: ScanInstall["status"], installationId = "installation-1"): ScanInstall {
  return {
    installationId,
    key: `package:entry:${installationId}`,
    name: "SiNiSistar.zip",
    workCode: null,
    status,
    detail: null,
    conflict: null,
  };
}
