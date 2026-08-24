import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Archive,
  ArrowLeft,
  Check,
  CheckCircle2,
  ChevronRight,
  CopyPlus,
  ExternalLink,
  FolderOpen,
  LoaderCircle,
  PackageCheck,
  PackageOpen,
  PackageSearch,
  RefreshCw,
  Save,
  ShieldCheck,
  Square,
  Trash2,
  Wrench,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { formatByteSize as formatBytes } from "../../formatByteSize";
import {
  archivePolicyMessageKey,
  confidenceMessageKey,
  evidenceReasonMessageKey,
  launchActionMessageKey,
  mediaTypeMessageKey,
  packageContentMessageKey,
  sourceSetMessageKey,
} from "../../i18n/domainLabels";
import type { MessageKey } from "../../i18n/catalogs";
import { usePresentation } from "../../preferences/PresentationProvider";
import { effectiveIdentity, installationTitle } from "./libraryPresentation";
import {
  buildInstallationReviewDraft,
  installationLaunchSelections,
  installationReviewRequest,
  launchSelectionKey,
  type ContentReviewValue,
  type InstallationReviewDraft,
} from "./review";
import type {
  ContentItem,
  ArchiveRetentionPolicy,
  Installation,
  InstallationHealthReport,
  InstallationHealthState,
  LaunchCandidate,
  LibraryGateway,
  MediaType,
  PackageInspection,
  PackageDestinationConflictPolicy,
  PackageDestinationPreview,
  PackagePreparationProgress,
  PreparedPackageInstallation,
  SelectedInstallationDestination,
} from "./types";
import {
  mergePackagePreparationProgress,
  packagePreparationIsIndeterminate,
  packagePreparationCanCancel,
  packagePreparationIsTerminal,
  packagePreparationNeedsPreparedRefresh,
} from "./types";

const packagePreparationProgressKey = ["library", "package-preparation", "progress"] as const;

const mediaTypes: MediaType[] = [
  "executable",
  "audio",
  "image",
  "pdf",
  "video",
  "archive",
  "android_package",
  "directory",
  "unknown",
];

type DecideEditor = "identity" | "action" | "content" | null;

export function LibraryReviewPage({
  installationId,
  gateway,
  onBack,
  onOpenWork,
  focusPreparation = false,
}: {
  installationId: string;
  gateway: LibraryGateway;
  onBack: () => void;
  onOpenWork?: (workCode: string) => void | Promise<void>;
  focusPreparation?: boolean;
}) {
  const { locale, t } = usePresentation();
  const queryClient = useQueryClient();
  const installation = useQuery({
    queryKey: ["library", "installation", installationId],
    queryFn: () => gateway.readInstallation(installationId),
  });
  const [draft, setDraft] = useState<InstallationReviewDraft | null>(null);
  const [editor, setEditor] = useState<DecideEditor>(null);
  const focusedStateRef = useRef<string | null>(null);

  useEffect(() => {
    if (installation.data) setDraft(buildInstallationReviewDraft(installation.data));
  }, [installation.data]);

  useEffect(() => {
    const data = installation.data;
    if (!data) return;
    const token = `${data.id}:${data.status}`;
    if (focusedStateRef.current === token) return;
    focusedStateRef.current = token;
    setEditor(data.status === "ready" ? null : pendingReviewFocus(data));
  }, [installation.data]);

  const save = useMutation({
    mutationFn: (request: ReturnType<typeof installationReviewRequest>) => gateway.saveReview(request),
    onSuccess: async (saved) => {
      queryClient.setQueryData(["library", "installation", installationId], saved);
      setDraft(buildInstallationReviewDraft(saved));
      setEditor(null);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["library", "installations"] }),
        queryClient.invalidateQueries({ queryKey: ["library", "shelves"] }),
        queryClient.invalidateQueries({ queryKey: ["library", "work-action"] }),
      ]);
    },
  });

  if (installation.isPending) return <ReviewLoading />;
  if (installation.error || !installation.data) {
    return (
      <main className="library-review-page">
        <button className="library-back-button" type="button" onClick={onBack}><ArrowLeft aria-hidden="true" />{t("library.back")}</button>
        <div className="library-callout library-callout-error" role="alert"><AlertTriangle aria-hidden="true" />{installation.error ? t("common.requestFailed", { error: String(installation.error) }) : t("library.notFound")}</div>
      </main>
    );
  }
  if (!draft) return <ReviewLoading />;

  const current = installation.data;
  const invalidIdentity = draft.identityMode === "catalog_work" && !draft.identityWorkCode.trim();
  const requiresReview = current.overrides.reviewedAt === null;
  const saveCurrentReview = async () => {
    if (invalidIdentity) return;
    await save.mutateAsync(installationReviewRequest(current, draft));
  };
  const submit = () => {
    if (invalidIdentity) return;
    save.mutate(installationReviewRequest(current, draft));
  };
  const toggle = (next: Exclude<DecideEditor, null>) => setEditor((open) => (open === next ? null : next));

  return (
    <main className="library-review-page">
      <button className="library-back-button" type="button" onClick={onBack}><ArrowLeft aria-hidden="true" />{t("library.back")}</button>

      <header className="review-masthead">
        <div>
          <span className="library-eyebrow"><ShieldCheck aria-hidden="true" />{t("library.reviewEyebrow")}</span>
          <h1>{installationTitle(current)}</h1>
          <code title={current.rootPath}>{current.rootPath}</code>
        </div>
        <span className={`review-state review-state-${current.status}`}>
          {current.status === "ready" ? <CheckCircle2 aria-hidden="true" /> : <AlertTriangle aria-hidden="true" />}
          {t(current.status === "ready" ? "library.status.ready" : "library.status.needsReview")}
        </span>
      </header>

      {save.error ? <div className="library-callout library-callout-error" role="alert"><AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(save.error) })}</div> : null}
      {save.isSuccess ? <div className="library-callout library-callout-success" role="status"><Check aria-hidden="true" />{t("library.saved")}</div> : null}

      <section className="review-panel">
        <h2 className="review-tab">{t("library.review.decide")}</h2>

        <div className="review-rows">
          <IdentityRow
            installation={current}
            draft={draft}
            open={editor === "identity"}
            invalidIdentity={invalidIdentity}
            onToggle={() => toggle("identity")}
            onChange={setDraft}
            onOpenWork={onOpenWork}
          />
          <ActionRow
            installation={current}
            draft={draft}
            open={editor === "action"}
            onToggle={() => toggle("action")}
            onChange={setDraft}
          />
          <ContentRow
            installation={current}
            draft={draft}
            open={editor === "content"}
            onToggle={() => toggle("content")}
            onChange={setDraft}
          />
        </div>

        <footer className="review-foot">
          <span><ShieldCheck aria-hidden="true" />{t("library.noLaunchYet")}</span>
          <button
            className="review-button is-cream"
            type="button"
            disabled={save.isPending || invalidIdentity}
            onClick={submit}
          >
            {save.isPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <Save aria-hidden="true" />}
            {t(requiresReview ? "library.addToLibrary" : "library.saveChanges")}
          </button>
        </footer>
      </section>

      {current.detection.packageInspection ? (
        <PackageZone
          installation={current}
          inspection={current.detection.packageInspection}
          gateway={gateway}
          locale={locale}
          focusPreparation={focusPreparation}
          requiresReview={requiresReview}
          canFinalizeReview={!invalidIdentity && !save.isPending}
          onPrepared={saveCurrentReview}
        />
      ) : null}

      <ManageZone installation={current} gateway={gateway} onRemoved={onBack} locale={locale} />
    </main>
  );
}

