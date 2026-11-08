import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  CircleHelp,
  Eraser,
  ExternalLink,
  FolderOpen,
  ListX,
  LoaderCircle,
  PackageOpen,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Undo2,
  Wrench,
  X,
} from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";

import { ProgressBar } from "../../app/ProgressBar";
import { Switch } from "../../app/Switch";
import { useModalDialog } from "../../app/useModalDialog";
import { displayPath } from "../../displayPath";
import { formatByteSize } from "../../formatByteSize";
import { launchActionMessageKey, mediaTypeMessageKey } from "../../i18n/domainLabels";
import { usePresentation } from "../../preferences/PresentationProvider";
import { CatalogWorkPicker } from "../catalog/CatalogWorkPicker";
import { catalogKeys } from "../catalog/catalogQueryKeys";
import { workTitle } from "../catalog/localizedNames";
import type { CatalogDetailGateway, CatalogWork } from "../catalog/types";
import type { DeliveryGateway } from "../delivery/types";
import { WorkCardImage } from "../catalog/WorkCardImage";
import type { ScanInstallGateway } from "../scanner/types";
import { scanInstallIsActive } from "../scanner/types";
import { acceptScanInstallView, useScanInstalls } from "../scanner/useScanInstalls";
import type { SearchGateway } from "../search/types";
import { GameTranslationTab } from "../translation/GameTranslationTab";
import { untranslatableLanguage } from "../translation/language";
import type { TranslationGateway } from "../translation/types";
import { InstalledFileTree, type InstalledFile } from "./InstalledFileTree";
import { libraryContentKind, libraryDisplayCreator, libraryDisplayTitle } from "./libraryHome";
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
  MediaPlayerKind,
  MediaType,
  PreparedPackageInstallation,
  OwnWorkDetails,
  VisualCppRuntime,
  WindowScale,
} from "./types";
import { packagePreparationPercent, usePackagePreparationProgress } from "./usePackagePreparationProgress";

import { manageTabs, type ManageTab } from "./types";
import { pluralText } from "../../i18n/pluralText";

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

const tabLabelKeys = {
  general: "library.manage.tab.general",
  files: "library.manage.tab.installedFiles",
  display: "library.manage.tab.display",
  translation: "library.manage.tab.translation",
} as const satisfies Record<ManageTab, string>;

/** A work the launcher can start: the only kind it can translate. */
/** A work that is a video: by what it starts with, what it holds, or its catalog category. */
function isVideoWork(installation: Installation, work: CatalogWork | undefined, own: OwnWorkDetails | undefined): boolean {
  if (own?.kind === "video") return true;
  if (work?.categories.some((category) => /video|動画/i.test(`${category.name} ${category.nameEnglish}`))) return true;
  const action = installation.overrides.preferredAction?.action
    ?? installation.detection.launchCandidates[0]?.action
    ?? null;
  return libraryContentKind(installation, action) === "video";
}

function isAudioWork(installation: Installation, work: CatalogWork | undefined, own: OwnWorkDetails | undefined): boolean {
  if (own?.kind === "voice") return true;
  if (work?.categories.some((category) => /voice|audio|音声|ボイス|ASMR/i.test(`${category.name} ${category.nameEnglish}`))) return true;
  const action = installation.overrides.preferredAction?.action
    ?? installation.detection.launchCandidates[0]?.action
    ?? null;
  return libraryContentKind(installation, action) === "audio";
}

function isGame(installation: Installation): boolean {
  return installation.detection.launchCandidates.length > 0
    || installation.detection.contentItems.some((item) => item.mediaType === "executable");
}

/** A work that starts a program, the only kind a resolution applies to. */
function isApp(installation: Installation): boolean {
  const action = installation.overrides.preferredAction?.action
    ?? installation.detection.launchCandidates[0]?.action
    ?? null;
  return libraryContentKind(installation, action) === "apps";
}

/**
 * A Library work's properties, as game launchers present them: General for
 * its name, catalog match and start file; Installed files for its folder,
 * verification and uninstalling; Translation for games. Each change saves as
 * it is made.
 */
