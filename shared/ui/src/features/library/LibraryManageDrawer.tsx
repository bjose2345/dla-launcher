import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  CircleHelp,
  ExternalLink,
  FolderOpen,
  LoaderCircle,
  PackageOpen,
  ShieldCheck,
  Wrench,
  X,
} from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";

import { useDocumentScrollLock } from "../../app/useDocumentScrollLock";
import { displayPath } from "../../displayPath";
import { formatByteSize } from "../../formatByteSize";
import { launchActionMessageKey, mediaTypeMessageKey } from "../../i18n/domainLabels";
import { usePresentation } from "../../preferences/PresentationProvider";
import { CatalogWorkPicker } from "../catalog/CatalogWorkPicker";
import type { CatalogDetailGateway, CatalogWork } from "../catalog/types";
import { WorkCardImage } from "../catalog/WorkCardImage";
import type { ScanInstallGateway } from "../scanner/types";
import { scanInstallIsActive } from "../scanner/types";
import { acceptScanInstallView, useScanInstalls } from "../scanner/useScanInstalls";
import type { SearchGateway } from "../search/types";
import { libraryDisplayCreator, libraryDisplayTitle } from "./libraryHome";
import { OwnWorkDetailsEditor, ownWorkDetailsKey, type OwnWorkSaveState } from "./OwnWorkDetailsEditor";
import { useOwnWork } from "./ownWorkDetails";
import {
  effectiveIdentity,
  installationFolder,
  installationName,
  pathInInstallationFolder,
} from "./libraryPresentation";
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
  Installation,
  InstallationHealthReport,
  LaunchCandidate,
  LibraryGateway,
  MediaType,
  PreparedPackageInstallation,
  OwnWorkDetails,
} from "./types";
import { packagePreparationPercent, usePackagePreparationProgress } from "./usePackagePreparationProgress";

import { manageTabs, type ManageTab } from "./types";

export type { ManageTab };

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

type SaveState = "idle" | "saving" | "saved" | "failed";

/**
 * A Library work's properties, as game launchers present them: General for
 * its name, catalog match and start file; Installed files for its folder,
 * verification and uninstalling. Each change saves as it is made.
 */
