// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { ScannerPage, distinctEvidenceLabels } from "./ScannerPage";
import type {
  ScanCounters,
  ScanEvidence,
  ScanIssuePage,
  ScanResultItem,
  ScanResultPage,
  ScanSessionView,
  ScannerGateway,
} from "./types";

afterEach(() => {
  cleanup();
});

describe("distinctEvidenceLabels", () => {
  it("collapses raw codes that render the same label", () => {
    const labels = distinctEvidenceLabels([
      evidence("code_in_directory_name"),
      evidence("code_in_filename"),
      evidence("code_in_path"),
    ]);

    expect(labels.map((label) => label.key)).toEqual(["domain.evidence.codeInName"]);
  });

  it("puts an archive hash match first and marks it as the strongest signal", () => {
    const labels = distinctEvidenceLabels([
      evidence("code_in_filename"),
      evidence("archive_sha256_match"),
      evidence("audio_extension"),
    ]);

    expect(labels[0]).toEqual({ key: "domain.evidence.archiveHashMatch", strongest: true });
    expect(labels.filter((label) => label.strongest)).toHaveLength(1);
  });
});

describe("ScannerPage", () => {
  it("shows one evidence chip per distinct label", async () => {
    const gateway = scannerGateway();
    gateway.browseResults = vi.fn().mockResolvedValue(resultPage([
      resultItem({
        evidence: [
          evidence("code_in_directory_name"),
          evidence("code_in_filename"),
          evidence("archive_sha256_match"),
        ],
      }),
    ]));
    renderScanner(gateway);

    const row = (await screen.findByText("RJ01678999")).closest(".scanner-result");
    if (!row) throw new Error("result row was not rendered");
    const chips = within(row as HTMLElement).getAllByText(/Code found in a name or path|Archive hash match/);
    expect(chips).toHaveLength(2);
  });

  it("counts inspected files without presenting unmatched files as works", async () => {
    const gateway = scannerGateway();
    gateway.readLatest = vi.fn().mockResolvedValue(sessionView({
      matched: 0,
      unmatched: 4,
    }));
    renderScanner(gateway);

    const caption = await screen.findByText("files inspected in this folder");
    const figure = caption.closest(".scanner-figure");
    if (!figure) throw new Error("headline figure was not rendered");
    expect(within(figure as HTMLElement).getByText("4")).toBeTruthy();
    expect(screen.getByText(/Inspected 4 files in 3 folders/)).toBeTruthy();
    expect(screen.getByText(/no scan issues/)).toBeTruthy();
    expect(screen.getByText(/No catalog matches were found/)).toBeTruthy();
    expect(screen.getByText(/Check you picked the folder/)).toBeTruthy();
    expect(document.body.textContent).not.toContain("works found");
    expect(document.body.textContent).not.toContain("Recoverable errors");
  });

  it("labels the preferred-root action instead of promising to rescan the selected folder", async () => {
    const gateway = scannerGateway();
    renderScanner(gateway);

    fireEvent.click(await screen.findByRole("button", { name: "Scan My Works" }));

    await waitFor(() => {
      expect(gateway.preparePreferredRoot).toHaveBeenCalledOnce();
      expect(gateway.start).toHaveBeenCalledWith("root-1");
    });
    expect(screen.queryByRole("button", { name: "Scan again" })).toBeNull();
  });

  it("filters to matched from the summary action", async () => {
    const gateway = scannerGateway();
    renderScanner(gateway);

    fireEvent.click(await screen.findByRole("button", { name: "Show 4 matched" }));

    await waitFor(() => {
      expect(gateway.browseResults).toHaveBeenCalledWith(
        expect.objectContaining({ outcome: "matched" }),
      );
    });
  });

  it("moves recoverable issues behind a tab", async () => {
    const gateway = scannerGateway();
    gateway.readLatest = vi.fn().mockResolvedValue(sessionView({ recoverableErrors: 1 }));
    gateway.browseIssues = vi.fn().mockResolvedValue({
      items: [{
        id: "issue-1",
        sessionId: "session-1",
        entryId: null,
        relativePath: "Doujin/broken/RJ01120044.zip",
        code: "archive_extension",
        message: "zip: central directory not found",
        recoverable: true,
        createdAt: "2026-08-19T19:33:00Z",
      }],
      total: 1,
      limit: 30,
      offset: 0,
    } satisfies ScanIssuePage);
    renderScanner(gateway);

    const tab = await screen.findByRole("tab", { name: /Issues/ });
    expect(screen.getByText(/Scan issues: 1/)).toBeTruthy();

    fireEvent.click(tab);

    expect(await screen.findByText("Doujin/broken/RJ01120044.zip")).toBeTruthy();
    expect(screen.getByText(/zip: central directory not found/)).toBeTruthy();
    expect(screen.getByText(/Recorded scan issues: 1/)).toBeTruthy();
  });

  it("uses the durable issue counter while issue details load and reports query failures", async () => {
    const gateway = scannerGateway();
    gateway.readLatest = vi.fn().mockResolvedValue(sessionView({ recoverableErrors: 2 }));
    let rejectIssues!: (reason?: unknown) => void;
    gateway.browseIssues = vi.fn().mockReturnValue(new Promise<ScanIssuePage>((_resolve, reject) => {
      rejectIssues = reject;
    }));
    renderScanner(gateway);

    expect(await screen.findByText(/Scan issues: 2/)).toBeTruthy();
    fireEvent.click(screen.getByRole("tab", { name: /Issues 2/ }));
    expect(await screen.findByText("Loading scan issues…")).toBeTruthy();

    rejectIssues(new Error("issue read failed"));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("issue read failed");
    expect(screen.getByRole("tab", { name: /Issues 2/ })).toBeTruthy();
  });

  it("loads every page of recoverable issues", async () => {
    const gateway = scannerGateway();
    gateway.readLatest = vi.fn().mockResolvedValue(sessionView({ recoverableErrors: 2 }));
    gateway.browseIssues = vi.fn()
      .mockResolvedValueOnce(issuePage("issue-1", "first.zip", 0, 2))
      .mockResolvedValueOnce(issuePage("issue-2", "second.zip", 1, 2));
    renderScanner(gateway);

    fireEvent.click(await screen.findByRole("tab", { name: /Issues 2/ }));
    expect(await screen.findByText("first.zip")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Show more issues" }));

    expect(await screen.findByText("second.zip")).toBeTruthy();
    expect(gateway.browseIssues).toHaveBeenLastCalledWith({
      sessionId: "session-1",
      limit: 30,
      offset: 1,
    });
  });

  it("renders ranked candidates for an ambiguous result", async () => {
    const gateway = scannerGateway();
    gateway.browseResults = vi.fn().mockResolvedValue(resultPage([
      resultItem({
        outcome: "ambiguous",
        selectedWorkCode: null,
        confidence: "strong",
        candidateEntryId: null,
        candidates: [
          { workCode: "RJ01326398", confidence: "strong", reasonCodes: ["code_in_path"], rank: 1 },
          { workCode: "RJ01326400", confidence: "possible", reasonCodes: ["code_in_filename"], rank: 2 },
        ],
      }),
    ]));
    renderScanner(gateway);

    expect(await screen.findByText("Possible matches")).toBeTruthy();
    expect(screen.getByText("RJ01326400")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Review" })).toBeNull();
  });
});

function renderScanner(gateway: ScannerGateway) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <ScannerPage gateway={gateway} />
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function scannerGateway(): ScannerGateway {
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
    readLatest: vi.fn().mockResolvedValue(sessionView()),
    browseResults: vi.fn().mockResolvedValue(resultPage([resultItem()])),
    browseIssues: vi.fn().mockResolvedValue({ items: [], total: 0, limit: 30, offset: 0 }),
    createInstallation: vi.fn().mockResolvedValue({ id: "installation-1" }),
    subscribeProgress: vi.fn().mockResolvedValue(() => undefined),
  };
}

function sessionView(counterOverrides: Partial<ScanCounters> = {}): ScanSessionView {
  return {
    root: {
      id: "root-1",
      platform: "linux",
      pathKey: "/home/developer/My Works",
      displayPath: "/home/developer/My Works",
      createdAt: "2026-08-19T19:32:00Z",
      updatedAt: "2026-08-19T19:33:00Z",
    },
    session: {
      id: "session-1",
      rootId: "root-1",
      status: "completed",
      options: { followSymlinks: false, hashPolicy: "candidate_archives", workerLimit: 4 },
      counters: counters(counterOverrides),
      startedAt: "2026-08-19T19:32:17Z",
      finishedAt: "2026-08-19T19:33:29Z",
      fatalErrorCode: null,
      fatalErrorMessage: null,
    },
  };
}

function counters(overrides: Partial<ScanCounters> = {}): ScanCounters {
  return {
    discoveredFiles: 4,
    discoveredDirectories: 3,
    inspectedFiles: 4,
    matched: 4,
    ambiguous: 0,
    unmatched: 0,
    recoverableErrors: 0,
    ...overrides,
  };
}

function issuePage(id: string, relativePath: string, offset: number, total: number): ScanIssuePage {
  return {
    items: [{
      id,
      sessionId: "session-1",
      entryId: null,
      relativePath,
      code: "archive_extension",
      message: "could not inspect entry",
      recoverable: true,
      createdAt: "2026-08-19T19:33:00Z",
    }],
    total,
    limit: 30,
    offset,
  };
}

function resultPage(items: ScanResultItem[]): ScanResultPage {
  return { items, total: items.length, limit: 60, offset: 0 };
}

function resultItem(overrides: Partial<ScanResultItem["result"]> = {}): ScanResultItem {
  return {
    relativePath: "RJ01678999.zip",
    result: {
      id: "result-1",
      sessionId: "session-1",
      candidateEntryId: "entry-1",
      outcome: "matched",
      selectedWorkCode: "RJ01678999",
      confidence: "exact",
      candidates: [],
      evidence: [evidence("archive_sha256_match")],
      createdAt: "2026-08-19T19:33:00Z",
      updatedAt: "2026-08-19T19:33:00Z",
      ...overrides,
    },
  };
}

function evidence(reasonCode: string): ScanEvidence {
  return {
    id: `evidence-${reasonCode}`,
    resultId: "result-1",
    sourceEntryId: null,
    kind: "observed",
    normalizedValue: "",
    reasonCode,
    createdAt: "2026-08-19T19:33:00Z",
  };
}