export function LibraryManageDrawer({
  installationId,
  initialTab = "general",
  gateway,
  catalogGateway,
  searchGateway,
  installs,
  translationGateway,
  deliveryGateway,
  onClose,
  onOpenWork,
  onOpenTranslationSettings,
  onOpenExternal,
}: {
  installationId: string;
  initialTab?: ManageTab;
  gateway: LibraryGateway;
  catalogGateway: Pick<CatalogDetailGateway, "readWorks"> & Partial<Pick<CatalogDetailGateway, "read">>;
  searchGateway?: Pick<SearchGateway, "search">;
  installs?: ScanInstallGateway;
  /** Game translation, where the launcher can start games. */
  translationGateway?: TranslationGateway;
  /** Signs in to DLA for the Archive's translations, where downloads work. */
  deliveryGateway?: DeliveryGateway;
  onClose: () => void;
  onOpenWork?: (workCode: string) => void;
  onOpenTranslationSettings?: () => void;
  onOpenExternal?: (url: string) => Promise<void>;
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
    queryKey: catalogKeys.works(code ? [code] : []),
    queryFn: () => catalogGateway.readWorks([code!]),
    enabled: Boolean(code),
    staleTime: 5 * 60_000,
    select: (works) => works.find((item) => item.code === code),
  });
  // The work's languages, which decide whether it can be translated.
  const detail = useQuery({
    queryKey: catalogKeys.work(code),
    queryFn: () => catalogGateway.read!(code!),
    enabled: Boolean(code && catalogGateway.read && translationGateway),
    staleTime: 5 * 60_000,
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

  useModalDialog({ dialog: panel, onClose });
  useEffect(() => {
    setTab(initialTab);
  }, [initialTab, installationId]);

  const preferEnglish = locale !== "ja-JP";
  const current = installation.data;
  const title = current ? libraryDisplayTitle(current, work.data, preferEnglish) : "";
  const tabs = manageTabs.filter((value) => (
    value === "translation"
      ? Boolean(translationGateway && current && isGame(current))
      : value === "display"
        ? Boolean(gateway.readLaunchOptions && current && isApp(current))
        : true
  ));
  const untranslatable = untranslatableLanguage(detail.data?.supportedLanguages.map((language) => language.code));
  const shownTab = tabs.includes(tab) ? tab : "general";
  const launch = useMutation({
    mutationFn: () => gateway.launchInstallation(installationId),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["library", "launches"] }),
  });

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
          {saveState === "saving" ? <LoaderCircle className="app-spin" aria-hidden="true" /> : null}
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
          {tabs.map((value) => (
            <button
              type="button"
              role="tab"
              aria-selected={shownTab === value}
              className={shownTab === value ? "active" : undefined}
              key={value}
              onClick={() => setTab(value)}
            >
              {t(tabLabelKeys[value])}
            </button>
          ))}
        </div>

        <div className="library-manage-body" role="tabpanel">
          {saveError ? (
            <div className="app-callout" role="alert">
              <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(saveError) })}
            </div>
          ) : null}
          {installation.isPending ? (
            <p className="library-manage-note"><LoaderCircle className="app-spin" aria-hidden="true" />{t("library.loading")}</p>
          ) : null}
          {installation.error ? (
            <div className="app-callout" role="alert">
              <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(installation.error) })}
            </div>
          ) : null}
          {launch.error ? (
            <div className="app-callout" role="alert">
              <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(launch.error) })}
            </div>
          ) : null}
          {current && shownTab === "general" ? (
            <GeneralTab
              installation={current}
              work={work.data}
              own={own.data ?? undefined}
              launchOptions={gateway.readLaunchOptions ? (
                <>
                  {isGame(current) ? <JapaneseLocaleSection installationId={installationId} gateway={gateway} /> : null}
                  {gateway.selectMediaPlayer ? (
                    isVideoWork(current, work.data, own.data ?? undefined)
                      ? <ExternalPlayerSection installationId={installationId} gateway={gateway} kind="video" />
                      : isAudioWork(current, work.data, own.data ?? undefined)
                        ? <ExternalPlayerSection installationId={installationId} gateway={gateway} kind="audio" />
                        : null
                  ) : null}
                </>
              ) : null}
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
          {current && shownTab === "files" ? (
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
          {current && shownTab === "display" ? (
            <DisplayTab installationId={installationId} gateway={gateway} />
          ) : null}
          {current && shownTab === "translation" && translationGateway ? (
            detail.isLoading ? (
              <p className="library-manage-note"><LoaderCircle className="app-spin" aria-hidden="true" />{t("library.loading")}</p>
            ) : (
              <GameTranslationTab
                installationId={installationId}
                gateway={translationGateway}
                untranslatable={untranslatable}
                deliveryGateway={deliveryGateway}
                onOpenSettings={onOpenTranslationSettings}
                onOpenExternal={onOpenExternal}
                onPlay={() => launch.mutate()}
              />
            )
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
  launchOptions,
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
  /** How the work starts, last. */
  launchOptions?: ReactNode;
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
          placeholder={work ? workTitle(work, preferEnglish) : installationName(installation)}
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
                <strong>{workTitle(work, preferEnglish)}</strong>
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
            <button className="app-button" type="button" aria-expanded={matching} onClick={() => setMatching((open) => !open)}>
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

      {launchOptions}
    </div>
  );
}

// Microsoft's lasting links to the latest Visual C++ Redistributable.
const runtimeDownloads = {
  x64: "https://aka.ms/vs/17/release/vc_redist.x64.exe",
  x86: "https://aka.ms/vs/17/release/vc_redist.x86.exe",
} as const satisfies Record<VisualCppRuntime, string>;

const runtimeLabelKeys = {
  x64: "library.manage.japaneseLocaleRuntimeX64",
  x86: "library.manage.japaneseLocaleRuntimeX86",
} as const satisfies Record<VisualCppRuntime, string>;

/** Windows only: an older game started as if Windows were Japanese. */
function JapaneseLocaleSection({ installationId, gateway }: { installationId: string; gateway: LibraryGateway }) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const key = ["library", "launch-options", installationId] as const;
  const options = useQuery({ queryKey: key, queryFn: () => gateway.readLaunchOptions!(installationId) });
  const save = useMutation({
    mutationFn: (japaneseLocale: boolean) => gateway.saveLaunchOptions!(installationId, {
      japaneseLocale,
      videoPlayer: options.data?.videoPlayer ?? null,
      audioPlayer: options.data?.audioPlayer ?? null,
      windowScale: options.data?.windowScale ?? null,
      refreshRate: options.data?.refreshRate ?? null,
    }),
    onSuccess: (saved) => queryClient.setQueryData(key, saved),
  });
  if (!options.data?.japaneseLocaleAvailable) return null;
  const checked = save.isPending ? Boolean(save.variables) : options.data.japaneseLocale;
  // Turning it on the first time downloads Locale Remulator.
  const downloading = save.isPending && Boolean(save.variables);
  return (
    <section className="library-manage-field">
      <h3>{t("library.manage.japaneseLocale")}</h3>
      <Switch
        checked={checked}
        disabled={save.isPending}
        label={t("library.manage.japaneseLocaleSwitch")}
        help={t(downloading ? "library.manage.japaneseLocaleDownloading" : "library.manage.japaneseLocaleHelp")}
        onChange={(next) => save.mutate(next)}
      />
      {checked && options.data.missingRuntimes.length ? (
        <p className="library-manage-result is-problem">
          <AlertTriangle aria-hidden="true" />{t("library.manage.japaneseLocaleRuntime")}
          {gateway.openWebsite ? options.data.missingRuntimes.map((runtime) => (
            <button
              className="library-manage-link"
              type="button"
              key={runtime}
              onClick={() => void gateway.openWebsite!(runtimeDownloads[runtime])}
            >
              {t(runtimeLabelKeys[runtime])}
            </button>
          )) : null}
        </p>
      ) : null}
      {save.error ? (
        <p className="library-manage-result is-problem" role="alert">
          <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(save.error) })}
        </p>
      ) : null}
    </section>
  );
}