export function LibraryManageDrawer({
  installationId,
  initialTab = "general",
  gateway,
  catalogGateway,
  searchGateway,
  installs,
  onClose,
  onOpenWork,
}: {
  installationId: string;
  initialTab?: ManageTab;
  gateway: LibraryGateway;
  catalogGateway: Pick<CatalogDetailGateway, "readWorks">;
  searchGateway?: Pick<SearchGateway, "search">;
  installs?: ScanInstallGateway;
  onClose: () => void;
  onOpenWork?: (workCode: string) => void;
}) {
  const { locale, t } = usePresentation();
  const queryClient = useQueryClient();
  const headingId = useId();
  const panel = useRef<HTMLElement>(null);
  const [tab, setTab] = useState<ManageTab>(initialTab);
  const installationKey = useMemo(() => ["library", "installation", installationId] as const, [installationId]);
  const installation = useQuery({
    queryKey: installationKey,
    queryFn: () => gateway.readInstallation(installationId),
  });
  const prepared = useQuery({
    queryKey: ["library", "prepared-package", installationId],
    queryFn: () => gateway.readPreparedPackage(installationId),
    enabled: Boolean(installation.data?.detection.packageInspection),
  });
  const code = installation.data ? effectiveIdentity(installation.data) : null;
  const work = useQuery({
    queryKey: ["catalog-works", code ? [code] : []],
    queryFn: () => catalogGateway.readWorks([code!]),
    enabled: Boolean(code),
    staleTime: 5 * 60_000,
    select: (works) => works.find((item) => item.code === code),
  });
  const autosave = useReviewAutosave(installationId, gateway);
  const own = useQuery({
    queryKey: ownWorkDetailsKey(installationId),
    queryFn: () => gateway.readOwnWorkDetails(installationId),
  });
  const [ownSave, setOwnSave] = useState<{ state: OwnWorkSaveState; error?: unknown } | null>(null);
  // Your details save on their own; the line under the heading shows whichever saved last.
  const saveState = ownSave?.state ?? autosave.state;
  const saveError = ownSave?.state === "failed" ? ownSave.error : autosave.error;

  useDocumentScrollLock(true);
  useEffect(() => {
    setTab(initialTab);
  }, [initialTab, installationId]);
  useEffect(() => {
    panel.current?.focus({ preventScroll: true });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const preferEnglish = locale !== "ja-JP";
  const current = installation.data;
  const title = current ? libraryDisplayTitle(current, work.data, preferEnglish) : "";

  return (
    <div className="library-manage-layer">
      <div className="library-manage-backdrop" aria-hidden="true" onClick={onClose} />
      <section
        ref={panel}
        className="library-manage"
        role="dialog"
        aria-modal="true"
        aria-labelledby={headingId}
        tabIndex={-1}
      >
        <header className="library-manage-head">
          <span className="library-manage-art" aria-hidden="true">
            {work.data ? (
              <WorkCardImage
                code={work.data.code}
                title={title}
                mainImageUrls={work.data.mainImageUrls}
                thumbnailUrls={work.data.thumbnailUrls}
              />
            ) : own.data?.cover ? (
              <WorkCardImage
                code={own.data.cover}
                title={title}
                mainImageUrls={[gateway.ownWorkArtworkUrl(installationId, own.data.cover)]}
                thumbnailUrls={[]}
              />
            ) : <CircleHelp />}
          </span>
          <div className="library-manage-heading">
            <span className="library-manage-eyebrow">{t("library.manage.eyebrow")}</span>
            <h2 id={headingId}>{title || t("library.loading")}</h2>
            {current ? (
              <span>
                {work.data
                  ? `${libraryDisplayCreator(work.data, preferEnglish)} · ${work.data.code}`
                  : [own.data?.circle, own.data?.code].filter(Boolean).join(" · ") || t("library.own.yourWork")}
              </span>
            ) : null}
          </div>
          <button className="library-manage-close" type="button" aria-label={t("library.manage.close")} onClick={onClose}>
            <X aria-hidden="true" />
          </button>
        </header>

        <p className={`library-manage-saved is-${saveState}`} role="status">
          {saveState === "saving" ? <LoaderCircle className="library-spin" aria-hidden="true" /> : null}
          {saveState === "saved" ? <Check aria-hidden="true" /> : null}
          {saveState === "failed" ? <AlertTriangle aria-hidden="true" /> : null}
          {t(saveState === "saving"
            ? "library.manage.saving"
            : saveState === "saved"
              ? "library.manage.saved"
              : saveState === "failed"
                ? "library.manage.saveFailed"
                : "library.manage.autosave")}
        </p>

        <div className="library-manage-tabs" role="tablist" aria-label={t("library.manage.tabs")}>
          {manageTabs.map((value) => (
            <button
              type="button"
              role="tab"
              aria-selected={tab === value}
              className={tab === value ? "active" : undefined}
              key={value}
              onClick={() => setTab(value)}
            >
              {t(value === "general" ? "library.manage.tab.general" : "library.manage.tab.installedFiles")}
            </button>
          ))}
        </div>

        <div className="library-manage-body" role="tabpanel">
          {saveError ? (
            <div className="library-callout library-callout-error" role="alert">
              <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(saveError) })}
            </div>
          ) : null}
          {installation.isPending ? (
            <p className="library-manage-note"><LoaderCircle className="library-spin" aria-hidden="true" />{t("library.loading")}</p>
          ) : null}
          {installation.error ? (
            <div className="library-callout library-callout-error" role="alert">
              <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(installation.error) })}
            </div>
          ) : null}
          {current && tab === "general" ? (
            <GeneralTab
              installation={current}
              work={work.data}
              own={own.data ?? undefined}
              ownEditor={!work.data && work.fetchStatus !== "fetching" ? (
                <OwnWorkDetailsEditor
                  installationId={installationId}
                  gateway={gateway}
                  onSaveState={(state, error) => setOwnSave({ state, error })}
                />
              ) : null}
              catalogGateway={catalogGateway}
              searchGateway={searchGateway}
              onSave={(change) => {
                setOwnSave(null);
                autosave.save(change);
              }}
              onOpenWork={onOpenWork}
            />
          ) : null}
          {current && tab === "files" ? (
            <InstalledFilesTab
              installation={current}
              title={title}
              prepared={prepared.data ?? null}
              gateway={gateway}
              installs={installs}
              onSave={autosave.save}
              onRemoved={async () => {
                onClose();
                queryClient.removeQueries({ queryKey: installationKey, exact: true });
                await queryClient.invalidateQueries({ queryKey: ["library"] });
                await queryClient.invalidateQueries({ queryKey: ["scanner", "findings"] });
              }}
            />
          ) : null}
        </div>
      </section>
    </div>
  );
}