/* ── Decide rows ─────────────────────────────────────────── */

function ReviewRow({
  editorId,
  label,
  value,
  hint,
  chips,
  open,
  onToggle,
  children,
}: {
  editorId: string;
  label: string;
  value: string;
  hint?: string;
  chips?: React.ReactNode;
  open: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  const { t } = usePresentation();
  return (
    <div className={`review-row${open ? " is-open" : ""}`}>
      <div className="review-row-head">
        <small>{label}</small>
        <div className="review-row-value">
          <strong>{value}</strong>
          {hint ? <span>{hint}</span> : null}
          {chips}
        </div>
        <button
          className="review-button"
          type="button"
          aria-expanded={open}
          aria-controls={editorId}
          aria-label={t(open ? "library.review.doneField" : "library.review.changeField", { field: label })}
          onClick={onToggle}
        >
          <ChevronRight aria-hidden="true" />
          {t(open ? "library.review.done" : "library.review.change")}
        </button>
      </div>
      {open ? <div id={editorId} className="review-row-body">{children}</div> : null}
    </div>
  );
}

function IdentityRow({
  installation,
  draft,
  open,
  invalidIdentity,
  onToggle,
  onChange,
  onOpenWork,
}: {
  installation: Installation;
  draft: InstallationReviewDraft;
  open: boolean;
  invalidIdentity: boolean;
  onToggle: () => void;
  onChange: (draft: InstallationReviewDraft) => void;
  onOpenWork?: (workCode: string) => void | Promise<void>;
}) {
  const { t } = usePresentation();
  const detected = installation.detection.catalogIdentity;
  const currentIdentity = effectiveIdentity(installation);
  const value = draft.identityMode === "unidentified"
    ? t("library.keepUnidentified")
    : draft.identityMode === "catalog_work"
      ? draft.identityWorkCode.trim() || t("library.review.identityUnset")
      : detected?.workCode ?? t("library.review.identityUnset");

  return (
    <ReviewRow
      editorId="library-review-identity-editor"
      label={t("library.identity")}
      value={value}
      hint={draft.customTitle.trim() ? draft.customTitle : undefined}
      chips={detected && draft.identityMode === "detected"
        ? <Evidence confidence={detected.confidence} reasons={detected.reasonCodes} />
        : undefined}
      open={open}
      onToggle={onToggle}
    >
      <div className="review-choices">
        <label className={draft.identityMode === "detected" ? "selected" : undefined}>
          <input
            type="radio"
            name="identity-mode"
            checked={draft.identityMode === "detected"}
            onChange={() => onChange({ ...draft, identityMode: "detected" })}
          />
          <strong>{t("library.useDetected")}</strong>
          <span>{detected?.workCode ?? t("library.noDetectedIdentity")}</span>
        </label>
        <label className={draft.identityMode === "catalog_work" ? "selected" : undefined}>
          <input
            type="radio"
            name="identity-mode"
            checked={draft.identityMode === "catalog_work"}
            onChange={() => onChange({ ...draft, identityMode: "catalog_work" })}
          />
          <strong>{t("library.chooseCatalogWork")}</strong>
          <input
            className={invalidIdentity ? "invalid" : undefined}
            type="text"
            value={draft.identityWorkCode}
            placeholder={t("library.workCodePlaceholder")}
            disabled={draft.identityMode !== "catalog_work"}
            aria-invalid={invalidIdentity}
            aria-label={t("library.chooseCatalogWork")}
            onChange={(event) => onChange({ ...draft, identityWorkCode: event.target.value })}
          />
        </label>
        <label className={draft.identityMode === "unidentified" ? "selected" : undefined}>
          <input
            type="radio"
            name="identity-mode"
            checked={draft.identityMode === "unidentified"}
            onChange={() => onChange({ ...draft, identityMode: "unidentified" })}
          />
          <strong>{t("library.keepUnidentified")}</strong>
          <span>{t("library.keepUnidentifiedHelp")}</span>
        </label>
      </div>
      <div className="review-inline-field">
        <label htmlFor="library-custom-title">{t("library.customTitle")}</label>
        <input
          id="library-custom-title"
          type="text"
          value={draft.customTitle}
          placeholder={currentIdentity ?? installation.rootPath.split(/[\\/]/).at(-1)}
          onChange={(event) => onChange({ ...draft, customTitle: event.target.value })}
        />
        {currentIdentity && onOpenWork ? (
          <button className="review-button" type="button" onClick={() => void onOpenWork(currentIdentity)}>
            <ExternalLink aria-hidden="true" />{t("library.openCatalogWork")}
          </button>
        ) : null}
      </div>
    </ReviewRow>
  );
}

function ActionRow({
  installation,
  draft,
  open,
  onToggle,
  onChange,
}: {
  installation: Installation;
  draft: InstallationReviewDraft;
  open: boolean;
  onToggle: () => void;
  onChange: (draft: InstallationReviewDraft) => void;
}) {
  const { t } = usePresentation();
  const selections = installationLaunchSelections(installation);
  const chosen = selections.find((selection) => launchSelectionKey(selection) === draft.preferredSelectionKey);

  return (
    <ReviewRow
      editorId="library-review-action-editor"
      label={t("library.review.opensWith")}
      value={chosen ? t(launchActionMessageKey(chosen.action)) : t("library.noPreferredAction")}
      hint={chosen ? targetLabel(chosen.target, t("library.installationRoot")) : t("library.noPreferredActionHelp")}
      open={open}
      onToggle={onToggle}
    >
      <div className="review-choices">
        <label className={draft.preferredSelectionKey === "" ? "selected" : undefined}>
          <input
            type="radio"
            name="preferred-action"
            checked={draft.preferredSelectionKey === ""}
            onChange={() => onChange({ ...draft, preferredSelectionKey: "" })}
          />
          <strong>{t("library.noPreferredAction")}</strong>
          <span>{t("library.noPreferredActionHelp")}</span>
        </label>
        {selections.map((selection) => {
          const key = launchSelectionKey(selection);
          const candidate = installation.detection.launchCandidates.find((item) => launchSelectionKey(item) === key);
          return (
            <label className={draft.preferredSelectionKey === key ? "selected" : undefined} key={key}>
              <input
                type="radio"
                name="preferred-action"
                checked={draft.preferredSelectionKey === key}
                onChange={() => onChange({ ...draft, preferredSelectionKey: key })}
              />
              <strong>{t(launchActionMessageKey(selection.action))}</strong>
              <span>{targetLabel(selection.target, t("library.installationRoot"))}</span>
              {candidate ? <Evidence confidence={candidate.confidence} reasons={candidate.reasonCodes} /> : <em>{t("library.manualSelection")}</em>}
            </label>
          );
        })}
      </div>
    </ReviewRow>
  );
}

function ContentRow({
  installation,
  draft,
  open,
  onToggle,
  onChange,
}: {
  installation: Installation;
  draft: InstallationReviewDraft;
  open: boolean;
  onToggle: () => void;
  onChange: (draft: InstallationReviewDraft) => void;
}) {
  const { locale, t } = usePresentation();
  const rows = useMemo(() => contentRows(installation), [installation]);
  const update = (path: string, value: ContentReviewValue) => {
    onChange({ ...draft, content: { ...draft.content, [path]: value } });
  };
  const overridden = rows.filter(({ relativePath }) => {
    const value = draft.content[relativePath];
    return Boolean(value && (value.mediaType || value.ignored || value.order));
  }).length;
  const ordered = rows.length > 1;
  const summaryType = rows.length === 1 && rows[0]
    ? t(mediaTypeMessageKey(draft.content[rows[0].relativePath]?.mediaType ?? rows[0].item?.mediaType ?? "unknown"))
    : "";

  return (
    <ReviewRow
      editorId="library-review-content-editor"
      label={t("library.detectedContent")}
      value={rows.length === 0
        ? t("library.noContent")
        : t("library.review.contentSummary", { count: rows.length.toLocaleString(locale) })}
      hint={rows.length === 1
        ? summaryType
        : overridden > 0
          ? t("library.review.contentOverrides", { count: overridden.toLocaleString(locale) })
          : undefined}
      open={open}
      onToggle={onToggle}
    >
      {rows.length === 0 ? <p className="review-note">{t("library.noContent")}</p> : (
        <div className="review-table-wrap">
          <table className="review-table">
            <thead>
              <tr>
                <th>{t("library.path")}</th>
                <th>{t("library.detectedType")}</th>
                <th>{t("library.overrideType")}</th>
                {ordered ? <th>{t("library.order")}</th> : null}
                <th>{t("library.ignore")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map(({ item, relativePath }) => {
                const value = draft.content[relativePath] ?? emptyContentReview();
                const displayType = value.mediaType ?? item?.mediaType ?? "unknown";
                return (
                  <tr className={value.ignored ? "ignored" : undefined} key={relativePath}>
                    <td>
                      <code title={relativePath}>{relativePath}</code>
                      {item ? <Evidence confidence={item.confidence} reasons={item.reasonCodes} /> : <small>{t("library.orphanedOverride")}</small>}
                    </td>
                    <td>{item ? t(mediaTypeMessageKey(item.mediaType)) : "—"}</td>
                    <td>
                      <select
                        value={displayType}
                        aria-label={t("library.overrideType")}
                        onChange={(event) => {
                          const selected = event.target.value as MediaType;
                          update(relativePath, { ...value, mediaType: item?.mediaType === selected ? null : selected });
                        }}
                      >
                        {mediaTypes.map((mediaType) => <option value={mediaType} key={mediaType}>{t(mediaTypeMessageKey(mediaType))}</option>)}
                      </select>
                    </td>
                    {ordered ? (
                      <td><input type="number" min="0" value={value.order} aria-label={t("library.orderFor", { path: relativePath })} onChange={(event) => update(relativePath, { ...value, order: event.target.value })} /></td>
                    ) : null}
                    <td><input type="checkbox" checked={value.ignored} aria-label={t("library.ignorePath", { path: relativePath })} onChange={(event) => update(relativePath, { ...value, ignored: event.target.checked })} /></td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </ReviewRow>
  );
}

/* ── Package zone ────────────────────────────────────────── */

function PackageZone({
  installation,
  inspection,
  gateway,
  locale,
  focusPreparation,
  requiresReview,
  canFinalizeReview,
  onPrepared,
}: {
  installation: Installation;
  inspection: PackageInspection;
  gateway: LibraryGateway;
  locale: string;
  focusPreparation: boolean;
  requiresReview: boolean;
  canFinalizeReview: boolean;
  onPrepared: () => Promise<void>;
}) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const [destination, setDestination] = useState<SelectedInstallationDestination | null>(null);
  const [destinationPreview, setDestinationPreview] = useState<PackageDestinationPreview | null>(null);
  const [destinationConflictPolicy, setDestinationConflictPolicy] =
    useState<PackageDestinationConflictPolicy>("refuse");
  const [retention, setRetention] = useState<ArchiveRetentionPolicy>(inspection.installPlan.archiveRetention);
  const sectionRef = useRef<HTMLElement>(null);
  const pendingReviewOperationRef = useRef<string | null>(null);
  const preparedKey = useMemo(
    () => ["library", "prepared-package", installation.id] as const,
    [installation.id],
  );
  const prepared = useQuery({
    queryKey: preparedKey,
    queryFn: () => gateway.readPreparedPackage(installation.id),
  });
  const progress = useQuery({
    queryKey: packagePreparationProgressKey,
    queryFn: async () => mergePackagePreparationProgress(
      queryClient.getQueryData<PackagePreparationProgress | null>(packagePreparationProgressKey),
      await gateway.readPackagePreparationProgress(),
    ),
    refetchInterval: (query) => {
      const value = query.state.data;
      return value
        && value.installationId === installation.id
        && !packagePreparationIsTerminal(value.stage)
        ? 650
        : false;
    },
  });
  const chooseDestination = useMutation({
    mutationFn: async () => {
      const selected = await gateway.selectInstallationDestination();
      if (!selected) return null;
      const preview = await gateway.inspectPackageDestination(
        installation.id,
        selected.accessHandle,
      );
      return { selected, preview };
    },
    onSuccess: (result) => {
      if (result) {
        setDestination(result.selected);
        setDestinationPreview(result.preview);
        setDestinationConflictPolicy("refuse");
      }
    },
  });
  const start = useMutation({
    mutationFn: () => {
      if (!destination) throw new Error(t("library.chooseDestinationFirst"));
      return gateway.startPackagePreparation(
        installation.id,
        destination.accessHandle,
        destinationConflictPolicy,
        retention,
      );
    },
    onSuccess: (value) => {
      if (requiresReview) pendingReviewOperationRef.current = value.operationId;
      queryClient.setQueryData<PackagePreparationProgress | null>(
        packagePreparationProgressKey,
        (currentProgress) => mergePackagePreparationProgress(currentProgress, value),
      );
    },
  });
  const cancel = useMutation({
    mutationFn: (operationId: string) => gateway.cancelPackagePreparation(operationId),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: packagePreparationProgressKey });
      void queryClient.invalidateQueries({ queryKey: preparedKey });
    },
  });

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let disposed = false;
    void gateway.subscribePackagePreparationProgress((value) => {
      queryClient.setQueryData<PackagePreparationProgress | null>(
        packagePreparationProgressKey,
        (currentProgress) => mergePackagePreparationProgress(currentProgress, value),
      );
    }).then((listener) => {
      if (disposed) listener();
      else unsubscribe = listener;
    });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [gateway, queryClient]);

  useEffect(() => {
    if (!packagePreparationNeedsPreparedRefresh(progress.data, installation.id)) return;
    void queryClient.invalidateQueries({ queryKey: preparedKey });
    void queryClient.invalidateQueries({ queryKey: ["library", "work-action"] });
  }, [installation.id, preparedKey, progress.data, queryClient]);

  useEffect(() => {
    const value = progress.data;
    if (
      value?.stage !== "completed"
      || value.installationId !== installation.id
      || value.operationId !== pendingReviewOperationRef.current
    ) return;
    pendingReviewOperationRef.current = null;
    void onPrepared().catch(() => undefined);
  }, [installation.id, onPrepared, progress.data]);

  useEffect(() => {
    if (!focusPreparation) return;
    const frame = window.requestAnimationFrame(() => {
      const section = sectionRef.current;
      if (!section) return;
      section.focus({ preventScroll: true });
      section.scrollIntoView({
        behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth",
        block: "start",
      });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [focusPreparation]);

  const currentProgress = progress.data?.installationId === installation.id ? progress.data : null;
  const active = Boolean(currentProgress && !packagePreparationIsTerminal(currentProgress.stage));
  const requestError = prepared.error ?? progress.error ?? chooseDestination.error ?? start.error ?? cancel.error;
  const isPrepared = Boolean(prepared.data);
  const destinationAvailable = destinationPreview?.state === "available";
  const keepBothSelected = destinationConflictPolicy === "keep_both"
    && Boolean(destinationPreview?.keepBothDestinationName);
  const replaceSelected = destinationConflictPolicy === "replace_existing"
    && destinationPreview?.state === "occupied_unknown";
  const destinationReady = destinationAvailable || keepBothSelected || replaceSelected;

  return (
    <section
      ref={sectionRef}
      className={`review-panel${focusPreparation ? " is-focused" : ""}`}
      tabIndex={-1}
    >
      <h2 className={`review-tab${isPrepared ? " is-quiet" : ""}`}>{t("library.preparePackage")}</h2>

      {requestError ? (
        <div className="library-callout library-callout-error" role="alert">
          <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(requestError) })}
        </div>
      ) : null}

      {prepared.isPending ? (
        <p className="review-note"><LoaderCircle className="library-spin" aria-hidden="true" />{t("library.readingPreparation")}</p>
      ) : prepared.data ? (
        <PreparedSummary prepared={prepared.data} locale={locale} />
      ) : (
        <>
          <div className="review-rows">
            <div className="review-row">
              <div className="review-row-head">
                <small>{t("library.installDestination")}</small>
                <div className="review-row-value">
                  <strong>{destination?.displayPath ?? t("library.noDestination")}</strong>
                  {destinationPreview ? <code>{destinationPreview.destinationName}</code> : null}
                </div>
                <button
                  className="review-button"
                  type="button"
                  disabled={active || chooseDestination.isPending}
                  onClick={() => chooseDestination.mutate()}
                >
                  {chooseDestination.isPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}
                  {t(destination ? "library.changeDestination" : "library.chooseDestination")}
                </button>
              </div>
            </div>
          </div>

          {destinationPreview && destinationPreview.state !== "available" ? (
            <div className={`review-destination-conflict${keepBothSelected ? " is-resolved" : ""}${replaceSelected ? " is-destructive" : ""}`} role="status">
              {keepBothSelected ? <CheckCircle2 aria-hidden="true" /> : <AlertTriangle aria-hidden="true" />}
              <div>
                <strong>{t(replaceSelected
                  ? "library.destinationReplaceReady"
                  : keepBothSelected
                    ? "library.destinationKeepBothReady"
                    : destinationPreview.state === "managed_same_installation"
                      ? "library.destinationManagedSame"
                      : destinationPreview.state === "managed_other_installation"
                        ? "library.destinationManagedOther"
                        : "library.destinationOccupied")}</strong>
                <span>{replaceSelected
                  ? t("library.destinationReplaceWarning", { name: destinationPreview.destinationName })
                  : keepBothSelected && destinationPreview.keepBothDestinationName
                    ? t("library.destinationKeepBothAs", { name: destinationPreview.keepBothDestinationName })
                    : t(destinationPreview.state === "managed_same_installation"
                      ? "library.destinationManagedSameHelp"
                      : destinationPreview.state === "managed_other_installation"
                        ? "library.destinationManagedOtherHelp"
                        : "library.destinationOccupiedHelp", { name: destinationPreview.destinationName })}</span>
              </div>
              <div className="review-destination-actions">
                {destinationPreview.state !== "managed_same_installation"
                  && destinationPreview.keepBothDestinationName ? (
                    <button className="review-button" type="button" disabled={keepBothSelected} onClick={() => setDestinationConflictPolicy("keep_both")}>
                      <CopyPlus aria-hidden="true" />{t("library.keepBoth")}
                    </button>
                  ) : null}
                {destinationPreview.state === "occupied_unknown" ? (
                  <button className="review-button is-danger" type="button" disabled={replaceSelected} onClick={() => setDestinationConflictPolicy("replace_existing")}>
                    <Trash2 aria-hidden="true" />{t("library.replaceExisting")}
                  </button>
                ) : null}
                <button className="review-button" type="button" disabled={chooseDestination.isPending} onClick={() => chooseDestination.mutate()}>
                  <FolderOpen aria-hidden="true" />{t("library.chooseAnotherDestination")}
                </button>
              </div>
            </div>
          ) : null}

          <fieldset className="review-choices review-retention" disabled={active}>
            <legend>{t("library.sourceRetention")}</legend>
            <label className={retention === "keep" ? "selected" : undefined}>
              <input type="radio" name="archive-retention" checked={retention === "keep"} onChange={() => setRetention("keep")} />
              <strong>{t("library.keepArchives")}</strong>
              <span>{t("library.keepArchivesHelp")}</span>
            </label>
            <label className={retention === "delete_after_verified_install" ? "selected" : undefined}>
              <input type="radio" name="archive-retention" checked={retention === "delete_after_verified_install"} onChange={() => setRetention("delete_after_verified_install")} />
              <strong>{t("library.deleteArchives")}</strong>
              <span>{t("library.deleteArchivesHelp")}</span>
            </label>
          </fieldset>

          {currentProgress ? (
            <PreparationOperation
              progress={currentProgress}
              cancelling={cancel.isPending}
              onCancel={() => cancel.mutate(currentProgress.operationId)}
            />
          ) : null}

          <footer className="review-foot">
            <span><ShieldCheck aria-hidden="true" />{t(replaceSelected
              ? "library.atomicReplacement"
              : "library.atomicPreparation")}</span>
            <button
              className="review-button is-primary"
              type="button"
              disabled={active || start.isPending || !destination || !destinationReady
                || inspection.safety !== "safe" || (requiresReview && !canFinalizeReview)}
              onClick={() => start.mutate()}
            >
              {active || start.isPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <PackageOpen aria-hidden="true" />}
              {t(requiresReview ? "library.installAndAdd" : "library.prepareAndVerify")}
            </button>
          </footer>
        </>
      )}

      <PackageFacts inspection={inspection} locale={locale} open={!isPrepared} />
    </section>
  );
}

function PackageFacts({
  inspection,
  locale,
  open,
}: {
  inspection: PackageInspection;
  locale: string;
  open: boolean;
}) {
  const { t } = usePresentation();
  const classification = inspection.classification;
  const plan = inspection.installPlan;
  const action = plan.preferredAction;
  const safe = inspection.safety === "safe";
  const sourceSet = inspection.sourceSet ?? { kind: "single_archive" as const, volumes: [inspection.source] };

  return (
    <details className="review-more" open={open}>
      <summary>
        <ChevronRight aria-hidden="true" />
        {t("library.packageInspection")}
        <span className={`review-chip review-chip-${safe ? "ok" : "warn"}`}>
          {t(safe ? "library.packageSafe" : "library.packageUnsafe")}
        </span>
      </summary>
      <div className="review-more-body">
        <div className="review-facts">
          <span><small>{t("library.packageFiles")}</small><b>{inspection.fileCount.toLocaleString(locale)}</b></span>
          <span><small>{t("library.packageFolders")}</small><b>{inspection.directoryCount.toLocaleString(locale)}</b></span>
          <span><small>{t("library.packageCompressed")}</small><b>{formatBytes(inspection.totalCompressedBytes, locale)}</b></span>
          <span><small>{t("library.packageExpanded")}</small><b>{formatBytes(inspection.totalUncompressedBytes, locale)}</b></span>
        </div>

        <div className="review-subrow">
          <div>
            <small>{t("library.sourceSet")}</small>
            <strong>{t(sourceSetMessageKey(sourceSet.kind))}</strong>
            <code>{sourceSet.volumes.length === 1
              ? `${inspection.source.relativePath} · ${inspection.format.toUpperCase()}`
              : t("library.review.volumes", { count: sourceSet.volumes.length, format: inspection.format.toUpperCase() })}</code>
          </div>
          <div>
            <small>{t("library.detectedPackage")}</small>
            <strong>{t(packageContentMessageKey(classification.contentKind))}</strong>
            <span>{classification.engine ?? t("library.unknownEngine")}</span>
            <Evidence confidence={classification.confidence} reasons={classification.reasonCodes} />
          </div>
          <div>
            <small>{t("library.proposedInstall")}</small>
            <strong>{action ? t(launchActionMessageKey(action.action)) : t("library.noSafePackageAction")}</strong>
            <code>{action?.relativePath ?? "—"}</code>
            {action ? <Evidence confidence={action.confidence} reasons={action.reasonCodes} /> : null}
          </div>
        </div>

        <div className="review-facts">
          <span><small>{t("library.contentRoot")}</small><b>{plan.contentRoot ?? inspection.commonRoot ?? "—"}</b></span>
          <span><small>{t("library.extraction")}</small><b>{t(plan.requiresExtraction ? "library.extractionRequired" : "library.extractionNotRequired")}</b></span>
          <span><small>{t("library.archivePolicy")}</small><b>{t(archivePolicyMessageKey(plan.archiveRetention))}</b></span>
          <span><small>{t("library.sourceSet")}</small><b>{t(sourceSetMessageKey(sourceSet.kind))} · {sourceSet.volumes.length}</b></span>
        </div>

        {sourceSet.volumes.length > 1 || plan.archiveRetention === "delete_after_verified_install" ? (
          <ol className="review-volumes">
            {sourceSet.volumes.map((volume) => (
              <li key={volume.scanEntryId}><code>{volume.relativePath}</code><span>{formatBytes(volume.sizeBytes ?? 0, locale)}</span></li>
            ))}
          </ol>
        ) : null}

        {sourceSet.kind === "multipart_rar_sfx" ? (
          <p className="review-note"><ShieldCheck aria-hidden="true" />{t("library.sfxSafety")}</p>
        ) : null}
      </div>
    </details>
  );
}

function PreparationOperation({
  progress,
  cancelling,
  onCancel,
}: {
  progress: PackagePreparationProgress;
  cancelling: boolean;
  onCancel: () => void;
}) {
  const { locale, t } = usePresentation();
  const active = !packagePreparationIsTerminal(progress.stage);
  const indeterminate = active && packagePreparationIsIndeterminate(progress.stage);
  const bytePercent = progress.counters.totalBytes > 0
    ? (progress.counters.processedBytes / progress.counters.totalBytes) * 100
    : 0;
  const filePercent = progress.counters.totalFiles > 0
    ? (progress.counters.processedFiles / progress.counters.totalFiles) * 100
    : 0;
  const percent = progress.stage === "completed" ? 100 : Math.min(100, Math.max(bytePercent, filePercent));

  return (
    <div className={`review-operation review-operation-${progress.stage}`}>
      <div className="review-operation-head">
        <div>
          <strong>{t(`library.preparationStage.${progress.stage}` as MessageKey)}</strong>
          {packagePreparationIsTerminal(progress.stage) && progress.stage !== "completed" && progress.detail
            ? <p>{t("common.technicalDetail", { detail: progress.detail })}</p>
            : null}
        </div>
        {active && packagePreparationCanCancel(progress.stage) ? (
          <button className="review-button is-danger" type="button" disabled={cancelling} onClick={onCancel}>
            <Square aria-hidden="true" />{t("library.cancelPreparation")}
          </button>
        ) : progress.stage === "completed"
          ? <PackageCheck aria-hidden="true" />
          : packagePreparationIsTerminal(progress.stage)
            ? <AlertTriangle aria-hidden="true" />
            : <LoaderCircle className="library-spin" aria-hidden="true" />}
      </div>
      <div
        className={`review-track${indeterminate ? " is-indeterminate" : ""}`}
        role="progressbar"
        aria-label={t("library.preparationProgress")}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={indeterminate ? undefined : Math.round(percent)}
      >
        <i style={indeterminate ? undefined : { width: `${percent}%` }} />
      </div>
      <div className="review-operation-meta">
        <code title={progress.currentPath ?? undefined}>{progress.currentPath ?? t(`library.preparationStage.${progress.stage}` as MessageKey)}</code>
        <strong>{indeterminate ? "…" : `${Math.round(percent)}%`}</strong>
      </div>
      <div className="review-facts">
        <span><small>{t("library.preparedBytes")}</small><b>{formatBytes(progress.counters.processedBytes, locale)} / {formatBytes(progress.counters.totalBytes, locale)}</b></span>
        <span><small>{t("library.preparedFiles")}</small><b>{progress.counters.processedFiles.toLocaleString(locale)} / {progress.counters.totalFiles.toLocaleString(locale)}</b></span>
      </div>
    </div>
  );
}

function PreparedSummary({ prepared, locale }: { prepared: PreparedPackageInstallation; locale: string }) {
  const { t } = usePresentation();
  return (
    <div className="review-rows">
      <div className="review-row">
        <div className="review-row-head">
          <small>{t("library.preparedAndVerified")}</small>
          <div className="review-row-value">
            <strong>{prepared.destinationRoot}</strong>
            <span>{t("library.preparedSummary", {
              files: prepared.installedFileCount.toLocaleString(locale),
              size: formatBytes(prepared.installedBytes, locale),
            })}</span>
            {prepared.sourcesDeleted ? <span>{t("library.sourceSetDeleted")}</span> : null}
            {prepared.sourceCleanupError ? <span className="is-error">{t("library.sourceCleanupFailed", { error: prepared.sourceCleanupError })}</span> : null}
          </div>
          <PackageCheck className="review-row-mark" aria-hidden="true" />
        </div>
      </div>
    </div>
  );
}

/* ── Manage + Danger ─────────────────────────────────────── */

function ManageZone({
  installation,
  gateway,
  onRemoved,
  locale,
}: {
  installation: Installation;
  gateway: LibraryGateway;
  onRemoved: () => void;
  locale: string;
}) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const [confirmation, setConfirmation] = useState<"remove" | "uninstall" | null>(null);
  const healthKey = ["library", "installation-health", installation.id] as const;
  const health = useQuery({
    queryKey: healthKey,
    queryFn: () => gateway.readInstallationHealth(installation.id),
  });
  const acceptReport = async (report: InstallationHealthReport) => {
    queryClient.setQueryData(healthKey, report);
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["library", "installations"] }),
      queryClient.invalidateQueries({ queryKey: ["library", "shelves"] }),
      queryClient.invalidateQueries({ queryKey: ["library", "prepared-packages"] }),
      queryClient.invalidateQueries({ queryKey: ["library", "installation", installation.id] }),
    ]);
  };
  const verify = useMutation({ mutationFn: () => gateway.verifyInstallation(installation.id), onSuccess: acceptReport });
  const locate = useMutation({
    mutationFn: async () => {
      const selected = await gateway.selectInstallationLocation();
      if (!selected) return null;
      return gateway.locateInstallation(installation.id, selected.accessHandle);
    },
    onSuccess: async (report) => {
      if (report) await acceptReport(report);
    },
  });
  const rescan = useMutation({ mutationFn: () => gateway.rescanInstallation(installation.id), onSuccess: acceptReport });
  const repair = useMutation({ mutationFn: () => gateway.repairInstallation(installation.id), onSuccess: acceptReport });
  const cleanup = useMutation({ mutationFn: () => gateway.cleanupMaintenance() });
  const destructive = useMutation({
    mutationFn: (action: "remove" | "uninstall") => action === "uninstall"
      ? gateway.uninstallInstallation(installation.id)
      : gateway.removeInstallation(installation.id),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["library", "installations"] }),
        queryClient.invalidateQueries({ queryKey: ["library", "shelves"] }),
        queryClient.invalidateQueries({ queryKey: ["library", "installation-healths"] }),
        queryClient.invalidateQueries({ queryKey: ["library", "prepared-packages"] }),
      ]);
      onRemoved();
      queryClient.removeQueries({
        queryKey: ["library", "installation", installation.id],
        exact: true,
      });
      queryClient.removeQueries({ queryKey: healthKey, exact: true });
      queryClient.removeQueries({
        queryKey: ["library", "prepared-package", installation.id],
        exact: true,
      });
    },
  });
  const pending = health.isPending
    || verify.isPending
    || locate.isPending
    || rescan.isPending
    || repair.isPending
    || cleanup.isPending
    || destructive.isPending;
  const report = health.data;
  const operationError = health.error ?? verify.error ?? locate.error ?? rescan.error ?? repair.error ?? cleanup.error ?? destructive.error;
  const moved = report?.state === "moved";
  const repairable = Boolean(report?.repairable);
  const checked = Boolean(report?.checkedAt);

  return (
    <>
      <section className="review-panel">
        <h2 className="review-tab is-quiet">{t("library.maintenance.title")}</h2>

        {operationError ? (
          <div className="library-callout library-callout-error" role="alert">
            <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(operationError) })}
          </div>
        ) : null}

        <div className="review-rows">
          <div className="review-row">
            <div className="review-row-head">
              <small>{t(report?.managed ? "library.maintenance.managedRoot" : "library.maintenance.externalRoot")}</small>
              <div className="review-row-value">
                <strong>{report?.checkedRoot ?? installation.rootPath}</strong>
                <span>
                  {report ? t("library.maintenance.expected") + ` ${report.expectedFiles.toLocaleString(locale)}` : null}
                  {report && checked
                    ? ` · ${t("library.maintenance.checkedAt", {
                      date: new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(new Date(report.checkedAt!)),
                    })}`
                    : ` · ${t("library.maintenance.notChecked")}`}
                </span>
                {report ? <MaintenanceState state={report.state} /> : null}
              </div>
              <button className="review-button is-primary" type="button" disabled={pending} onClick={() => verify.mutate()}>
                {verify.isPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <PackageSearch aria-hidden="true" />}
                {t("library.maintenance.verify")}
              </button>
            </div>
          </div>
        </div>

        {report && checked ? (
          <div className="review-facts">
            <span><small>{t("library.maintenance.present")}</small><b>{report.presentFiles.toLocaleString(locale)}</b></span>
            <span className={report.missingFiles > 0 ? "is-warning" : undefined}><small>{t("library.maintenance.missing")}</small><b>{report.missingFiles.toLocaleString(locale)}</b></span>
            <span className={report.modifiedFiles > 0 ? "is-warning" : undefined}><small>{t("library.maintenance.modified")}</small><b>{report.modifiedFiles.toLocaleString(locale)}</b></span>
            <span className={report.inaccessibleFiles > 0 ? "is-warning" : undefined}><small>{t("library.maintenance.inaccessible")}</small><b>{report.inaccessibleFiles.toLocaleString(locale)}</b></span>
            <span><small>{t("library.maintenance.unexpected")}</small><b>{report.unexpectedFiles.toLocaleString(locale)}</b></span>
          </div>
        ) : null}

        {moved ? (
          <div className="review-attention">
            <AlertTriangle aria-hidden="true" />
            <span>{t("library.maintenance.locateHelp")}</span>
            <button className="review-button" type="button" disabled={pending} onClick={() => locate.mutate()}>
              {locate.isPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}
              {t("library.maintenance.locate")}
            </button>
          </div>
        ) : null}

        {repairable ? (
          <div className="review-attention">
            <Wrench aria-hidden="true" />
            <span>{t("library.maintenance.repairHelp")}</span>
            <button className="review-button" type="button" disabled={pending} onClick={() => repair.mutate()}>
              {repair.isPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <Wrench aria-hidden="true" />}
              {t("library.maintenance.repair")}
            </button>
          </div>
        ) : null}

        {report?.issues.length ? (
          <details className="review-more">
            <summary><ChevronRight aria-hidden="true" />{t("library.maintenance.issueCount", { count: report.issues.length })}</summary>
            <ul className="review-issues">
              {report.issues.map((issue, index) => (
                <li key={`${issue.kind}-${issue.relativePath ?? "root"}-${index}`}>
                  <strong>{t(maintenanceIssueKey(issue.kind))}</strong>
                  {issue.relativePath ? <code>{issue.relativePath}</code> : null}
                  <span>{issue.detail}</span>
                </li>
              ))}
            </ul>
          </details>
        ) : null}

        <details className="review-more">
          <summary><ChevronRight aria-hidden="true" />{t("library.review.moreActions")}</summary>
          <div className="review-more-body">
            <div className="review-action-row">
              <button className="review-button" type="button" disabled={pending} onClick={() => rescan.mutate()}>
                {rescan.isPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <RefreshCw aria-hidden="true" />}
                {t("library.maintenance.rescan")}
              </button>
              {moved ? null : (
                <button className="review-button" type="button" disabled={pending} onClick={() => locate.mutate()}>
                  {locate.isPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}
                  {t("library.maintenance.locate")}
                </button>
              )}
              <button className="review-button" type="button" disabled={pending} onClick={() => cleanup.mutate()}>
                {cleanup.isPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <Archive aria-hidden="true" />}
                {t("library.maintenance.cleanupAction")}
              </button>
            </div>
            <p className="review-note">{t("library.maintenance.cleanupHelp")}</p>
            {cleanup.data ? (
              <div className="library-callout library-callout-success" role="status">
                <Check aria-hidden="true" />
                {t("library.maintenance.cleanupResult", {
                  staging: cleanup.data.removedStagingDirectories,
                  repairs: cleanup.data.removedRepairDirectories,
                  restored: cleanup.data.restoredSourceFiles,
                })}
              </div>
            ) : null}
            {cleanup.data?.retainedPaths.length ? (
              <ul className="review-issues">
                {cleanup.data.retainedPaths.map((path) => <li key={path}><code>{path}</code></li>)}
              </ul>
            ) : null}
          </div>
        </details>
      </section>

      <section className="review-panel is-danger">
        <h2 className="review-tab is-danger">{t("library.review.danger")}</h2>
        <div className="review-rows">
          <div className="review-row is-danger">
            <div className="review-row-head">
              <div className="review-row-value review-row-span">
                <strong>{t("library.maintenance.removeTitle")}</strong>
                <span>{t("library.maintenance.removeHelp")}</span>
              </div>
              <button className="review-button" type="button" disabled={pending} onClick={() => setConfirmation("remove")}>
                <Trash2 aria-hidden="true" />{t("library.maintenance.remove")}
              </button>
            </div>
          </div>
          {report?.managed ? (
            <div className="review-row is-danger">
              <div className="review-row-head">
                <div className="review-row-value review-row-span">
                  <strong>{t("library.maintenance.uninstallTitle")}</strong>
                  <span>{t("library.review.uninstallDetail", {
                    path: report.checkedRoot,
                    size: report.expectedFiles.toLocaleString(locale),
                  })}</span>
                </div>
                <button className="review-button is-danger" type="button" disabled={pending} onClick={() => setConfirmation("uninstall")}>
                  <Trash2 aria-hidden="true" />{t("library.maintenance.uninstall")}
                </button>
              </div>
            </div>
          ) : null}
          {confirmation ? (
            <div className="review-confirm" role="alertdialog" aria-modal="false">
              <AlertTriangle aria-hidden="true" />
              <p>
                {t(confirmation === "uninstall" ? "library.maintenance.confirmUninstall" : "library.maintenance.confirmRemove")}
                {confirmation === "uninstall" && report ? (
                  <em>{t("library.review.uninstallDetail", {
                    path: report.checkedRoot,
                    size: report.expectedFiles.toLocaleString(locale),
                  })}</em>
                ) : null}
              </p>
              <button className="review-button" type="button" disabled={destructive.isPending} onClick={() => setConfirmation(null)}>
                {t("common.cancel")}
              </button>
              <button className="review-button is-solid-danger" type="button" disabled={destructive.isPending} onClick={() => destructive.mutate(confirmation)}>
                {destructive.isPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <Trash2 aria-hidden="true" />}
                {t("library.maintenance.confirm")}
              </button>
            </div>
          ) : null}
        </div>
      </section>
    </>
  );
}