const windowSizes = [
  "1280x720", "1280x960", "1600x1200", "1920x1080", "1920x1440", "2560x1440", "2560x1920",
] as const satisfies readonly WindowScale[];

const fullScreenScales = {
  fit: "library.manage.windowScaleFit",
  integer: "library.manage.windowScaleInteger",
} as const satisfies Partial<Record<WindowScale, string>>;

const refreshRates = {
  60: "library.manage.refreshRate60",
  120: "library.manage.refreshRate120",
} as const;

const DEFAULT_REFRESH_RATE = 120;

type DisplayChange = { windowScale?: WindowScale | null; refreshRate?: number | null };

/**
 * Linux only, experimental: an app that opens small shown at a chosen size or
 * full screen through gamescope, paced at a chosen refresh rate.
 */
function DisplayTab({ installationId, gateway }: { installationId: string; gateway: LibraryGateway }) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const resolutionId = useId();
  const rateId = useId();
  const key = ["library", "launch-options", installationId] as const;
  const options = useQuery({ queryKey: key, queryFn: () => gateway.readLaunchOptions!(installationId) });
  const save = useMutation({
    mutationFn: (change: DisplayChange) => gateway.saveLaunchOptions!(installationId, {
      japaneseLocale: options.data?.japaneseLocale ?? false,
      videoPlayer: options.data?.videoPlayer ?? null,
      audioPlayer: options.data?.audioPlayer ?? null,
      windowScale: change.windowScale !== undefined ? change.windowScale : options.data?.windowScale ?? null,
      refreshRate: change.refreshRate !== undefined ? change.refreshRate : options.data?.refreshRate ?? null,
    }),
    onSuccess: (saved) => queryClient.setQueryData(key, saved),
  });
  if (!options.data) return null;
  const pending = save.isPending ? save.variables : undefined;
  const scale = pending?.windowScale !== undefined ? pending.windowScale : options.data.windowScale;
  const rate = (pending?.refreshRate !== undefined ? pending.refreshRate : options.data.refreshRate) ?? DEFAULT_REFRESH_RATE;
  return (
    <>
      <div className="app-callout">
        <AlertTriangle aria-hidden="true" />{t("library.manage.displayExperimental")}
      </div>
      {options.data.windowScaleAvailable ? (
        <>
          <section className="library-manage-field">
            <label htmlFor={resolutionId}><h3>{t("library.manage.windowScale")}</h3></label>
            <select
              id={resolutionId}
              className="library-manage-input"
              value={scale ?? ""}
              disabled={save.isPending}
              onChange={(event) => save.mutate({ windowScale: event.target.value === "" ? null : (event.target.value as WindowScale) })}
            >
              <option value="">{t("library.manage.windowScaleNative")}</option>
              {windowSizes.map((size) => (
                <option value={size} key={size}>{size.replace("x", " × ")}</option>
              ))}
              {(Object.keys(fullScreenScales) as (keyof typeof fullScreenScales)[]).map((value) => (
                <option value={value} key={value}>{t(fullScreenScales[value])}</option>
              ))}
            </select>
            <p className="library-manage-help">{t("library.manage.windowScaleHelp")}</p>
          </section>
          <section className="library-manage-field">
            <label htmlFor={rateId}><h3>{t("library.manage.refreshRate")}</h3></label>
            <select
              id={rateId}
              className="library-manage-input"
              value={rate}
              disabled={save.isPending || scale === null}
              onChange={(event) => save.mutate({ refreshRate: Number(event.target.value) })}
            >
              {(Object.keys(refreshRates) as unknown as (keyof typeof refreshRates)[]).map((value) => (
                <option value={value} key={value}>{t(refreshRates[value])}</option>
              ))}
            </select>
            <p className="library-manage-help">{t("library.manage.refreshRateHelp")}</p>
          </section>
        </>
      ) : (
        <p className="library-manage-note">{t("library.manage.displayNeedsGamescope")}</p>
      )}
      {save.error ? (
        <p className="library-manage-result is-problem" role="alert">
          <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(save.error) })}
        </p>
      ) : null}
    </>
  );
}

