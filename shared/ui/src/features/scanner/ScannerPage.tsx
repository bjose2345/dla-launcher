import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  CheckCircle2,
  FolderCog,
  FolderOpen,
  FolderSearch2,
  LoaderCircle,
  OctagonAlert,
  Square,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { Notice, type NoticeAction } from "../../app/Notice";
import { PathText } from "../../app/PathText";
import { displayPath } from "../../displayPath";
import { evidenceReasonMessageKey } from "../../i18n/domainLabels";
import type { MessageKey } from "../../i18n/catalogs";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { CatalogDetailGateway, CatalogWork } from "../catalog/types";
import { LibraryManageDrawer, type ManageTab } from "../library/LibraryManageDrawer";
import type { LibraryGateway } from "../library/types";
import { usePackagePreparationProgress } from "../library/usePackagePreparationProgress";
import type { SearchGateway } from "../search/types";
import { ScanFindingRow, workTitle, type RowOutcome } from "./ScanFindingRow";
import {
  findingName,
  findingWorkCodes,
  groupFindings,
  type FindingSection,
} from "./scanFindings";
import type {
  AddScanFinding,
  ScanAddResult,
  ScanFinding,
  ScanProgress,
  ScanSessionView,
  ScannerGateway,
} from "./types";
import { scanInstallIsActive, scannerRootPreferenceKey } from "./types";
import { acceptScanInstallView, useScanInstalls } from "./useScanInstalls";

const latestKey = ["scanner", "latest"] as const;
const terminalStatuses = new Set(["completed", "cancelled", "interrupted", "failed"]);

interface ScannerNotice {
  tone: "success" | "progress" | "problem";
  title: string;
  detail?: string;
  actions: NoticeAction[];
  /** An install this notice reports; it closes once the install settles. */
  installationId?: string;
}

/**
 * Scan a folder, then add what it holds in one step per work. Folders join
 * the Library at once; archives install in the background. Setup happens in
 * the Library afterwards.
 */
