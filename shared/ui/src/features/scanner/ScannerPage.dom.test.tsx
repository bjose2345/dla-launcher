// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import type { CatalogWork } from "../catalog/types";
import type { LibraryGateway } from "../library/types";
import { ScannerPage } from "./ScannerPage";
import type {
  ScanAddResponse,
  ScanFinding,
  ScanInstallView,
  ScanSessionView,
  ScannerGateway,
} from "./types";

afterEach(() => {
  cleanup();
});

describe("ScannerPage", () => {
  it("lists one row per work, grouped by what adding it needs", async () => {
    renderScanner(scannerGateway({
      findings: [
        finding({ key: "package:entry:1", relativePath: "RJ01000001.zip", kind: "archive", workCode: "RJ01000001" }),
        finding({
          key: "directory:kanojo",
          relativePath: "kanojo",
          state: "choose",
          workCode: null,
          candidates: [
            { workCode: "RJ01000003", confidence: "strong" },
            { workCode: "RJ01000004", confidence: "strong" },
          ],
        }),
        finding({ key: "directory:old", relativePath: "old_backup", state: "unknown", workCode: null, candidates: [] }),
        finding({
          key: "directory:done",
          relativePath: "RJ01000005",
          workCode: "RJ01000005",
          inLibrary: { installationId: "installation-5", sameFiles: true, workCode: "RJ01000005" },
        }),
      ],
    }));

    const ready = (await screen.findByRole("heading", { name: "Ready to add" })).closest("section")!;
    expect(await within(ready).findByText("Elf Training")).toBeTruthy();
    expect(within(ready).getByRole("button", { name: "Install" })).toBeTruthy();
    expect(within(ready).getByText("Exact match")).toBeTruthy();

    const choose = screen.getByRole("heading", { name: "Choose the right work" }).closest("section")!;
    expect(within(choose).getByRole("radio", { name: /^Her Secret ?RJ01000003/ })).toBeTruthy();
    expect(within(choose).getByRole("radio", { name: /English/ })).toBeTruthy();

    const unknown = screen.getByRole("heading", { name: "Not recognised" }).closest("section")!;
    expect(within(unknown).getByText("old_backup", { selector: "strong" })).toBeTruthy();
    expect(within(unknown).getByRole("button", { name: "Add as an unknown work" })).toBeTruthy();

    const library = screen.getByRole("heading", { name: "Already in your Library" }).closest("details")!;
    expect(library.open).toBe(true);
    expect(within(library).getByText("Voice Drama")).toBeTruthy();
  });

  it("says which part of an incomplete archive set to rename or find", async () => {
    renderScanner(scannerGateway({
      findings: [
        finding({
          key: "package:entry:9",
          relativePath: "RJ01000009.part1.exe",
          kind: "archive",
          state: "incomplete",
          workCode: "RJ01000009",
          volumeCount: 2,
          incompleteSet: {
            missingParts: [3],
            renames: [{ from: "RJ01000009part2.rar", to: "RJ01000009.part2.rar" }],
          },
        }),
      ],
    }));

    expect(await screen.findByText("Rename RJ01000009part2.rar to RJ01000009.part2.rar.")).toBeTruthy();
    expect(screen.getByText("Missing: part 3.")).toBeTruthy();
  });

  it("adds a folder in one click, confirms it, and undoes it", async () => {
    const gateway = scannerGateway({
      findings: [finding({ key: "directory:game", relativePath: "KUNOICHI RED", workCode: "RJ01000002" })],
      add: addResponse([{ key: "directory:game", outcome: "added", installationId: "installation-2" }]),
    });
    const library = libraryGateway();
    const onShowInLibrary = vi.fn();
    renderScanner(gateway, { library, onShowInLibrary });

    await screen.findByText("Kunoichi Red");
    fireEvent.click(screen.getByRole("button", { name: "Add" }));

    await waitFor(() => expect(gateway.addFindings).toHaveBeenCalledWith("session-1", [{ key: "directory:game" }]));
    expect(await screen.findByText("Kunoichi Red was added to your Library")).toBeTruthy();
    expect(screen.getByText("In your Library")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Open in Library" }));
    expect(onShowInLibrary).toHaveBeenCalledWith("installation-2");

    fireEvent.click(screen.getAllByRole("button", { name: "Undo" })[0]!);
    await waitFor(() => expect(library.removeInstallation).toHaveBeenCalledWith("installation-2"));
    expect(await screen.findByRole("button", { name: "Add" })).toBeTruthy();
  });

  it("asks for an install folder once before installing an archive", async () => {
    const gateway = scannerGateway({
      findings: [finding({ key: "package:entry:1", relativePath: "RJ01000001.zip", kind: "archive", workCode: "RJ01000001" })],
      installFolder: null,
      add: addResponse([{ key: "package:entry:1", outcome: "installing", installationId: "installation-1" }], {
        installs: [{
          installationId: "installation-1",
          key: "package:entry:1",
          name: "RJ01000001.zip",
          workCode: "RJ01000001",
          status: "queued",
          detail: null,
        }],
      }),
    });
    renderScanner(gateway);

    expect(await screen.findByText("Archives need an install folder.")).toBeTruthy();
    await screen.findByText("Elf Training");
    fireEvent.click(screen.getByRole("button", { name: "Install" }));

    await waitFor(() => expect(gateway.selectInstallFolder).toHaveBeenCalledOnce());
    await waitFor(() => expect(gateway.addFindings).toHaveBeenCalledWith("session-1", [{ key: "package:entry:1" }]));
    expect(await screen.findByText("Waiting to install")).toBeTruthy();
    expect(screen.getByText("Elf Training is installing")).toBeTruthy();
  });

  it("needs a pick before it adds a finding with several possible works", async () => {
    const gateway = scannerGateway({
      findings: [finding({
        key: "directory:kanojo",
        relativePath: "kanojo",
        state: "choose",
        workCode: null,
        candidates: [
          { workCode: "RJ01000003", confidence: "strong" },
          { workCode: "RJ01000004", confidence: "strong" },
        ],
      })],
      add: addResponse([{ key: "directory:kanojo", outcome: "added", installationId: "installation-4" }]),
    });
    renderScanner(gateway);

    const section = (await screen.findByRole("heading", { name: "Choose the right work" })).closest("section")!;
    const english = await within(section).findByRole("radio", { name: /English/ });
    const add = within(section).getByRole("button", { name: "Add" }) as HTMLButtonElement;
    expect(add.disabled).toBe(true);

    fireEvent.click(english);
    expect(add.disabled).toBe(false);
    fireEvent.click(add);

    await waitFor(() => expect(gateway.addFindings).toHaveBeenCalledWith("session-1", [
      { key: "directory:kanojo", workCode: "RJ01000004" },
    ]));
  });

  it("adds an unrecognised folder as an unknown work, or ignores it", async () => {
    const gateway = scannerGateway({
      findings: [finding({ key: "directory:old", relativePath: "old_backup", state: "unknown", workCode: null, candidates: [] })],
      add: addResponse([{ key: "directory:old", outcome: "added", installationId: "installation-9" }]),
    });
    renderScanner(gateway);

    fireEvent.click(await screen.findByRole("button", { name: "Ignore" }));
    await waitFor(() => expect(gateway.setFindingIgnored).toHaveBeenCalledWith("root-1", "directory:old", true));

    fireEvent.click(screen.getByRole("button", { name: "Add as an unknown work" }));
    await waitFor(() => expect(gateway.addFindings).toHaveBeenCalledWith("session-1", [
      { key: "directory:old", unknown: true },
    ]));
  });

  it("adds every ready work at once", async () => {
    const gateway = scannerGateway({
      findings: [
        finding({ key: "directory:a", relativePath: "KUNOICHI RED", workCode: "RJ01000002" }),
        finding({ key: "directory:b", relativePath: "RJ01000005", workCode: "RJ01000005" }),
      ],
      add: addResponse([
        { key: "directory:a", outcome: "added", installationId: "installation-a" },
        { key: "directory:b", outcome: "added", installationId: "installation-b" },
      ]),
    });
    renderScanner(gateway);

    fireEvent.click(await screen.findByRole("button", { name: "Add all 2" }));

    await waitFor(() => expect(gateway.addFindings).toHaveBeenCalledWith("session-1", [
      { key: "directory:a" },
      { key: "directory:b" },
    ]));
    expect(await screen.findByText("2 works were added to your Library")).toBeTruthy();
  });

  it("offers editing, showing and removing a work already in the Library from its menu", async () => {
    const onShowInLibrary = vi.fn();
    renderScanner(scannerGateway({
      findings: [finding({
        key: "directory:done",
        relativePath: "RJ01000005",
        workCode: "RJ01000005",
        inLibrary: { installationId: "installation-5", sameFiles: true, workCode: "RJ01000005" },
      })],
    }), { onShowInLibrary });

    const library = (await screen.findByRole("heading", { name: "Already in your Library" })).closest("details")!;
    expect(within(library).queryByRole("button", { name: "Open" })).toBeNull();
    const more = await within(library).findByRole("button", { name: "More for Voice Drama" });

    fireEvent.click(more);
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Edit details…",
      "Show in Library",
      "Remove from Library…",
    ]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Show in Library" }));
    expect(onShowInLibrary).toHaveBeenCalledWith("installation-5");

    fireEvent.click(more);
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit details…" }));
    expect(await screen.findByRole("dialog", { name: "Voice Drama" })).toBeTruthy();
    expect(screen.getByRole("tab", { name: "General", selected: true })).toBeTruthy();
  });

  it("shows how far a running scan has got", async () => {
    renderScanner(scannerGateway({ status: "running" }));

    expect(await screen.findByText((_, element) => element?.tagName === "STRONG" && element.textContent === "Scanning /home/developer/My Works")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Stop scan" })).toBeTruthy();
  });
});

const works: Record<string, CatalogWork> = {
  RJ01000001: work("RJ01000001", "Elf Training"),
  RJ01000002: work("RJ01000002", "Kunoichi Red"),
  RJ01000003: work("RJ01000003", "Her Secret"),
  RJ01000004: work("RJ01000004", "Her Secret (English)"),
  RJ01000005: work("RJ01000005", "Voice Drama"),
};

function renderScanner(
  gateway: ScannerGateway,
  options: { library?: LibraryGateway; onShowInLibrary?: (id: string) => void } = {},
) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <ScannerPage
          gateway={gateway}
          libraryGateway={options.library ?? libraryGateway()}
          catalogGateway={{
            readWorks: vi.fn(async (codes: string[]) => codes.flatMap((code) => works[code] ? [works[code]] : [])),
          }}
          onShowInLibrary={options.onShowInLibrary ?? (() => undefined)}
          onOpenLibrary={() => undefined}
        />
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function libraryGateway(): LibraryGateway {
  return {
    removeInstallation: vi.fn().mockResolvedValue(undefined),
    uninstallInstallation: vi.fn().mockResolvedValue(undefined),
    readPackagePreparationProgress: vi.fn().mockResolvedValue(null),
    subscribePackagePreparationProgress: vi.fn().mockResolvedValue(() => undefined),
    readInstallation: vi.fn().mockResolvedValue(libraryInstallation()),
    readInstallationHealth: vi.fn().mockResolvedValue(null),
    readPreparedPackage: vi.fn().mockResolvedValue(null),
  } as unknown as LibraryGateway;
}

function libraryInstallation() {
  return {
    id: "installation-5",
    scanRootId: "root-1",
    rootPath: "/home/developer/My Works",
    platform: "linux",
    status: "ready",
    detection: {
      sourceScanSessionId: "session-1",
      catalogIdentity: { workCode: "RJ01000005", confidence: "strong", reasonCodes: ["code_in_directory_name"] },
      suggestedStatus: "ready",
      contentItems: [],
      launchCandidates: [],
      packageInspection: null,
    },
    overrides: { catalogIdentity: null, customTitle: null, preferredAction: null, contentItems: [], reviewedAt: "2026-10-08T00:00:00Z" },
    discoveredAt: "2026-10-08T00:00:00Z",
    updatedAt: "2026-10-08T00:00:00Z",
  };
}

function scannerGateway(options: {
  findings?: ScanFinding[];
  status?: ScanSessionView["session"]["status"];
  installFolder?: string | null;
  add?: ScanAddResponse;
} = {}): ScannerGateway {
  const installs = installView({
    installFolder: options.installFolder === undefined ? "/home/developer/DLA Library" : options.installFolder,
  });
  return {
    readRootPreference: vi.fn().mockResolvedValue({
      platform: "linux",
      displayPath: "/home/developer/My Works",
      source: "platform_default",
      available: true,
      canPrepare: true,
    }),
    selectPreferredRoot: vi.fn().mockResolvedValue(null),
    resetPreferredRoot: vi.fn(),
    preparePreferredRoot: vi.fn().mockResolvedValue({ accessHandle: "root-1", displayPath: "/home/developer/My Works" }),
    selectRoot: vi.fn().mockResolvedValue(null),
    start: vi.fn(),
    cancel: vi.fn().mockResolvedValue(true),
    readLatest: vi.fn().mockResolvedValue(sessionView(options.status ?? "completed")),
    listFindings: vi.fn().mockResolvedValue({
      sessionId: "session-1",
      rootId: "root-1",
      rootPath: "/home/developer/My Works",
      findings: options.findings ?? [],
    }),
    setFindingIgnored: vi.fn().mockResolvedValue(undefined),
    addFindings: vi.fn().mockResolvedValue(options.add ?? addResponse([])),
    browseIssues: vi.fn().mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 }),
    subscribeProgress: vi.fn().mockResolvedValue(() => undefined),
    readInstalls: vi.fn().mockResolvedValue(installs),
    subscribeInstalls: vi.fn().mockResolvedValue(() => undefined),
    selectInstallFolder: vi.fn().mockResolvedValue(
      installView({ installFolder: "/home/developer/DLA Library", revision: 5 }),
    ),
    installArchive: vi.fn(),
    cancelInstall: vi.fn().mockResolvedValue(installs),
  };
}