/** Saves one change at a time, each built on the newest saved review. */
function useReviewAutosave(installationId: string, gateway: LibraryGateway) {
  const queryClient = useQueryClient();
  const chain = useRef<Promise<void>>(Promise.resolve());
  const [state, setState] = useState<SaveState>("idle");
  const [error, setError] = useState<unknown>(null);
  const key = ["library", "installation", installationId] as const;

  const save = (change: (draft: InstallationReviewDraft) => InstallationReviewDraft) => {
    chain.current = chain.current.then(async () => {
      const current = queryClient.getQueryData<Installation>(key);
      if (!current) return;
      const draft = change(buildInstallationReviewDraft(current));
      if (draft.identityMode === "catalog_work" && !draft.identityWorkCode.trim()) return;
      setState("saving");
      setError(null);
      try {
        const saved = await gateway.saveReview(installationReviewRequest(current, draft));
        queryClient.setQueryData(key, saved);
        setState("saved");
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: ["library", "shelves"] }),
          queryClient.invalidateQueries({ queryKey: ["library", "installations"] }),
          queryClient.invalidateQueries({ queryKey: ["library", "work-action"] }),
        ]);
      } catch (failure) {
        setError(failure);
        setState("failed");
      }
    });
  };

  return { save, state, error };
}

type Save = ReturnType<typeof useReviewAutosave>["save"];

