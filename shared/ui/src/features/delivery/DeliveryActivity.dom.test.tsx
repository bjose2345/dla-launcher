// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import type { CatalogWork } from "../catalog/types";
import { DeliveryActivity } from "./DeliveryActivity";
import type { DeliveryEvents, DeliveryGateway, DeliveryJob, DeliveryView } from "./types";

afterEach(() => {
  cleanup();
});

function job(id: string, workCode: string, status: DeliveryJob["status"]): DeliveryJob {
  return {
    id,
    workCode,
    releaseDate: "2018-11-30",
    version: "",
    updateDate: "",
    archives: [{ fileName: `${workCode}.zip`, size: 1000, sha256: "a".repeat(64), remotePath: null, complete: false }],
    installFolder: "C:\\Works",
    status,
    receivedBytes: status === "downloading" ? 250 : 0,
    problem: null,
    detail: null,
    installationId: status === "installed" ? `installation-${id}` : null,
    createdAt: "2026-10-08T09:00:00Z",
    updatedAt: "2026-10-08T09:00:00Z",
  };
}

function view(jobs: DeliveryJob[], revision = 0): DeliveryView {
  return { revision, available: true, account: "demo", installFolder: "C:\\Works", jobs, progress: null };
}

function renderActivity(initial: DeliveryView) {
  let events: DeliveryEvents | null = null;
  const gateway = {
    read: vi.fn().mockResolvedValue(initial),
    subscribe: (next: DeliveryEvents) => {
      events = next;
      return () => undefined;
    },
  } as unknown as DeliveryGateway;
  const catalogGateway = {
    readWorks: vi.fn(async (codes: string[]) => codes.map((code) => ({ code, title: `Title ${code}` }) as CatalogWork)),
  };
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <DeliveryActivity
          gateway={gateway}
          catalogGateway={catalogGateway}
          onOpenDownloads={() => undefined}
          onOpenInstallation={() => undefined}
        />
      </PresentationProvider>
    </QueryClientProvider>,
  );
  return {
    loaded: () => waitFor(() => expect(client.getQueryData(["delivery"])).toBeDefined()),
    emit: (next: DeliveryView) => act(() => events!.onChange(next)),
  };
}

describe("DeliveryActivity", () => {
  it("shows the running job in the sidebar", async () => {
    renderActivity(view([job("1", "RJ01", "downloading")]));

    expect(await screen.findByText("Title RJ01")).toBeTruthy();
    expect(document.querySelector(".delivery-activity progress")).toBeTruthy();
  });

  it("announces a job that finished before the view first showed it, but not older jobs", async () => {
    const { loaded, emit } = renderActivity(view([job("old", "RJ00", "installed")]));
    await loaded();
    await act(async () => undefined);
    expect(screen.queryByRole("status")).toBeNull();

    await emit(view([job("old", "RJ00", "installed"), job("new", "RJ02", "installed")], 1));

    // The code shows until the catalog title arrives.
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Title RJ02"));
    expect(screen.getByRole("status").textContent).not.toContain("Title RJ00");
  });
});