function installView(overrides: Partial<ScanInstallView> = {}): ScanInstallView {
  return { revision: 1, installFolder: null, installs: [], ...overrides };
}

function addResponse(
  results: Array<{ key: string; outcome: ScanAddResponse["results"][number]["outcome"]; installationId: string }>,
  installs: Partial<ScanInstallView> = {},
): ScanAddResponse {
  return {
    results: results.map((result) => ({ ...result, refusal: null, detail: null })),
    installs: installView({ installFolder: "/home/developer/DLA Library", revision: 9, ...installs }),
  };
}

function finding(overrides: Partial<ScanFinding>): ScanFinding {
  const workCode = overrides.workCode === undefined ? "RJ01000001" : overrides.workCode;
  return {
    key: "directory:work",
    relativePath: "work",
    kind: "folder",
    state: "ready",
    workCode,
    candidates: workCode ? [{ workCode, confidence: "exact" }] : [],
    fileCount: 3,
    sizeBytes: 300,
    volumeCount: overrides.kind === "archive" ? 1 : 0,
    incompleteSet: null,
    inLibrary: null,
    ignored: false,
    ...overrides,
  };
}

function sessionView(status: ScanSessionView["session"]["status"]): ScanSessionView {
  return {
    root: {
      id: "root-1",
      platform: "linux",
      pathKey: "/home/developer/My Works",
      displayPath: "/home/developer/My Works",
      createdAt: "2026-10-08T09:00:00Z",
      updatedAt: "2026-10-08T09:01:00Z",
    },
    session: {
      id: "session-1",
      rootId: "root-1",
      status,
      options: { followSymlinks: false, hashPolicy: "candidate_archives", workerLimit: 4 },
      counters: {
        discoveredFiles: 12,
        discoveredDirectories: 4,
        inspectedFiles: 12,
        matched: 4,
        ambiguous: 1,
        unmatched: 7,
        recoverableErrors: 0,
      },
      startedAt: "2026-10-08T09:00:00Z",
      finishedAt: status === "running" ? null : "2026-10-08T09:01:00Z",
      fatalErrorCode: null,
      fatalErrorMessage: null,
    },
  };
}

function work(code: string, title: string): CatalogWork {
  return {
    code,
    sourceCode: code,
    title,
    titleEnglish: "",
    addedDate: "2026-10-01",
    releaseDate: "2026-10-01",
    updatedDate: "2026-10-01",
    ageRating: "all_ages",
    releaseType: "game",
    mainImageUrls: [],
    thumbnailUrls: [],
    circles: [],
    categories: [],
    tags: [],
    synthetic: true,
  };
}