function GeneralTab({
  installation,
  work,
  own,
  ownEditor,
  catalogGateway,
  searchGateway,
  onSave,
  onOpenWork,
}: {
  installation: Installation;
  work: CatalogWork | undefined;
  own: OwnWorkDetails | undefined;
  /** Shown for a work outside the catalog, after its catalog section. */
  ownEditor: ReactNode;
  catalogGateway: Pick<CatalogDetailGateway, "readWorks">;
  searchGateway?: Pick<SearchGateway, "search">;
  onSave: Save;
  onOpenWork?: (workCode: string) => void;
}) {
  const { locale, t } = usePresentation();
  const preferEnglish = locale !== "ja-JP";
  const ownWork = useOwnWork();
  const [matching, setMatching] = useState(false);
  const nameId = useId();
  const startId = useId();
  const savedName = installation.overrides.customTitle ?? "";
  const [name, setName] = useState(savedName);
  useEffect(() => setName(savedName), [savedName]);
  const detected = installation.detection.catalogIdentity;
  const manual = installation.overrides.catalogIdentity;
  const code = effectiveIdentity(installation);
  const commitName = () => {
    if (name.trim() === savedName.trim()) return;
    onSave((draft) => ({ ...draft, customTitle: name }));
  };
  const draft = buildInstallationReviewDraft(installation);
  const selections = installation.detection.packageInspection ? [] : installationLaunchSelections(installation);
  const recommended = recommendedKey(installation.detection.launchCandidates);

  return (
    <div className="library-manage-stack">
      <section className="library-manage-field">
        <label htmlFor={nameId}><h3>{t("library.manage.name")}</h3></label>
        <input
          id={nameId}
          className="library-manage-input"
          type="text"
          value={name}
          placeholder={work ? (preferEnglish && work.titleEnglish.trim() ? work.titleEnglish : work.title) : installationName(installation)}
          onChange={(event) => setName(event.target.value)}
          onBlur={commitName}
          onKeyDown={(event) => {
            if (event.key === "Enter") commitName();
          }}
        />
        <p className="library-manage-help">{t(work ? "library.manage.titleHelp" : "library.manage.titleHelpUnknown")}</p>
      </section>

      <section className="library-manage-field">
        <h3>{t("library.manage.catalogWork")}</h3>
        <div className={`library-manage-work${work ? "" : " is-unknown"}`}>
          {work ? (
            <>
              <span className="library-manage-work-cover" aria-hidden="true">
                <WorkCardImage
                  code={work.code}
                  title={work.title}
                  mainImageUrls={work.mainImageUrls}
                  thumbnailUrls={work.thumbnailUrls}
                />
              </span>
              <span className="library-manage-work-copy">
                <strong>{preferEnglish && work.titleEnglish.trim() ? work.titleEnglish : work.title}</strong>
                <small>{libraryDisplayCreator(work, preferEnglish)} · {work.code}</small>
              </span>
            </>
          ) : (
            <>
              <span className="library-manage-work-cover" aria-hidden="true"><CircleHelp /></span>
              <span className="library-manage-work-copy">
                <strong>{t(own ? "library.own.yourWork" : "library.manage.unknownWork")}</strong>
                <dl className="library-manage-unknown">
                  <div><dt>{t("library.manage.circle")}</dt><dd>{own?.circle ?? "?"}</dd></div>
                  <div><dt>{t("library.manage.code")}</dt><dd>{code ?? own?.code ?? "?"}</dd></div>
                </dl>
              </span>
            </>
          )}
          <span className="library-manage-work-actions">
            <button className="library-manage-button" type="button" aria-expanded={matching} onClick={() => setMatching((open) => !open)}>
              {t(matching ? "library.manage.closeMatch" : work ? "library.manage.fixMatch" : "library.manage.findInCatalog")}
            </button>
            {work && onOpenWork ? (
              <button className="library-manage-link" type="button" onClick={() => onOpenWork(work.code)}>
                <ExternalLink aria-hidden="true" />{t("workMenu.catalogPage")}
              </button>
            ) : null}
            {!work && ownWork.openPage ? (
              <button className="library-manage-link" type="button" onClick={() => ownWork.openPage!(installation.id)}>
                <ExternalLink aria-hidden="true" />{t("workMenu.workPage")}
              </button>
            ) : null}
          </span>
        </div>
        {!work ? <p className="library-manage-help">{t("library.manage.unknownHelp")}</p> : null}
        {matching ? (
          <div className="library-manage-match">
            <CatalogWorkPicker
              catalogGateway={catalogGateway}
              searchGateway={searchGateway}
              autoFocus
              onPick={(picked) => {
                setMatching(false);
                onSave((next) => ({ ...next, identityMode: "catalog_work", identityWorkCode: picked.code }));
              }}
            />
            <div className="library-manage-row">
              {detected && manual ? (
                <button
                  className="library-manage-link"
                  type="button"
                  onClick={() => {
                    setMatching(false);
                    onSave((next) => ({ ...next, identityMode: "detected" }));
                  }}
                >
                  {t("library.manage.useDetected", { code: detected.workCode })}
                </button>
              ) : null}
              {manual?.kind !== "unidentified" ? (
                <button
                  className="library-manage-link"
                  type="button"
                  onClick={() => {
                    setMatching(false);
                    onSave((next) => ({ ...next, identityMode: "unidentified" }));
                  }}
                >
                  {t("library.manage.markUnknown")}
                </button>
              ) : null}
            </div>
          </div>
        ) : null}
      </section>

      {ownEditor}

      {selections.length > 1 ? (
        <section className="library-manage-field">
          <label htmlFor={startId}><h3>{t("library.manage.startsWith")}</h3></label>
          <select
            id={startId}
            className="library-manage-input"
            value={draft.preferredSelectionKey}
            onChange={(event) => {
              const key = event.target.value;
              onSave((next) => ({ ...next, preferredSelectionKey: key }));
            }}
          >
            {draft.preferredSelectionKey ? null : <option value="">{t("library.manage.startsWithNone")}</option>}
            {selections.map((selection) => {
              const key = launchSelectionKey(selection);
              const target = selection.target.kind === "installation_root"
                ? t(launchActionMessageKey(selection.action))
                : `${pathInInstallationFolder(installation, selection.target.path)} · ${t(launchActionMessageKey(selection.action))}`;
              return (
                <option value={key} key={key}>
                  {key === recommended ? t("library.manage.recommendedOption", { label: target }) : target}
                </option>
              );
            })}
          </select>
        </section>
      ) : null}
    </div>
  );
}