export function ScannerPage({
  gateway,
  libraryGateway,
  catalogGateway,
  searchGateway,
  onShowInLibrary,
  onOpenLibrary,
  onOpenWork,
}: {
  gateway: ScannerGateway;
  libraryGateway: LibraryGateway;
  catalogGateway: Pick<CatalogDetailGateway, "readWorks">;
  searchGateway?: Pick<SearchGateway, "search">;
  /** Opens the Library with this work in its featured spot. */
  onShowInLibrary: (installationId: string) => void;
  onOpenLibrary: () => void;
  onOpenWork?: (code: string) => void;
}) {
  const { locale, t } = usePresentation();
  const queryClient = useQueryClient();
  const preferEnglish = locale !== "ja-JP";
  const [currentPath, setCurrentPath] = useState<string | null>(null);
  const [outcomes, setOutcomes] = useState<Record<string, RowOutcome>>({});
  const [notice, setNotice] = useState<ScannerNotice | null>(null);
  const [managing, setManaging] = useState<{ installationId: string; tab: ManageTab } | null>(null);

  const latest = useQuery({
    queryKey: latestKey,
    queryFn: () => gateway.readLatest(),
    refetchInterval: (query) => isActive(query.state.data?.session.status) ? 750 : false,
  });
  const rootPreference = useQuery({
    queryKey: scannerRootPreferenceKey,
    queryFn: () => gateway.readRootPreference(),
  });
  const view = latest.data;
  const sessionId = view?.session.id;
  const completed = view?.session.status === "completed";
  const findings = useQuery({
    queryKey: ["scanner", "findings", sessionId],
    queryFn: () => gateway.listFindings(sessionId!),
    enabled: Boolean(sessionId) && completed,
  });
  const issueCount = view?.session.counters.recoverableErrors ?? 0;
  const issues = useQuery({
    queryKey: ["scanner", "issues", sessionId],
    queryFn: () => gateway.browseIssues({ sessionId: sessionId!, limit: 50, offset: 0 }),
    enabled: Boolean(sessionId) && !isActive(view?.session.status) && issueCount > 0,
  });
  const installs = useScanInstalls(gateway);
  const progress = usePackagePreparationProgress(libraryGateway);
  const list = findings.data?.findings ?? [];
  const codes = useMemo(() => [...new Set([
    ...findingWorkCodes(list),
    ...Object.values(outcomes).flatMap((outcome) => (
      outcome.kind !== "refused" && outcome.workCode ? [outcome.workCode] : []
    )),
  ])].sort(), [list, outcomes]);
  const works = useQuery({
    queryKey: ["catalog-works", codes],
    queryFn: () => catalogGateway.readWorks(codes),
    enabled: codes.length > 0,
    staleTime: 5 * 60_000,
  });
  const workByCode = useMemo(
    () => new Map<string, CatalogWork>((works.data ?? []).map((work) => [work.code, work])),
    [works.data],
  );

  useEffect(() => {
    setOutcomes({});
  }, [sessionId]);

  useEffect(() => {
    if (!notice?.installationId) return;
    const install = installs.data?.installs.find((item) => item.installationId === notice.installationId);
    if (install && !scanInstallIsActive(install.status)) setNotice(null);
  }, [installs.data, notice]);

  const acceptStartedScan = async (started: ScanSessionView | null) => {
    if (!started) return;
    setCurrentPath(null);
    setNotice(null);
    queryClient.setQueryData(latestKey, started);
    await queryClient.invalidateQueries({ queryKey: latestKey });
  };
  const startPreferred = useMutation({
    mutationFn: async () => {
      const selected = await gateway.preparePreferredRoot();
      return gateway.start(selected.accessHandle);
    },
    onSuccess: acceptStartedScan,
  });
  const startSelected = useMutation({
    mutationFn: async () => {
      const selected = await gateway.selectRoot();
      if (!selected) return null;
      return gateway.start(selected.accessHandle);
    },
    onSuccess: acceptStartedScan,
  });
  const cancel = useMutation({ mutationFn: (id: string) => gateway.cancel(id) });

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let disposed = false;
    void gateway.subscribeProgress((update) => {
      queryClient.setQueryData<ScanSessionView | null>(latestKey, (current) => mergeProgress(current, update));
      setCurrentPath(terminalStatuses.has(update.status) ? null : update.currentRelativePath);
      if (terminalStatuses.has(update.status)) {
        void queryClient.invalidateQueries({ queryKey: latestKey });
      }
    }).then((listener) => {
      if (disposed) listener();
      else unsubscribe = listener;
    });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [gateway, queryClient]);

  const refreshLibrary = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["library"] }),
      queryClient.invalidateQueries({ queryKey: ["scanner", "findings", sessionId] }),
    ]);
  };

  // A work removed from the Library in the panel can be added here again.
  const closeManaging = async () => {
    const closed = managing;
    setManaging(null);
    if (!closed || !sessionId) return;
    const fresh = await queryClient.fetchQuery({
      queryKey: ["scanner", "findings", sessionId],
      queryFn: () => gateway.listFindings(sessionId),
    });
    setOutcomes((current) => {
      const next = { ...current };
      for (const [key, outcome] of Object.entries(current)) {
        if (outcome.kind === "refused" || outcome.installationId !== closed.installationId) continue;
        if (!fresh.findings.find((finding) => finding.key === key)?.inLibrary) delete next[key];
      }
      return next;
    });
  };

  const titleOf = (finding: ScanFinding | undefined, pickedCode?: string | null) => {
    if (!finding) return "";
    const code = pickedCode ?? finding.workCode;
    const work = code ? workByCode.get(code) : undefined;
    return work ? workTitle(work, preferEnglish) : findingName(finding, findings.data?.rootPath ?? "");
  };

  const ensureInstallFolder = async (items: AddScanFinding[]) => {
    const archives = items.some((item) => list.find((finding) => finding.key === item.key)?.kind === "archive");
    if (!archives || installs.data?.installFolder) return true;
    const chosen = await gateway.selectInstallFolder();
    if (chosen) acceptScanInstallView(queryClient, chosen);
    return Boolean(chosen?.installFolder);
  };

  const add = useMutation({
    mutationFn: async (items: AddScanFinding[]) => {
      const ready = await ensureInstallFolder(items);
      const allowed = ready
        ? items
        : items.filter((item) => list.find((finding) => finding.key === item.key)?.kind !== "archive");
      if (!allowed.length) return { results: [] as ScanAddResult[], skipped: items.length };
      const response = await gateway.addFindings(sessionId!, allowed);
      acceptScanInstallView(queryClient, response.installs);
      return { results: response.results, skipped: items.length - allowed.length };
    },
    onSuccess: async ({ results, skipped }, items) => {
      const picked = (key: string) => items.find((item) => item.key === key)?.workCode ?? null;
      setOutcomes((current) => {
        const next = { ...current };
        for (const result of results) {
          if (result.outcome === "added" && result.installationId) {
            next[result.key] = { kind: "added", installationId: result.installationId, workCode: picked(result.key) };
          } else if (result.outcome === "installing" && result.installationId) {
            next[result.key] = {
              kind: "installing",
              installationId: result.installationId,
              workCode: picked(result.key),
            };
          } else if (result.outcome === "refused") {
            next[result.key] = { kind: "refused", refusal: result.refusal ?? "failed" };
          } else {
            delete next[result.key];
          }
        }
        return next;
      });
      setNotice(addNotice(results, skipped, items));
      await refreshLibrary();
    },
  });

  const undo = useMutation({
    mutationFn: async (keys: string[]) => {
      for (const key of keys) {
        const outcome = outcomes[key];
        if (!outcome || outcome.kind === "refused") continue;
        const install = installs.data?.installs.find((item) => item.installationId === outcome.installationId);
        if (outcome.kind === "installing" && install && scanInstallIsActive(install.status)) {
          acceptScanInstallView(queryClient, await gateway.cancelInstall(outcome.installationId));
        } else if (outcome.kind === "installing" && (install?.status === "installed" || install?.status === "needs_review")) {
          await libraryGateway.uninstallInstallation(outcome.installationId);
        } else if (install?.status !== "cancelled") {
          await libraryGateway.removeInstallation(outcome.installationId);
        }
      }
      return keys;
    },
    onSuccess: async (keys) => {
      setOutcomes((current) => {
        const next = { ...current };
        for (const key of keys) delete next[key];
        return next;
      });
      setNotice(null);
      await refreshLibrary();
    },
  });

  const ignore = useMutation({
    mutationFn: ({ key, ignored }: { key: string; ignored: boolean }) =>
      gateway.setFindingIgnored(findings.data!.rootId, key, ignored),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["scanner", "findings", sessionId] });
    },
  });

  const addNotice = (results: ScanAddResult[], skipped: number, items: AddScanFinding[]): ScannerNotice | null => {
    const finding = (key: string) => list.find((item) => item.key === key);
    const picked = (key: string) => items.find((item) => item.key === key)?.workCode ?? null;
    const added = results.filter((result) => result.outcome === "added" || result.outcome === "installing");
    const installing = results.filter((result) => result.outcome === "installing");
    const refused = results.filter((result) => result.outcome === "refused");
    const keys = added.map((result) => result.key);
    const undoAction: NoticeAction = { label: t("scanner.undo"), onClick: () => undo.mutate(keys) };
    if (added.length > 1) {
      return {
        tone: "success",
        title: t("scanner.notice.addedMany", { count: added.length.toLocaleString(locale) }),
        detail: installing.length
          ? t("scanner.notice.installingMany", { count: installing.length.toLocaleString(locale) })
          : undefined,
        actions: [undoAction, { label: t("scanner.notice.openLibrary"), onClick: onOpenLibrary, primary: true }],
      };
    }
    const single = added[0];
    if (single) {
      const title = titleOf(finding(single.key), picked(single.key));
      const id = single.installationId!;
      return single.outcome === "installing"
        ? {
          tone: "progress",
          title: t("scanner.notice.installing", { title }),
          detail: t("scanner.notice.installingHelp"),
          actions: [undoAction],
          installationId: id,
        }
        : {
          tone: "success",
          title: t("scanner.notice.added", { title }),
          detail: t("scanner.notice.addedHelp"),
          actions: [
            undoAction,
            { label: t("scanner.notice.openLibrary"), onClick: () => onShowInLibrary(id), primary: true },
          ],
        };
    }
    const already = results.find((result) => result.outcome === "already_in_library");
    if (already?.installationId) {
      const id = already.installationId;
      return {
        tone: "success",
        title: t("scanner.notice.already", { title: titleOf(finding(already.key)) }),
        actions: [{ label: t("scanner.notice.openLibrary"), onClick: () => onShowInLibrary(id), primary: true }],
      };
    }
    if (refused[0]) {
      return {
        tone: "problem",
        title: t("scanner.notice.refused", { title: titleOf(finding(refused[0].key)) }),
        detail: refused[0].detail ? t("common.technicalDetail", { detail: refused[0].detail }) : undefined,
        actions: [],
      };
    }
    if (skipped) {
      return { tone: "problem", title: t("scanner.notice.needsInstallFolder"), actions: [] };
    }
    return null;
  };

  const active = isActive(view?.session.status);
  const starting = startPreferred.isPending || startSelected.isPending;
  const canScanDefault = !active && !starting && !rootPreference.isPending && Boolean(rootPreference.data?.canPrepare);
  const requestError = startPreferred.error ?? startSelected.error ?? cancel.error ?? add.error ?? undo.error
    ?? ignore.error ?? findings.error ?? latest.error;
  const staying = new Set(Object.keys(outcomes));
  const groups = groupFindings(list, staying);
  const readyToAdd = groups.ready.filter((finding) => !outcomes[finding.key] && !finding.inLibrary);
  const busy = add.isPending || undo.isPending || ignore.isPending;
  const rootPath = findings.data?.rootPath ?? view?.root.displayPath ?? "";
  const archivesFound = list.some((finding) => finding.kind === "archive" && !finding.inLibrary && !finding.ignored);

  const row = (finding: ScanFinding) => {
    const outcome = outcomes[finding.key];
    const installationId = outcome && outcome.kind !== "refused" ? outcome.installationId : null;
    const install = installationId
      ? installs.data?.installs.find((item) => item.installationId === installationId)
      : undefined;
    return (
      <ScanFindingRow
        finding={finding}
        rootPath={rootPath}
        works={workByCode}
        outcome={outcome}
        install={install}
        progress={installationId && progress.data?.installationId === installationId ? progress.data : null}
        busy={busy}
        catalogGateway={catalogGateway}
        searchGateway={searchGateway}
        onAdd={(item) => add.mutate([item])}
        onIgnore={(ignored) => ignore.mutate({ key: finding.key, ignored })}
        onUndo={() => undo.mutate([finding.key])}
        onManage={(installationId, tab) => setManaging({ installationId, tab })}
        onShowInLibrary={onShowInLibrary}
        key={finding.key}
      />
    );
  };

  return (
    <main className="scanner-page">
      <section className="scanner-masthead">
        <div className="scanner-masthead-title">
          <span className="scanner-eyebrow"><FolderSearch2 aria-hidden="true" />{t("scanner.eyebrow")}</span>
          <h1>{t("scanner.heading")}</h1>
        </div>
        <div className="scanner-actions">
          {active && view ? (
            <button
              className="scanner-button scanner-button-danger"
              type="button"
              disabled={cancel.isPending}
              onClick={() => cancel.mutate(view.session.id)}
            >
              {cancel.isPending ? <LoaderCircle className="scanner-spin" aria-hidden="true" /> : <Square aria-hidden="true" />}
              {t("scanner.cancel")}
            </button>
          ) : (
            <>
              <button className="scanner-button" type="button" disabled={starting} onClick={() => startSelected.mutate()}>
                {startSelected.isPending ? <LoaderCircle className="scanner-spin" aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}
                {t("scanner.chooseFolder")}
              </button>
              <button
                className="scanner-button scanner-button-primary"
                type="button"
                disabled={!canScanDefault}
                onClick={() => startPreferred.mutate()}
              >
                {startPreferred.isPending ? <LoaderCircle className="scanner-spin" aria-hidden="true" /> : <FolderCog aria-hidden="true" />}
                {t("scanner.scanDefault")}
              </button>
            </>
          )}
        </div>
      </section>

      <div className="scanner-body">
        {requestError ? (
          <div className="scanner-callout" role="alert">
            <OctagonAlert aria-hidden="true" />
            <span>{t("common.requestFailed", { error: String(requestError) })}</span>
          </div>
        ) : null}

        {!view && latest.isPending ? <ScannerLoading /> : null}

        {!view && !latest.isPending ? (
          <section className="scanner-empty">
            <span className="scanner-empty-mark"><FolderSearch2 aria-hidden="true" /></span>
            <h2>{t("scanner.emptyTitle")}</h2>
            <p>{t("scanner.description")}</p>
            <small>{t("scanner.emptyHelp")}</small>
            {rootPreference.data?.displayPath ? (
              <small><PathText text={(path) => t("scanner.defaultFolderIs", { path })} path={rootPreference.data.displayPath} /></small>
            ) : null}
          </section>
        ) : null}

        {view && active ? (
          <section className="scan-progress" aria-live="polite">
            <LoaderCircle className="scanner-spin" aria-hidden="true" />
            <div>
              <strong><PathText text={(path) => t("scanner.progress.title", { path })} path={view.root.displayPath} /></strong>
              <span>{t("scanner.progress.files", {
                count: view.session.counters.inspectedFiles.toLocaleString(locale),
              })}</span>
              {currentPath ? <code title={currentPath}>{currentPath}</code> : null}
            </div>
            <i className="scan-progress-bar" aria-hidden="true" />
          </section>
        ) : null}

        {view && !active && !completed ? (
          <div className="scanner-callout is-notice" role="status">
            <AlertTriangle aria-hidden="true" />
            <span>
              {t("scanner.stopped")}
              {view.session.fatalErrorMessage
                ? ` ${t("common.technicalDetail", { detail: view.session.fatalErrorMessage })}`
                : ""}
            </span>
          </div>
        ) : null}

        {view && completed ? (
          <>
            <div className="scan-summary">
              <p>
                <CheckCircle2 aria-hidden="true" />
                <span>
                  <strong><code className="app-path">{displayPath(rootPath)}</code></strong>
                  {t("scanner.summary.done", {
                    files: view.session.counters.inspectedFiles.toLocaleString(locale),
                    works: list.length.toLocaleString(locale),
                  })}
                  {issueCount > 0 ? ` · ${t("scanner.summary.issues", { count: issueCount.toLocaleString(locale) })}` : ""}
                </span>
              </p>
              {archivesFound ? (
                <p className="scan-install-folder">
                  <span>
                    {installs.data?.installFolder
                      ? <PathText text={(path) => t("scanner.installFolder", { path })} path={installs.data.installFolder} />
                      : t("scanner.installFolderMissing")}
                  </span>
                  <button
                    className="scan-text-button is-link"
                    type="button"
                    onClick={async () => {
                      const chosen = await gateway.selectInstallFolder();
                      if (chosen) acceptScanInstallView(queryClient, chosen);
                    }}
                  >
                    {t(installs.data?.installFolder ? "scanner.changeInstallFolder" : "scanner.chooseInstallFolder")}
                  </button>
                </p>
              ) : null}
            </div>

            {findings.isPending ? <ScannerLoading /> : null}
            {findings.isSuccess && list.length === 0 ? (
              <p className="scan-nothing">{t("scanner.summary.nothing")}</p>
            ) : null}

            {(["ready", "choose", "unknown", "blocked"] as FindingSection[]).map((section) => (
              groups[section].length ? (
                <section className="scan-section" key={section}>
                  <header>
                    <h2>{t(sectionKey(section))}</h2>
                    <span className={`scan-count is-${section}`}>{groups[section].length.toLocaleString(locale)}</span>
                    {section === "ready" && readyToAdd.length > 1 ? (
                      <button
                        className="scan-button"
                        type="button"
                        disabled={busy}
                        onClick={() => add.mutate(readyToAdd.map((finding) => ({ key: finding.key })))}
                      >
                        {t("scanner.addAll", { count: readyToAdd.length.toLocaleString(locale) })}
                      </button>
                    ) : null}
                  </header>
                  <div className="scan-list">{groups[section].map(row)}</div>
                </section>
              ) : null
            ))}

            {(["library", "ignored"] as FindingSection[]).map((section) => (
              groups[section].length ? (
                <details className="scan-section is-folded" key={section} open={section === "library"}>
                  <summary>
                    <h2>{t(sectionKey(section))}</h2>
                    <span className="scan-count">{groups[section].length.toLocaleString(locale)}</span>
                  </summary>
                  <div className="scan-list">{groups[section].map(row)}</div>
                </details>
              ) : null
            ))}
          </>
        ) : null}

        {view && !active && issues.data?.items.length ? (
          <details className="scan-section is-folded">
            <summary>
              <h2>{t("scanner.section.issues")}</h2>
              <span className="scan-count is-blocked">{issueCount.toLocaleString(locale)}</span>
            </summary>
            <ul className="scan-issues">
              {issues.data.items.map((issue) => (
                <li key={issue.id}>
                  <code>{issue.relativePath ?? displayPath(view.root.displayPath)}</code>
                  <span>{t(evidenceReasonMessageKey(issue.code))}</span>
                </li>
              ))}
            </ul>
          </details>
        ) : null}
      </div>

      {managing ? (
        <LibraryManageDrawer
          installationId={managing.installationId}
          initialTab={managing.tab}
          gateway={libraryGateway}
          catalogGateway={catalogGateway}
          searchGateway={searchGateway}
          installs={gateway}
          onClose={() => void closeManaging()}
          onOpenWork={onOpenWork}
        />
      ) : null}

      {notice ? (
        <Notice
          tone={notice.tone}
          title={notice.title}
          detail={notice.detail}
          actions={notice.actions}
          onClose={() => setNotice(null)}
        />
      ) : null}
    </main>
  );
}

function ScannerLoading() {
  const { t } = usePresentation();
  return (
    <div className="scanner-loading">
      <LoaderCircle className="scanner-spin" aria-hidden="true" /><span>{t("scanner.loading")}</span>
    </div>
  );
}

function sectionKey(section: FindingSection): MessageKey {
  switch (section) {
    case "ready": return "scanner.section.ready";
    case "choose": return "scanner.section.choose";
    case "unknown": return "scanner.section.unknown";
    case "blocked": return "scanner.section.blocked";
    case "library": return "scanner.section.library";
    case "ignored": return "scanner.section.ignored";
  }
}

function isActive(status: string | undefined): boolean {
  return status === "queued" || status === "running";
}

function mergeProgress(current: ScanSessionView | null | undefined, progress: ScanProgress): ScanSessionView | null {
  if (!current || current.session.id !== progress.sessionId) return current ?? null;
  return {
    ...current,
    session: {
      ...current.session,
      status: progress.status,
      counters: progress.counters,
    },
  };
}
