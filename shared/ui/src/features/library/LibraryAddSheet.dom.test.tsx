// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import type { CatalogWork } from "../catalog/types";
import type { ScanFinding, ScanInstallGateway, ScanInstallView } from "../scanner/types";
import type { LibraryAddFinding, LibraryAddGateway, LibraryAddView } from "./libraryAdd";
import { LibraryAddButton, LibraryAddSheet } from "./LibraryAddSheet";

afterEach(() => {
  cleanup();
});

describe("LibraryAddSheet", () => {
  it("adds the ticked works, waits for a pick where several works fit, and offers to delete archives", async () => {
    const gateway = addGateway();
    renderWithProviders(<LibraryAddSheet {...sheetProps(gateway, reviewView())} />);

    expect(await screen.findByText("5 works found")).toBeTruthy();
    expect(await screen.findByText("Newer version out")).toBeTruthy();
    expect(screen.getByText("Not in the catalog")).toBeTruthy();
    expect((screen.getByRole("checkbox", { name: /RJ01554677/ }) as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByText("1 still need the work picked")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Add 3 works" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Pick" }));
    fireEvent.click(screen.getByRole("button", { name: /RJ02000002/ }));
    expect(screen.getByRole("button", { name: "Add 4 works" })).toBeTruthy();

    fireEvent.click(screen.getByLabelText("Delete archives after unpacking"));
    fireEvent.click(screen.getByRole("button", { name: "Add 4 works" }));

    await waitFor(() => expect(gateway.confirmLibraryAdd).toHaveBeenCalledOnce());
    const request = gateway.confirmLibraryAdd.mock.calls[0]![0];
    expect(request.deleteArchives).toBe(true);
    expect(request.items).toEqual([
      { sourceId: "s1", key: "a", workCode: null, unknown: false },
      { sourceId: "s1", key: "b", workCode: null, unknown: false },
      { sourceId: "s1", key: "c", workCode: null, unknown: true },
      { sourceId: "s1", key: "f", workCode: "RJ02000002", unknown: false },
    ]);
  });

  it("follows each work as it arrives and offers metadata for a work outside the catalog", async () => {
    const gateway = addGateway();
    const onManage = vi.fn();
    const view: LibraryAddView = {
      ...reviewView(),
      phase: "adding",
      jobs: [
        { sourceId: "s1", key: "a", kind: "copy", state: "running", installationId: null, error: null, warning: null, doneBytes: 45, totalBytes: 100 },
        { sourceId: "s1", key: "c", kind: "add", state: "done", installationId: "installation-c", error: null, warning: null, doneBytes: null, totalBytes: null },
        { sourceId: "s1", key: "b", kind: "unpack", state: "failed", installationId: null, error: "A folder named RJ01111111 is already in your install folder", warning: null, doneBytes: null, totalBytes: null },
      ],
    };
    renderWithProviders(<LibraryAddSheet {...sheetProps(gateway, view)} onManage={onManage} />);

    expect(await screen.findByText("Copying 45%")).toBeTruthy();
    expect(screen.getByText("A folder named RJ01111111 is already in your install folder")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add metadata" }));
    expect(onManage).toHaveBeenCalledWith("installation-c");
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("asks for the install folder before the first add, and stops if none is chosen", async () => {
    const gateway = addGateway();
    const installs = installGateway(null);
    const onView = vi.fn();
    renderWithProviders(<LibraryAddButton gateway={gateway} installs={installs} view={null} onView={onView} onOpen={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Work folders/ }));
    await waitFor(() => expect(installs.selectInstallFolder).toHaveBeenCalledOnce());
    expect(gateway.startLibraryAdd).not.toHaveBeenCalled();

    installs.selectInstallFolder.mockResolvedValueOnce(installView("/library"));
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /Archives/ }));
    await waitFor(() => expect(gateway.startLibraryAdd).toHaveBeenCalledWith("archives"));
    await waitFor(() => expect(onView).toHaveBeenCalled());
  });
});

function sheetProps(gateway: ReturnType<typeof addGateway>, view: LibraryAddView) {
  return {
    view,
    gateway,
    installs: installGateway("/library"),
    catalogGateway: { readWorks: vi.fn(async (codes: string[]) => codes.map(work)) },
    onHide: vi.fn(),
    onManage: vi.fn(),
  };
}

function renderWithProviders(node: ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PresentationProvider>{node}</PresentationProvider>
    </QueryClientProvider>,
  );
}

function addGateway() {
  return {
    startLibraryAdd: vi.fn().mockResolvedValue(reviewView()),
    readLibraryAdd: vi.fn().mockResolvedValue(null),
    confirmLibraryAdd: vi.fn().mockResolvedValue({ ...reviewView(), phase: "adding" }),
    cancelLibraryAdd: vi.fn().mockResolvedValue(null),
    dismissLibraryAdd: vi.fn().mockResolvedValue(undefined),
    subscribeLibraryAdd: vi.fn().mockResolvedValue(() => undefined),
  } satisfies LibraryAddGateway;
}

function installGateway(folder: string | null) {
  return {
    readInstalls: vi.fn().mockResolvedValue(installView(folder)),
    subscribeInstalls: vi.fn().mockResolvedValue(() => undefined),
    selectInstallFolder: vi.fn<() => Promise<ScanInstallView | null>>().mockResolvedValue(null),
    installArchive: vi.fn(),
    cancelInstall: vi.fn(),
  } satisfies ScanInstallGateway;
}

function installView(folder: string | null): ScanInstallView {
  return { revision: 1, installFolder: folder, installs: [] } as unknown as ScanInstallView;
}

function reviewView(): LibraryAddView {
  return {
    batchId: "batch-1",
    revision: 3,
    phase: "review",
    installFolder: "/library",
    deleteArchives: false,
    sources: [{ sourceId: "s1", displayName: "Downloads", kind: "scan", status: "done", error: null, sessionId: "session-1" }],
    findings: [
      found("a", { relativePath: "RJ01234567 (2024-05-01-01)", workCode: "RJ01234567" }, "copy"),
      found("b", { relativePath: "RJ01111111.zip", kind: "archive", workCode: "RJ01111111" }, "unpack"),
      found("c", { relativePath: "My set", state: "unknown", workCode: null, candidates: [] }, "move"),
      found("d", { relativePath: "RJ01554677", workCode: "RJ01554677", inLibrary: { installationId: "x", sameFiles: true, workCode: "RJ01554677" } }, "move"),
      found("f", { relativePath: "Bundle", state: "choose", workCode: null, candidates: [{ workCode: "RJ02000001", confidence: "possible" }, { workCode: "RJ02000002", confidence: "possible" }] }, "move"),
    ],
    jobs: [],
  };
}

function found(key: string, patch: Partial<ScanFinding>, placement: LibraryAddFinding["placement"]): LibraryAddFinding {
  return {
    sourceId: "s1",
    placement,
    finding: {
      key,
      relativePath: "",
      kind: "folder",
      state: "ready",
      workCode: null,
      candidates: [],
      fileCount: 10,
      sizeBytes: 2048,
      volumeCount: 0,
      incompleteSet: null,
      inLibrary: null,
      ignored: false,
      ...patch,
    },
  };
}

function work(code: string): CatalogWork {
  return {
    code,
    title: `Title ${code}`,
    titleEnglish: "",
    circles: [{ name: "Circle", nameEnglish: "" }],
    mainImageUrls: [],
    thumbnailUrls: [],
    latestUpdate: code === "RJ01234567" ? { date: "2024-07-10", version: "1.1" } : null,
  } as unknown as CatalogWork;
}