function MaintenanceState({ state }: { state: InstallationHealthState }) {
  const { t } = usePresentation();
  const healthy = state === "healthy";
  return (
    <span className={`review-chip review-chip-${healthy ? "ok" : "warn"}`}>
      {t(maintenanceStateKey(state))}
    </span>
  );
}

const maintenanceStateKeys: Record<InstallationHealthState, MessageKey> = {
  unknown: "library.maintenance.state.unknown",
  healthy: "library.maintenance.state.healthy",
  missing_files: "library.maintenance.state.missing_files",
  modified_files: "library.maintenance.state.modified_files",
  moved: "library.maintenance.state.moved",
  inaccessible: "library.maintenance.state.inaccessible",
  needs_review: "library.maintenance.state.needs_review",
  repairable: "library.maintenance.state.repairable",
};

const maintenanceIssueKeys: Record<
  InstallationHealthReport["issues"][number]["kind"],
  MessageKey
> = {
  missing: "library.maintenance.issue.missing",
  modified: "library.maintenance.issue.modified",
  inaccessible: "library.maintenance.issue.inaccessible",
  unexpected: "library.maintenance.issue.unexpected",
  invalid_ownership_marker: "library.maintenance.issue.invalid_ownership_marker",
};

function maintenanceStateKey(state: InstallationHealthState): MessageKey {
  return maintenanceStateKeys[state];
}