/** A player program of the user's for the work's videos or tracks, or the built-in one. */
function ExternalPlayerSection({
  installationId,
  gateway,
  kind,
}: {
  installationId: string;
  gateway: LibraryGateway;
  kind: MediaPlayerKind;
}) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const key = ["library", "launch-options", installationId] as const;
  const options = useQuery({ queryKey: key, queryFn: () => gateway.readLaunchOptions!(installationId) });
  const save = useMutation({
    mutationFn: (player: string | null) => gateway.saveLaunchOptions!(installationId, {
      japaneseLocale: options.data?.japaneseLocale ?? false,
      videoPlayer: kind === "video" ? player : options.data?.videoPlayer ?? null,
      audioPlayer: kind === "audio" ? player : options.data?.audioPlayer ?? null,
      windowScale: options.data?.windowScale ?? null,
      refreshRate: options.data?.refreshRate ?? null,
    }),
    onSuccess: (saved) => queryClient.setQueryData(key, saved),
  });
  const choose = useMutation({
    mutationFn: async () => {
      const chosen = await gateway.selectMediaPlayer!();
      if (chosen) await save.mutateAsync(chosen);
    },
  });
  if (!options.data) return null;
  const player = save.isPending
    ? save.variables ?? null
    : kind === "audio" ? options.data.audioPlayer : options.data.videoPlayer;
  return (
    <section className="library-manage-field">
      <h3>{t("library.manage.externalPlayer")}</h3>
      <div className="library-manage-location">
        <code className="library-manage-path" title={player ?? undefined}>
          {player ?? t("library.manage.externalPlayerBuiltIn")}
        </code>
        <button className="app-button" type="button" disabled={choose.isPending || save.isPending} onClick={() => choose.mutate()}>
          <FolderOpen aria-hidden="true" />{t("library.manage.externalPlayerChoose")}
        </button>
        {player ? (
          <button className="app-button" type="button" disabled={save.isPending} onClick={() => save.mutate(null)}>
            <X aria-hidden="true" />{t("library.manage.externalPlayerClear")}
          </button>
        ) : null}
      </div>
      <p className="library-manage-help">{t("library.manage.externalPlayerHelp")}</p>
      {save.error || choose.error ? (
        <p className="library-manage-result is-problem" role="alert">
          <AlertTriangle aria-hidden="true" />{t("common.requestFailed", { error: String(save.error ?? choose.error) })}
        </p>
      ) : null}
    </section>
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
  const [selected, setSelected] = useState<string | null>(null);
  const healthKey = ["library", "installation-health", installation.id] as const;
  const health = useQuery({
    queryKey: healthKey,
    queryFn: () => gateway.readInstallationHealth(installation.id),
  });
  const refreshWork = () => Promise.all([
    queryClient.invalidateQueries({ queryKey: ["library", "installations"] }),
    queryClient.invalidateQueries({ queryKey: ["library", "shelves"] }),
    queryClient.invalidateQueries({ queryKey: ["library", "installation-healths"] }),
    queryClient.invalidateQueries({ queryKey: ["library", "installation", installation.id] }),
  ]);
  const acceptReport = async (report: InstallationHealthReport) => {
    queryClient.setQueryData(healthKey, report);
    await refreshWork();
  };
  // File names an older install read wrongly; not for an archive still to install.
  const namesKey = ["library", "file-names", installation.id] as const;
  const names = useQuery({
    queryKey: namesKey,
    queryFn: () => gateway.checkFileNames!(installation.id),
    enabled: Boolean(gateway.checkFileNames) && !(installation.detection.packageInspection && !prepared),
  });
  const fixNames = useMutation({
    mutationFn: () => gateway.fixFileNames!(installation.id),
    onSuccess: async (fixed) => {
      queryClient.setQueryData(namesKey, fixed);
      await Promise.all([queryClient.invalidateQueries({ queryKey: healthKey }), refreshWork()]);
    },
  });
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
    || remove.isPending || fixNames.isPending;
  const error = health.error ?? verify.error ?? locate.error ?? rescan.error ?? repair.error ?? cleanup.error
    ?? install.error ?? browse.error ?? remove.error ?? fixNames.error;
  const wrongNames = names.data?.wrong ?? 0;
  const fixedNames = names.data?.fixed ?? 0;
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
  const draft = buildInstallationReviewDraft(installation);
  const rows = contentRows(installation);
  const selectedRow = rows.find((row) => row.relativePath === selected) ?? null;
  const files = useMemo<InstalledFile[]>(() => {
    const content = buildInstallationReviewDraft(installation).content;
    return installation.detection.contentItems.map((item) => {
      const value = content[item.relativePath] ?? emptyContentReview();
      return {
        relativePath: item.relativePath,
        mediaType: value.mediaType ?? item.mediaType,
        sizeBytes: item.sizeBytes,
        changed: isChanged(value),
        hidden: value.ignored,
      };
    });
  }, [installation]);
  // "Image 612, Audio 410, …": what the folder holds, most first.
  const kinds = useMemo(() => {
    const counts = new Map<MediaType, number>();
    for (const file of files) counts.set(file.mediaType, (counts.get(file.mediaType) ?? 0) + 1);
    const parts = [...counts]
      .sort((left, right) => right[1] - left[1])
      .map(([mediaType, count]) => `${t(mediaTypeMessageKey(mediaType))} ${count.toLocaleString(locale)}`);
    return new Intl.ListFormat(locale, { style: "narrow", type: "unit" }).format(parts);
  }, [files, locale, t]);
  const problem = report && ["missing_files", "modified_files", "inaccessible", "repairable"].includes(report.state);

  return (
    <div className="library-manage-stack">
      {error ? (
        <div className="app-callout" role="alert">
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
              <span><LoaderCircle className="app-spin" aria-hidden="true" />
                {percent === null ? t("library.manage.installing") : t("library.manage.installingPercent", { percent })}
              </span>
              <ProgressBar percent={percent} size="slim" />
            </span>
          ) : installs ? (
            <button className="app-button is-primary" type="button" disabled={install.isPending} onClick={() => install.mutate()}>
              {install.isPending ? <LoaderCircle className="app-spin" aria-hidden="true" /> : <PackageOpen aria-hidden="true" />}
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
            <button className="app-button" type="button" disabled={browse.isPending} onClick={() => browse.mutate()}>
              <FolderOpen aria-hidden="true" />{t("library.manage.browse")}
            </button>
          ) : null}
        </div>
        <p className="library-manage-help">
          {[
            pluralText(t, locale, fileCount, "library.manage.sizeOne", "library.manage.size", {
              size: formatByteSize(sizeBytes, locale),
            }),
            kinds,
          ].filter(Boolean).join(" · ")}
        </p>
      </section>

      <section className="library-manage-field">
        <h3>{t("library.manage.checkTitle")}</h3>
        <div className="library-manage-row">
          <ActionButton busy={verify.isPending} disabled={busy} icon={<ShieldCheck aria-hidden="true" />} onClick={() => verify.mutate()}>
            {t(verify.isPending ? "library.manage.verifying" : "library.manage.verify")}
          </ActionButton>
          <ActionButton busy={rescan.isPending} disabled={busy} icon={<RefreshCw aria-hidden="true" />} onClick={() => rescan.mutate()}>
            {t("library.manage.refreshFiles")}
          </ActionButton>
          <ActionButton busy={cleanup.isPending} disabled={busy} icon={<Eraser aria-hidden="true" />} onClick={() => cleanup.mutate()}>
            {t("library.manage.cleanUp")}
          </ActionButton>
        </div>
        <p className="library-manage-help">{t("library.manage.checkHelp")}</p>
        {verified && report?.state === "healthy" ? (
          <p className="library-manage-result is-ok" role="status">
            <CheckCircle2 aria-hidden="true" />
            {pluralText(t, locale, report.presentFiles, "library.manage.verifyOkOne", "library.manage.verifyOk")}
          </p>
        ) : null}
        {problem ? (
          <div className="library-manage-result is-problem" role="status">
            <AlertTriangle aria-hidden="true" />
            <span>
              {pluralText(t, locale, changedFiles, "library.manage.verifyProblemOne", "library.manage.verifyProblem")}
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
        {wrongNames > 0 ? (
          <div className="library-manage-result is-problem" role="status">
            <AlertTriangle aria-hidden="true" />
            <span>
              <strong>
                {pluralText(t, locale, wrongNames, "library.manage.namesWrongOne", "library.manage.namesWrong")}
              </strong>
              {" "}{t("library.manage.namesWrongHelp")}
            </span>
            <ActionButton busy={fixNames.isPending} disabled={busy} icon={<Wrench aria-hidden="true" />} onClick={() => fixNames.mutate()}>
              {t(fixNames.isPending ? "library.manage.fixingNames" : "library.manage.fixNames")}
            </ActionButton>
          </div>
        ) : null}
        {fixedNames > 0 ? (
          <p className="library-manage-result is-ok" role="status">
            <CheckCircle2 aria-hidden="true" />
            {pluralText(t, locale, fixedNames, "library.manage.namesFixedOne", "library.manage.namesFixed")}
          </p>
        ) : null}
        {cleanup.data ? (
          <p className="library-manage-help" role="status">
            {t("library.maintenance.cleanupResult", {
              staging: cleanup.data.removedStagingDirectories,
              repairs: cleanup.data.removedRepairDirectories,
              restored: cleanup.data.restoredSourceFiles,
            })}
          </p>
        ) : null}
      </section>

      <section className="library-manage-field">
        <h3>{t("library.manage.changeTitle")}</h3>
        <p className="library-manage-help">{t("library.manage.changeHelp")}</p>
        <InstalledFileTree files={files} selected={selectedRow ? selected : null} onSelect={setSelected} />
        {selectedRow ? (
          <FileEditor
            row={selectedRow}
            value={draft.content[selectedRow.relativePath] ?? emptyContentReview()}
            ordered={rows.length > 1}
            onSave={onSave}
            onClose={() => setSelected(null)}
          />
        ) : null}
        <h4 className="library-manage-subhead">{t("library.manage.changedTitle")}</h4>
        <ChangedFiles rows={rows} draft={draft} onSelect={setSelected} onSave={onSave} />
      </section>

      <section className="library-manage-field">
        <h3>{t("library.manage.removeTitle")}</h3>
        {confirm === "uninstall" ? (
          <ConfirmRow
            question={t("library.manage.uninstallConfirm")}
            confirmLabel={t("library.manage.uninstallAction")}
            danger
            busy={remove.isPending}
            onConfirm={() => remove.mutate("uninstall")}
            onCancel={() => setConfirm(null)}
          />
        ) : confirm === "remove" ? (
          <ConfirmRow
            question={t("library.manage.removeConfirm", { title })}
            confirmLabel={t("library.manage.removeAction")}
            busy={remove.isPending}
            onConfirm={() => remove.mutate("remove")}
            onCancel={() => setConfirm(null)}
          />
        ) : (
          <div className="library-manage-row">
            {report?.managed ? (
              <button className="app-button is-danger" type="button" disabled={busy} onClick={() => setConfirm("uninstall")}>
                <Trash2 aria-hidden="true" />{`${t("library.manage.uninstallAction")}…`}
              </button>
            ) : null}
            <button className="app-button" type="button" disabled={busy} onClick={() => setConfirm("remove")}>
              <ListX aria-hidden="true" />{`${t("library.manage.removeAction")}…`}
            </button>
          </div>
        )}
        <p className="library-manage-help">
          {t(report?.managed ? "library.manage.removeHelpBoth" : "library.manage.removeHelp")}
        </p>
      </section>
    </div>
  );
}

type ContentRow = { relativePath: string; item: ContentItem | null };

/** The file picked in the tree: its type, its place in a sequence, and whether the Library shows it. */
function FileEditor({
  row: { relativePath, item },
  value,
  ordered,
  onSave,
  onClose,
}: {
  row: ContentRow;
  value: ContentReviewValue;
  ordered: boolean;
  onSave: Save;
  onClose: () => void;
}) {
  const { t } = usePresentation();
  const update = (change: (current: ContentReviewValue) => ContentReviewValue) => onSave((next) => ({
    ...next,
    content: { ...next.content, [relativePath]: change(next.content[relativePath] ?? emptyContentReview()) },
  }));
  return (
    <div className="library-file-editor" role="group" aria-label={relativePath}>
      <div className="library-file-editor-head">
        <code title={relativePath}>{relativePath}</code>
        <button className="library-manage-close" type="button" aria-label={t("library.manage.close")} onClick={onClose}>
          <X aria-hidden="true" />
        </button>
      </div>
      {item ? null : <p className="library-manage-help">{t("library.orphanedOverride")}</p>}
      <div className="library-manage-row">
        <label className="library-file-editor-field">
          <span>{t("library.files.type")}</span>
          <select
            value={value.mediaType ?? item?.mediaType ?? "unknown"}
            onChange={(event) => {
              const selected = event.target.value as MediaType;
              update((current) => ({ ...current, mediaType: item?.mediaType === selected ? null : selected }));
            }}
          >
            {mediaTypes.map((mediaType) => (
              <option value={mediaType} key={mediaType}>{t(mediaTypeMessageKey(mediaType))}</option>
            ))}
          </select>
        </label>
        {ordered ? (
          <label className="library-file-editor-field">
            <span>{t("library.order")}</span>
            <OrderInput
              value={value.order}
              label={t("library.orderFor", { path: relativePath })}
              onCommit={(order) => update((current) => ({ ...current, order }))}
            />
          </label>
        ) : null}
        <label className="library-file-editor-check">
          <input
            type="checkbox"
            checked={value.ignored}
            onChange={(event) => {
              const ignored = event.target.checked;
              update((current) => ({ ...current, ignored }));
            }}
          />
          {t("library.files.hide")}
        </label>
      </div>
    </div>
  );
}

/** Only the files changed by hand, each with a way back to the launcher's own reading. */
function ChangedFiles({
  rows,
  draft,
  onSelect,
  onSave,
}: {
  rows: ContentRow[];
  draft: InstallationReviewDraft;
  onSelect: (relativePath: string) => void;
  onSave: Save;
}) {
  const { t } = usePresentation();
  const changed = rows.filter(({ relativePath }) => isChanged(draft.content[relativePath]));
  if (!changed.length) return <p className="library-manage-help">{t("library.manage.changedNone")}</p>;
  return (
    <ul className="library-changed-files">
      {changed.map(({ relativePath, item }) => {
        const value = draft.content[relativePath]!;
        const parts = relativePath.split("/");
        const what = [
          !item ? t("library.orphanedOverride") : null,
          value.mediaType && value.mediaType !== item?.mediaType ? t(mediaTypeMessageKey(value.mediaType)) : null,
          value.order.trim() ? t("library.files.orderValue", { order: value.order.trim() }) : null,
          value.ignored ? t("library.files.hidden") : null,
        ].filter(Boolean).join(" · ");
        return (
          <li key={relativePath}>
            <button className="library-changed-name" type="button" title={relativePath} onClick={() => onSelect(relativePath)}>
              <strong>{parts[parts.length - 1]}</strong>
              {parts.length > 1 ? <small>{parts.slice(0, -1).join("/")}</small> : null}
            </button>
            <span className="library-changed-what">{what}</span>
            <button
              className="app-button"
              type="button"
              onClick={() => onSave((next) => ({
                ...next,
                content: { ...next.content, [relativePath]: emptyContentReview() },
              }))}
            >
              <Undo2 aria-hidden="true" />{t("library.manage.undo")}
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function isChanged(value: ContentReviewValue | undefined): boolean {
  return Boolean(value && (value.mediaType !== null || value.ignored || value.order.trim() !== ""));
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
      <button className={`app-button ${danger ? "is-solid-danger" : "is-primary"}`} type="button" disabled={busy} onClick={onConfirm}>
        {busy ? <LoaderCircle className="app-spin" aria-hidden="true" /> : null}
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
    <button className="app-button" type="button" disabled={disabled} onClick={onClick}>
      {busy ? <LoaderCircle className="app-spin" aria-hidden="true" /> : icon}
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