function InstalledFilesTab({
  installation,
  title,
  prepared,
  gateway,
  installs,
  onSave,
  onRemoved,
}: {
  installation: Installation;
  title: string;
  prepared: PreparedPackageInstallation | null;
  gateway: LibraryGateway;
  installs?: ScanInstallGateway;
  onSave: Save;
  onRemoved: () => Promise<void>;
}) {
  const { locale, t } = usePresentation();
  const queryClient = useQueryClient();
  const [verified, setVerified] = useState(false);
  const [confirm, setConfirm] = useState<"remove" | "uninstall" | null>(null);
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
      queryClient.invalidateQueries({ queryKey: ["library", "installation-healths"] }),
      queryClient.invalidateQueries({ queryKey: ["library", "installation", installation.id] }),
    ]);
  };
  const verify = useMutation({
    mutationFn: () => gateway.verifyInstallation(installation.id),
    onSuccess: async (report) => {
      setVerified(true);
      await acceptReport(report);
    },
  });
  const locate = useMutation({
    mutationFn: async () => {
      const selected = await gateway.selectInstallationLocation();
      return selected ? gateway.locateInstallation(installation.id, selected.accessHandle) : null;
    },
    onSuccess: async (report) => {
      if (report) await acceptReport(report);
    },
  });
  const rescan = useMutation({ mutationFn: () => gateway.rescanInstallation(installation.id), onSuccess: acceptReport });
  const repair = useMutation({ mutationFn: () => gateway.repairInstallation(installation.id), onSuccess: acceptReport });
  const cleanup = useMutation({ mutationFn: () => gateway.cleanupMaintenance() });
  const browse = useMutation({ mutationFn: () => gateway.openInstallationFolder!(installation.id) });
  const remove = useMutation({
    mutationFn: (kind: "remove" | "uninstall") => kind === "uninstall"
      ? gateway.uninstallInstallation(installation.id)
      : gateway.removeInstallation(installation.id),
    onSuccess: onRemoved,
  });
  const scanInstalls = useScanInstalls(installs);
  const progress = usePackagePreparationProgress(installs ? gateway : undefined);
  const install = useMutation({
    mutationFn: async () => {
      if (!installs) return;
      if (!scanInstalls.data?.installFolder) {
        const chosen = await installs.selectInstallFolder();
        if (!chosen?.installFolder) return;
        acceptScanInstallView(queryClient, chosen);
      }
      acceptScanInstallView(queryClient, await installs.installArchive(installation.id));
    },
  });

  const report = health.data;
  const busy = verify.isPending || locate.isPending || rescan.isPending || repair.isPending || cleanup.isPending
    || remove.isPending;
  const error = health.error ?? verify.error ?? locate.error ?? rescan.error ?? repair.error ?? cleanup.error
    ?? install.error ?? browse.error ?? remove.error;
  const archive = installation.detection.packageInspection;
  const running = scanInstalls.data?.installs.find((item) => (
    item.installationId === installation.id && scanInstallIsActive(item.status)
  ));
  const percent = running && progress.data?.installationId === installation.id
    ? packagePreparationPercent(progress.data)
    : null;
  const location = displayPath(prepared?.destinationRoot ?? installationFolder(installation));
  const fileCount = prepared?.installedFileCount ?? installation.detection.contentItems.length;
  const sizeBytes = prepared?.installedBytes
    ?? installation.detection.contentItems.reduce((total, item) => total + (item.sizeBytes ?? 0), 0);
  const changedFiles = report ? report.missingFiles + report.modifiedFiles + report.inaccessibleFiles : 0;
  const problem = report && ["missing_files", "modified_files", "inaccessible", "repairable"].includes(report.state);

  return (
    <div className="library-manage-stack">
      {error ? (
        <div className="library-callout library-callout-error" role="alert">
          <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(error) })}
        </div>
      ) : null}

      {archive && !prepared ? (
        <section className="library-manage-field is-attention">
          <h3>{t("library.manage.notInstalled")}</h3>
          <code className="library-manage-path">{archive.source.relativePath}</code>
          <p className="library-manage-help">{t("library.manage.notInstalledHelp")}</p>
          {running ? (
            <span className="library-manage-progress">
              <span><LoaderCircle className="library-spin" aria-hidden="true" />
                {percent === null ? t("library.manage.installing") : t("library.manage.installingPercent", { percent })}
              </span>
              <progress max={100} value={percent ?? undefined} />
            </span>
          ) : installs ? (
            <button className="library-manage-button is-primary" type="button" disabled={install.isPending} onClick={() => install.mutate()}>
              {install.isPending ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <PackageOpen aria-hidden="true" />}
              {t("library.manage.install")}
            </button>
          ) : null}
        </section>
      ) : null}

      <section className="library-manage-field">
        <h3>{t("library.manage.location")}</h3>
        <div className="library-manage-location">
          <code className="library-manage-path" title={location}>{location}</code>
          {gateway.openInstallationFolder ? (
            <button className="library-manage-button" type="button" disabled={browse.isPending} onClick={() => browse.mutate()}>
              <FolderOpen aria-hidden="true" />{t("library.manage.browse")}
            </button>
          ) : null}
        </div>
        <p className="library-manage-help">
          {fileCount === 1
            ? t("library.manage.sizeOne", { size: formatByteSize(sizeBytes, locale) })
            : t("library.manage.size", {
              count: fileCount.toLocaleString(locale),
              size: formatByteSize(sizeBytes, locale),
            })}
        </p>
      </section>

      <section className="library-manage-field">
        <div className="library-manage-row">
          <ActionButton busy={verify.isPending} disabled={busy} icon={<ShieldCheck aria-hidden="true" />} onClick={() => verify.mutate()}>
            {t(verify.isPending ? "library.manage.verifying" : "library.manage.verify")}
          </ActionButton>
        </div>
        {verified && report?.state === "healthy" ? (
          <p className="library-manage-result is-ok" role="status">
            <CheckCircle2 aria-hidden="true" />
            {report.presentFiles === 1
              ? t("library.manage.verifyOkOne")
              : t("library.manage.verifyOk", { count: report.presentFiles.toLocaleString(locale) })}
          </p>
        ) : null}
        {problem ? (
          <div className="library-manage-result is-problem" role="status">
            <AlertTriangle aria-hidden="true" />
            <span>
              {changedFiles === 1
                ? t("library.manage.verifyProblemOne")
                : t("library.manage.verifyProblem", { count: changedFiles.toLocaleString(locale) })}
            </span>
            {report.repairable ? (
              <ActionButton busy={repair.isPending} disabled={busy} icon={<Wrench aria-hidden="true" />} onClick={() => repair.mutate()}>
                {t("library.manage.repair")}
              </ActionButton>
            ) : null}
          </div>
        ) : null}
        {report?.state === "moved" ? (
          <div className="library-manage-result is-problem" role="status">
            <AlertTriangle aria-hidden="true" />
            <span>{t("library.manage.moved")}</span>
            <ActionButton busy={locate.isPending} disabled={busy} icon={<FolderOpen aria-hidden="true" />} onClick={() => locate.mutate()}>
              {t("library.manage.locate")}
            </ActionButton>
          </div>
        ) : null}
      </section>

      <section className="library-manage-field">
        {report?.managed ? (
          <div className="library-manage-remove">
            {confirm === "uninstall" ? (
              <ConfirmRow
                question={t("library.manage.uninstallConfirm")}
                confirmLabel={t("library.manage.uninstallAction")}
                danger
                busy={remove.isPending}
                onConfirm={() => remove.mutate("uninstall")}
                onCancel={() => setConfirm(null)}
              />
            ) : (
              <button className="library-manage-button is-danger" type="button" disabled={busy} onClick={() => setConfirm("uninstall")}>
                {`${t("library.manage.uninstallAction")}…`}
              </button>
            )}
            <p className="library-manage-help">{t("library.manage.uninstallHelp")}</p>
          </div>
        ) : null}
        <div className="library-manage-remove">
          {confirm === "remove" ? (
            <ConfirmRow
              question={t("library.manage.removeConfirm", { title })}
              confirmLabel={t("library.manage.removeAction")}
              busy={remove.isPending}
              onConfirm={() => remove.mutate("remove")}
              onCancel={() => setConfirm(null)}
            />
          ) : (
            <button className="library-manage-button" type="button" disabled={busy} onClick={() => setConfirm("remove")}>
              {`${t("library.manage.removeAction")}…`}
            </button>
          )}
          <p className="library-manage-help">{t("library.manage.removeHelp")}</p>
        </div>
      </section>

      <details className="library-manage-advanced">
        <summary>{t("library.manage.advanced")}</summary>
        <ContentEditor installation={installation} onSave={onSave} />
        <div className="library-manage-row">
          <ActionButton busy={rescan.isPending} disabled={busy} onClick={() => rescan.mutate()}>
            {t("library.manage.refreshFiles")}
          </ActionButton>
          <ActionButton busy={cleanup.isPending} disabled={busy} onClick={() => cleanup.mutate()}>
            {t("library.maintenance.cleanupAction")}
          </ActionButton>
        </div>
        <p className="library-manage-help">{t("library.maintenance.cleanupHelp")}</p>
        {cleanup.data ? (
          <p className="library-manage-help" role="status">
            {t("library.maintenance.cleanupResult", {
              staging: cleanup.data.removedStagingDirectories,
              repairs: cleanup.data.removedRepairDirectories,
              restored: cleanup.data.restoredSourceFiles,
            })}
          </p>
        ) : null}
      </details>
    </div>
  );
}