function maintenanceIssueKey(kind: InstallationHealthReport["issues"][number]["kind"]): MessageKey {
  return maintenanceIssueKeys[kind];
}

function Evidence({ confidence, reasons }: { confidence: string; reasons: string[] }) {
  const { t } = usePresentation();
  return (
    <span className={`review-evidence review-confidence-${confidence}`}>
      <b>{t(confidenceMessageKey(confidence))}</b>
      {reasons.slice(0, 3).map((reason) => <i key={reason}>{t(evidenceReasonMessageKey(reason))}</i>)}
    </span>
  );
}

function ReviewLoading() {
  const { t } = usePresentation();
  return <main className="library-review-page"><div className="library-loading"><LoaderCircle className="library-spin" aria-hidden="true" />{t("library.loadingReview")}</div></main>;
}

/**
 * Mirrors reviewStatus() in dla-domain/src/installation.rs: review is required either
 * because nothing identifies the work, or because the preferred launch target is
 * missing (fix in Opens with) or ignored (fix in Content).
 */
export function pendingReviewFocus(installation: Installation): DecideEditor {
  const identified = installation.detection.catalogIdentity ?? installation.overrides.catalogIdentity;
  if (!identified) return "identity";
  const preferred = installation.overrides.preferredAction;
  if (!preferred || preferred.target.kind !== "relative_path") return "action";
  const caseless = installation.platform === "windows";
  const key = caseless ? preferred.target.path.toLowerCase() : preferred.target.path;
  const matches = (value: string) => (caseless ? value.toLowerCase() : value) === key;
  const ignored = installation.overrides.contentItems.some(
    (item) => matches(item.relativePath) && item.ignored,
  );
  const exists = installation.detection.contentItems.some((item) => matches(item.relativePath));
  if (!exists) return "action";
  return ignored ? "content" : "action";
}

function contentRows(installation: Installation): Array<{ relativePath: string; item: ContentItem | null }> {
  const rows = new Map<string, ContentItem | null>();
  for (const item of installation.detection.contentItems) rows.set(item.relativePath, item);
  for (const override of installation.overrides.contentItems) {
    if (!rows.has(override.relativePath)) rows.set(override.relativePath, null);
  }
  return [...rows.entries()].map(([relativePath, item]) => ({ relativePath, item }));
}

function emptyContentReview(): ContentReviewValue {
  return { mediaType: null, ignored: false, order: "" };
}

function targetLabel(target: LaunchCandidate["target"], installationRoot: string): string {
  return target.kind === "installation_root" ? installationRoot : target.path;
}
