import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Ban,
  CheckCircle2,
  FileQuestion,
  FolderCog,
  FolderOpen,
  FolderSearch2,
  ListChecks,
  LoaderCircle,
  OctagonAlert,
  ShieldCheck,
  Square,
} from "lucide-react";
import { useEffect, useState } from "react";

import {
  confidenceMessageKey,
  evidenceReasonMessageKey,
  platformMessageKey,
} from "../../i18n/domainLabels";
import type { MessageKey } from "../../i18n/catalogs";
import { usePresentation } from "../../preferences/PresentationProvider";
import { formatDuration } from "../library/LaunchHistory";
import type {
  ScanCounters,
  ScanEvidence,
  ScanMatchOutcome,
  ScanProgress,
  ScanResultItem,
  ScanRootPreference,
  ScanSessionView,
  ScannerGateway,
} from "./types";
import { scannerRootPreferenceKey } from "./types";

const latestKey = ["scanner", "latest"] as const;
const terminalStatuses = new Set(["completed", "cancelled", "interrupted", "failed"]);

type ScanFilter = "all" | "matched" | "ambiguous" | "unmatched" | "issues";

export function ScannerPage({
  gateway,
  onReviewInstallation,
}: {
  gateway: ScannerGateway;
  onReviewInstallation?: (installationId: string) => void | Promise<void>;
}) {
  const { locale, t } = usePresentation();
  const queryClient = useQueryClient();
  const [filter, setFilter] = useState<ScanFilter>("all");
  const [currentPath, setCurrentPath] = useState<string | null>(null);
  const outcome = filter === "all" || filter === "issues" ? undefined : filter;
  const latest = useQuery({
    queryKey: latestKey,
    queryFn: () => gateway.readLatest(),
    refetchInterval: (query) => isActive(query.state.data?.session.status) ? 750 : false,
  });
  const rootPreference = useQuery({
    queryKey: scannerRootPreferenceKey,
    queryFn: () => gateway.readRootPreference(),
  });
  const sessionId = latest.data?.session.id;
  const results = useInfiniteQuery({
    queryKey: ["scanner", "results", sessionId, outcome ?? "all"],
    queryFn: ({ pageParam }) => gateway.browseResults({ sessionId: sessionId!, outcome, limit: 60, offset: pageParam }),
    initialPageParam: 0,
    getNextPageParam: (page) => {
      const nextOffset = page.offset + page.items.length;
      return nextOffset < page.total ? nextOffset : undefined;
    },
    enabled: Boolean(sessionId) && filter !== "issues",
  });
  const issues = useInfiniteQuery({
    queryKey: ["scanner", "issues", sessionId],
    queryFn: ({ pageParam }) => gateway.browseIssues({ sessionId: sessionId!, limit: 30, offset: pageParam }),
    initialPageParam: 0,
    getNextPageParam: (page) => {
      const nextOffset = page.offset + page.items.length;
      return nextOffset < page.total ? nextOffset : undefined;
    },
    enabled: Boolean(sessionId) && filter === "issues",
  });
  const acceptStartedScan = async (view: ScanSessionView | null) => {
    if (!view) return;
    setFilter("all");
    setCurrentPath(null);
    queryClient.setQueryData(latestKey, view);
    await invalidateScanQueries(queryClient, view.session.id);
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
  const cancel = useMutation({
    mutationFn: (id: string) => gateway.cancel(id),
  });
  const review = useMutation({
    mutationFn: ({ resultId }: { resultId: string }) =>
      gateway.createInstallation(sessionId!, resultId),
    onSuccess: async (installation) => {
      await queryClient.invalidateQueries({ queryKey: ["library", "installations"] });
      await queryClient.invalidateQueries({ queryKey: ["library", "shelves"] });
      await onReviewInstallation?.(installation.id);
    },
  });

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let disposed = false;
    void gateway.subscribeProgress((progress) => {
      queryClient.setQueryData<ScanSessionView | null>(latestKey, (current) =>
        mergeProgress(current, progress),
      );
      setCurrentPath(terminalStatuses.has(progress.status) ? null : progress.currentRelativePath);
      void invalidateScanEvidenceQueries(queryClient, progress.sessionId);
      if (terminalStatuses.has(progress.status)) {
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

  const view = latest.data;
  const active = isActive(view?.session.status);
  const requestError = startPreferred.error ?? startSelected.error ?? cancel.error ?? review.error
    ?? rootPreference.error ?? latest.error;
  const resultItems = results.data?.pages.flatMap((page) => page.items) ?? [];
  const issueItems = issues.data?.pages.flatMap((page) => page.items) ?? [];
  const starting = startPreferred.isPending || startSelected.isPending;
  const browse = () => startSelected.mutate();
  const scanDefault = () => startPreferred.mutate();
  const canScanDefault = !active && !starting && !rootPreference.isPending && Boolean(rootPreference.data?.canPrepare);

  return (
    <main className="scanner-page">
      <section className="scanner-masthead">
        <div className="scanner-masthead-title">
          <span className="scanner-eyebrow"><FolderSearch2 aria-hidden="true" />{t("scanner.eyebrow")}</span>
          <h1>{t("scanner.title")}</h1>
        </div>
        {view ? (
          <div className="scanner-actions">
            {active ? (
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
                <button className="scanner-button" type="button" disabled={starting} onClick={browse}>
                  {startSelected.isPending ? <LoaderCircle className="scanner-spin" aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}
                  {t("scanner.chooseFolder")}
                </button>
                <button
                  className="scanner-button scanner-button-cream"
                  type="button"
                  disabled={!canScanDefault}
                  onClick={scanDefault}
                >
                  {startPreferred.isPending ? <LoaderCircle className="scanner-spin" aria-hidden="true" /> : <FolderCog aria-hidden="true" />}
                  {t("scanner.scanDefault")}
                </button>
              </>
            )}
          </div>
        ) : null}
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
          <EmptyScanner
            busy={starting}
            choosing={startSelected.isPending}
            preparing={startPreferred.isPending}
            canScanDefault={canScanDefault}
            onBrowse={browse}
            onScanDefault={scanDefault}
          />
        ) : null}

        {rootPreference.data && !view ? <PreferredRoot preference={rootPreference.data} /> : null}

        {view ? (
          <>
            <ScanTarget view={view} locale={locale} />
            <ScanSummary
              view={view}
              locale={locale}
              currentPath={currentPath}
              onShowMatched={() => setFilter("matched")}
              matchedShown={filter === "matched"}
            />
            <section className="scanner-results-section">
              <header className="scanner-section-head">
                <div>
                  <h2>{t("scanner.results")}</h2>
                  {filter === "issues" ? (
                    <p>{t("scanner.issuesHelp", {
                      count: view.session.counters.recoverableErrors.toLocaleString(locale),
                    })}</p>
                  ) : null}
                </div>
                <OutcomeTabs
                  counters={view.session.counters}
                  issueCount={view.session.counters.recoverableErrors}
                  selected={filter}
                  onSelect={setFilter}
                />
              </header>

              {filter === "issues" ? (
                <>
                  {issues.isPending ? <ScannerLoading compact label="scanner.loadingIssues" /> : null}
                  {issues.error ? (
                    <div className="scanner-callout" role="alert">
                      <OctagonAlert aria-hidden="true" />
                      <span>{t("common.requestFailed", { error: String(issues.error) })}</span>
                    </div>
                  ) : null}
                  {issues.data ? <IssueList issues={issueItems} fallbackPath={view.root.displayPath} /> : null}
                  {issues.hasNextPage ? (
                    <button
                      className="scanner-load-more"
                      type="button"
                      disabled={issues.isFetchingNextPage}
                      onClick={() => void issues.fetchNextPage()}
                    >
                      {issues.isFetchingNextPage ? <LoaderCircle className="scanner-spin" aria-hidden="true" /> : null}
                      {t("scanner.showMoreIssues")}
                    </button>
                  ) : null}
                </>
              ) : (
                <>
                  {results.isPending ? <ScannerLoading compact /> : null}
                  {results.error ? (
                    <div className="scanner-callout" role="alert">
                      <OctagonAlert aria-hidden="true" />
                      <span>{t("common.requestFailed", { error: String(results.error) })}</span>
                    </div>
                  ) : null}
                  {results.data && resultItems.length === 0 ? (
                    <div className="scanner-empty-results">
                      <FileQuestion aria-hidden="true" /><span>{t("scanner.noResults")}</span>
                    </div>
                  ) : null}
                  {resultItems.length ? (
                    <div className="scanner-result-list">
                      {resultItems.map((item) => (
                        <ScanResultRow
                          item={item}
                          reviewable={view.session.status === "completed"}
                          reviewing={review.isPending && review.variables?.resultId === item.result.id}
                          busy={review.isPending}
                          onReview={() => review.mutate({ resultId: item.result.id })}
                          key={item.result.id}
                        />
                      ))}
                    </div>
                  ) : null}
                  {results.hasNextPage ? (
                    <button
                      className="scanner-load-more"
                      type="button"
                      disabled={results.isFetchingNextPage}
                      onClick={() => void results.fetchNextPage()}
                    >
                      {results.isFetchingNextPage ? <LoaderCircle className="scanner-spin" aria-hidden="true" /> : null}
                      {t("scanner.showMore")}
                    </button>
                  ) : null}
                </>
              )}
            </section>
          </>
        ) : null}
      </div>
    </main>
  );
}

function EmptyScanner({
  busy,
  choosing,
  preparing,
  canScanDefault,
  onBrowse,
  onScanDefault,
}: {
  busy: boolean;
  choosing: boolean;
  preparing: boolean;
  canScanDefault: boolean;
  onBrowse: () => void;
  onScanDefault: () => void;
}) {
  const { t } = usePresentation();
  return (
    <section className="scanner-empty">
      <span className="scanner-empty-mark"><FolderSearch2 aria-hidden="true" /></span>
      <h2>{t("scanner.emptyTitle")}</h2>
      <p>{t("scanner.description")}</p>
      <small>{t("scanner.emptyHelp")}</small>
      <div className="scanner-empty-actions">
        <button className="scanner-button" type="button" disabled={busy} onClick={onBrowse}>
          {choosing ? <LoaderCircle className="scanner-spin" aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}
          {t("scanner.chooseFolder")}
        </button>
        <button
          className="scanner-button scanner-button-primary"
          type="button"
          disabled={busy || !canScanDefault}
          onClick={onScanDefault}
        >
          {preparing ? <LoaderCircle className="scanner-spin" aria-hidden="true" /> : <FolderCog aria-hidden="true" />}
          {t("scanner.scanDefault")}
        </button>
      </div>
    </section>
  );
}

function PreferredRoot({ preference }: { preference: ScanRootPreference }) {
  const { t } = usePresentation();
  const status = preference.available
    ? t("scanner.rootReady")
    : preference.canPrepare
      ? t("scanner.rootWillBeCreated")
      : t("scanner.rootUnavailable");
  const source = preference.source === "configured"
    ? t("scanner.configuredRoot")
    : t("scanner.platformDefault");

  return (
    <section className="scanner-target">
      <span className="scanner-target-mark"><FolderCog aria-hidden="true" /></span>
      <span className="scanner-target-copy">
        <small>{t("scanner.defaultRoot")}</small>
        <strong>{preference.displayPath ?? t("scanner.rootUnavailable")}</strong>
        <span>{source}</span>
      </span>
      <span className="scanner-state">{status}</span>
    </section>
  );
}

function ScanTarget({ view, locale }: { view: ScanSessionView; locale: string }) {
  const { t } = usePresentation();
  const { session, root } = view;
  const active = isActive(session.status);
  const meta = [
    t(platformMessageKey(root.platform)),
    t("scanner.workers", { count: session.options.workerLimit }),
    session.finishedAt
      ? `${t("scanner.finished")} ${formatTimestamp(session.finishedAt, locale)}`
      : `${t("scanner.started")} ${formatTimestamp(session.startedAt, locale)}`,
  ];

  return (
    <section className={`scanner-target scanner-target-${session.status}`}>
      <span className="scanner-target-mark"><FolderOpen aria-hidden="true" /></span>
      <span className="scanner-target-copy">
        <small>{t(active ? "scanner.session.running" : "scanner.selectedRoot")}</small>
        <strong title={root.displayPath}>{root.displayPath}</strong>
        <span>{meta.join(" · ")}</span>
      </span>
      <span className={`scanner-state scanner-state-${session.status}`}>
        {active ? <LoaderCircle className="scanner-spin" aria-hidden="true" /> : <StatusIcon status={session.status} />}
        {t(sessionStatusKey(session.status))}
      </span>
    </section>
  );
}

function ScanSummary({
  view,
  locale,
  currentPath,
  onShowMatched,
  matchedShown,
}: {
  view: ScanSessionView;
  locale: string;
  currentPath: string | null;
  onShowMatched: () => void;
  matchedShown: boolean;
}) {
  const { t } = usePresentation();
  const { session } = view;
  const active = isActive(session.status);
  const counters = session.counters;
  const resultCount = counters.matched + counters.ambiguous + counters.unmatched;
  const recognisedCount = counters.matched + counters.ambiguous;
  const inspected = counters.inspectedFiles;
  const elapsed = useElapsedSeconds(session.startedAt, session.finishedAt, active);
  const stopped = session.status === "cancelled" || session.status === "interrupted" || session.status === "failed";
  const caption = active
    ? "scanner.filesInspectedSoFar"
    : stopped
      ? "scanner.filesInspectedStopped"
      : "scanner.filesInspectedCaption";

  return (
    <section className={`scanner-panel${stopped ? " scanner-panel-stopped" : ""}`} aria-label={t("scanner.results")}>
      <span className={`scanner-tab scanner-tab-${session.status}`}>
        {t(sessionStatusKey(session.status))}
        {elapsed === null ? null : <small>{formatDuration(elapsed * 1000, t)}</small>}
      </span>

      <p className="scanner-figure">
        <b>{inspected.toLocaleString(locale)}</b>
        <span>{t(caption)}</span>
      </p>

      {active ? (
        <>
          <div className="scanner-bar" aria-hidden="true"><i /></div>
          {currentPath ? (
            <p className="scanner-livepath">
              <FileQuestion aria-hidden="true" />
              <span title={currentPath}>{currentPath}</span>
            </p>
          ) : null}
        </>
      ) : resultCount > 0 ? (
        <div className="scanner-outcome-bar" aria-hidden="true">
          {counters.matched > 0 ? <i className="scanner-outcome-matched" style={{ flexGrow: counters.matched }} /> : null}
          {counters.ambiguous > 0 ? <i className="scanner-outcome-ambiguous" style={{ flexGrow: counters.ambiguous }} /> : null}
          {counters.unmatched > 0 ? <i className="scanner-outcome-unmatched" style={{ flexGrow: counters.unmatched }} /> : null}
        </div>
      ) : null}

      <div className="scanner-legend">
        <span className="scanner-legend-matched">
          <small>{t("scanner.matched")}</small>
          <b>{counters.matched.toLocaleString(locale)}</b>
        </span>
        <span className="scanner-legend-ambiguous">
          <small>{t("scanner.ambiguous")}</small>
          <b>{counters.ambiguous.toLocaleString(locale)}</b>
        </span>
        <span className="scanner-legend-unmatched">
          <small>{t("scanner.unmatched")}</small>
          <b>{counters.unmatched.toLocaleString(locale)}</b>
        </span>
      </div>

      <p className="scanner-footnote">
        {t("scanner.inspected", {
          files: counters.inspectedFiles.toLocaleString(locale),
          folders: counters.discoveredDirectories.toLocaleString(locale),
        })}
        {" · "}
        {counters.recoverableErrors > 0
          ? t("scanner.issueCount", { count: counters.recoverableErrors.toLocaleString(locale) })
          : t("scanner.noIssues")}
        {!active && inspected > 0 && recognisedCount === 0 ? ` · ${t("scanner.noneRecognised")}` : null}
        {stopped ? ` · ${t("scanner.stoppedKept")}` : null}
      </p>

      {session.fatalErrorMessage ? (
        <p className="scanner-fatal">
          <OctagonAlert aria-hidden="true" />
          {t("common.technicalDetail", { detail: session.fatalErrorMessage })}
        </p>
      ) : null}

      {active ? null : (
        <footer className="scanner-foot">
          <span className="scanner-safety">
            <ShieldCheck aria-hidden="true" />
            {stopped
              ? t("scanner.libraryUnchanged")
              : recognisedCount === 0
                ? t("scanner.checkFolder")
                : t("scanner.noInference")}
          </span>
          {session.status === "completed" && counters.matched > 0 && !matchedShown ? (
            <button className="scanner-button scanner-button-primary" type="button" onClick={onShowMatched}>
              <ListChecks aria-hidden="true" />
              {t("scanner.reviewMatched", { count: counters.matched.toLocaleString(locale) })}
            </button>
          ) : null}
        </footer>
      )}
    </section>
  );
}

function ScanResultRow({
  item,
  reviewable,
  reviewing,
  busy,
  onReview,
}: {
  item: ScanResultItem;
  reviewable: boolean;
  reviewing: boolean;
  busy: boolean;
  onReview: () => void;
}) {
  const { t } = usePresentation();
  const { result } = item;
  const labels = distinctEvidenceLabels(result.evidence);
  const heading = result.selectedWorkCode
    ?? result.candidates[0]?.workCode
    ?? t("scanner.unmatched");
  const canReview = reviewable && Boolean(result.candidateEntryId);

  return (
    <article className={`scanner-result scanner-result-${result.outcome}`}>
      <div className="scanner-result-copy">
        <div className="scanner-result-line">
          <strong>{heading}</strong>
          {result.confidence ? (
            <span className={`scanner-confidence scanner-confidence-${result.confidence}`}>
              {t(confidenceMessageKey(result.confidence))}
            </span>
          ) : null}
        </div>
        <code className="scanner-result-path" title={item.relativePath ?? undefined}>
          {item.relativePath ?? t("scanner.unknownPath")}
        </code>
        {labels.length ? (
          <div className="scanner-evidence-chips">
            {labels.slice(0, 4).map((label) => (
              <span className={label.strongest ? "strongest" : undefined} key={label.key}>{t(label.key)}</span>
            ))}
          </div>
        ) : null}
        {result.outcome === "ambiguous" && result.candidates.length > 1 ? (
          <div className="scanner-candidates">
            <small>{t("scanner.candidatesLabel")}</small>
            {result.candidates.slice(0, 4).map((candidate, index) => (
              <div className="scanner-candidate" key={candidate.workCode}>
                <i>{index + 1}</i>
                <strong>{candidate.workCode}</strong>
                <em>{t(confidenceMessageKey(candidate.confidence))}</em>
              </div>
            ))}
          </div>
        ) : null}
      </div>
      {canReview ? (
        <button className="scanner-review-button" type="button" disabled={busy} onClick={onReview}>
          {reviewing ? <LoaderCircle className="scanner-spin" aria-hidden="true" /> : <ListChecks aria-hidden="true" />}
          {t("scanner.review")}
        </button>
      ) : null}
    </article>
  );
}

function IssueList({
  issues,
  fallbackPath,
}: {
  issues: Array<{ id: string; relativePath: string | null; code: string; message: string }>;
  fallbackPath: string;
}) {
  const { t } = usePresentation();
  if (!issues.length) {
    return (
      <div className="scanner-empty-results">
        <FileQuestion aria-hidden="true" /><span>{t("scanner.noResults")}</span>
      </div>
    );
  }
  return (
    <div className="scanner-issue-list">
      {issues.map((issue) => (
        <article key={issue.id}>
          <AlertTriangle aria-hidden="true" />
          <div>
            <strong>{t(evidenceReasonMessageKey(issue.code))}</strong>
            <code>{issue.relativePath ?? fallbackPath}</code>
            <p>{t("common.technicalDetail", { detail: issue.message })}</p>
          </div>
        </article>
      ))}
    </div>
  );
}

function OutcomeTabs({
  counters,
  issueCount,
  selected,
  onSelect,
}: {
  counters: ScanCounters;
  issueCount: number;
  selected: ScanFilter;
  onSelect: (filter: ScanFilter) => void;
}) {
  const { t } = usePresentation();
  const total = counters.matched + counters.ambiguous + counters.unmatched;
  const tabs: Array<[ScanFilter, string, number]> = [
    ["all", t("scanner.all"), total],
    ["matched", t("scanner.matched"), counters.matched],
    ["ambiguous", t("scanner.ambiguous"), counters.ambiguous],
    ["unmatched", t("scanner.unmatched"), counters.unmatched],
  ];
  if (issueCount > 0) tabs.push(["issues", t("scanner.issues"), issueCount]);

  return (
    <div className="scanner-outcome-tabs" role="tablist" aria-label={t("scanner.results")}>
      {tabs.map(([value, label, count]) => (
        <button
          type="button"
          role="tab"
          aria-label={`${label} ${count}`}
          aria-selected={selected === value}
          className={`${value === "issues" ? "issues" : ""}${selected === value ? " active" : ""}`.trim() || undefined}
          key={value}
          onClick={() => onSelect(value)}
        >
          {label}<span>{count}</span>
        </button>
      ))}
    </div>
  );
}

function StatusIcon({ status }: { status: ScanSessionView["session"]["status"] }) {
  if (status === "completed") return <CheckCircle2 aria-hidden="true" />;
  if (status === "cancelled") return <Ban aria-hidden="true" />;
  return <AlertTriangle aria-hidden="true" />;
}

function ScannerLoading({
  compact = false,
  label = "scanner.loading",
}: {
  compact?: boolean;
  label?: MessageKey;
}) {
  const { t } = usePresentation();
  return (
    <div className={`scanner-loading${compact ? " compact" : ""}`}>
      <LoaderCircle className="scanner-spin" aria-hidden="true" /><span>{t(label)}</span>
    </div>
  );
}

function useElapsedSeconds(startedAt: string, finishedAt: string | null, active: boolean): number | null {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [active]);

  const start = new Date(startedAt).getTime();
  if (Number.isNaN(start)) return null;
  const end = finishedAt ? new Date(finishedAt).getTime() : now;
  if (Number.isNaN(end)) return null;
  return Math.max(0, Math.floor((end - start) / 1000));
}

export interface EvidenceLabel {
  key: MessageKey;
  strongest: boolean;
}

export function distinctEvidenceLabels(evidence: ScanEvidence[]): EvidenceLabel[] {
  const seen = new Set<MessageKey>();
  const labels: EvidenceLabel[] = [];
  for (const entry of evidence) {
    const key = evidenceReasonMessageKey(entry.reasonCode);
    if (seen.has(key)) continue;
    seen.add(key);
    labels.push({ key, strongest: key === "domain.evidence.archiveHashMatch" });
  }
  return labels.sort((left, right) => Number(right.strongest) - Number(left.strongest));
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

async function invalidateScanQueries(
  queryClient: ReturnType<typeof useQueryClient>,
  sessionId: string,
): Promise<void> {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: latestKey }),
    queryClient.invalidateQueries({ queryKey: ["scanner", "results", sessionId] }),
    queryClient.invalidateQueries({ queryKey: ["scanner", "issues", sessionId] }),
  ]);
}

async function invalidateScanEvidenceQueries(
  queryClient: ReturnType<typeof useQueryClient>,
  sessionId: string,
): Promise<void> {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["scanner", "results", sessionId] }),
    queryClient.invalidateQueries({ queryKey: ["scanner", "issues", sessionId] }),
  ]);
}

function formatTimestamp(value: string, locale: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(locale);
}

function sessionStatusKey(status: ScanSessionView["session"]["status"]) {
  switch (status) {
    case "queued": return "scanner.session.queued" as const;
    case "running": return "scanner.session.running" as const;
    case "completed": return "scanner.session.completed" as const;
    case "cancelled": return "scanner.session.cancelled" as const;
    case "interrupted": return "scanner.session.interrupted" as const;
    case "failed": return "scanner.session.failed" as const;
  }
}