function ContentEditor({ installation, onSave }: { installation: Installation; onSave: Save }) {
  const { t } = usePresentation();
  const draft = buildInstallationReviewDraft(installation);
  const rows = contentRows(installation);
  const ordered = rows.length > 1;
  const update = (path: string, change: (value: ContentReviewValue) => ContentReviewValue) => {
    onSave((next) => ({
      ...next,
      content: { ...next.content, [path]: change(next.content[path] ?? emptyContentReview()) },
    }));
  };

  if (!rows.length) return <p className="library-manage-note">{t("library.noContent")}</p>;
  return (
    <div className="library-manage-table-wrap">
      <table className="library-manage-table">
        <thead>
          <tr>
            <th>{t("library.path")}</th>
            <th>{t("library.overrideType")}</th>
            {ordered ? <th>{t("library.order")}</th> : null}
            <th>{t("library.ignore")}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map(({ item, relativePath }) => {
            const value = draft.content[relativePath] ?? emptyContentReview();
            return (
              <tr className={value.ignored ? "is-ignored" : undefined} key={relativePath}>
                <td>
                  <code title={relativePath}>{relativePath}</code>
                  {item ? null : <small>{t("library.orphanedOverride")}</small>}
                </td>
                <td>
                  <select
                    value={value.mediaType ?? item?.mediaType ?? "unknown"}
                    aria-label={t("library.overrideType")}
                    onChange={(event) => {
                      const selected = event.target.value as MediaType;
                      update(relativePath, (current) => ({
                        ...current,
                        mediaType: item?.mediaType === selected ? null : selected,
                      }));
                    }}
                  >
                    {mediaTypes.map((mediaType) => (
                      <option value={mediaType} key={mediaType}>{t(mediaTypeMessageKey(mediaType))}</option>
                    ))}
                  </select>
                </td>
                {ordered ? (
                  <td>
                    <OrderInput
                      value={value.order}
                      label={t("library.orderFor", { path: relativePath })}
                      onCommit={(order) => update(relativePath, (current) => ({ ...current, order }))}
                    />
                  </td>
                ) : null}
                <td>
                  <input
                    type="checkbox"
                    checked={value.ignored}
                    aria-label={t("library.ignorePath", { path: relativePath })}
                    onChange={(event) => {
                      const ignored = event.target.checked;
                      update(relativePath, (current) => ({ ...current, ignored }));
                    }}
                  />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function OrderInput({ value, label, onCommit }: { value: string; label: string; onCommit: (value: string) => void }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <input
      type="number"
      min="0"
      value={text}
      aria-label={label}
      onChange={(event) => setText(event.target.value)}
      onBlur={() => {
        if (text !== value) onCommit(text);
      }}
    />
  );
}

function ConfirmRow({
  question,
  confirmLabel,
  danger = false,
  busy,
  onConfirm,
  onCancel,
}: {
  question: string;
  confirmLabel: string;
  danger?: boolean;
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = usePresentation();
  return (
    <div className="library-manage-confirm" role="alertdialog" aria-modal="false" aria-label={question}>
      <span>{question}</span>
      <button className={`library-manage-button ${danger ? "is-solid-danger" : "is-primary"}`} type="button" disabled={busy} onClick={onConfirm}>
        {busy ? <LoaderCircle className="library-spin" aria-hidden="true" /> : null}
        {confirmLabel}
      </button>
      <button className="library-manage-link" type="button" disabled={busy} onClick={onCancel}>{t("common.cancel")}</button>
    </div>
  );
}

function ActionButton({
  busy,
  disabled,
  icon,
  onClick,
  children,
}: {
  busy: boolean;
  disabled: boolean;
  icon?: ReactNode;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button className="library-manage-button" type="button" disabled={disabled} onClick={onClick}>
      {busy ? <LoaderCircle className="library-spin" aria-hidden="true" /> : icon}
      {children}
    </button>
  );
}

/** The launch candidate a one-step add starts with, mirroring recommended_launch(). */
export function recommendedKey(candidates: LaunchCandidate[]): string | null {
  const rank = { low: 0, medium: 1, high: 2 } as const;
  let best: LaunchCandidate | null = null;
  for (const candidate of candidates) {
    if (!best || rank[candidate.confidence] > rank[best.confidence]) best = candidate;
  }
  return best ? launchSelectionKey(best) : null;
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
