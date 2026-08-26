import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  AlertTriangle,
  ArrowRight,
  AudioLines,
  AudioWaveform,
  Grid3X3,
  Library as LibraryIcon,
  List,
  ListChecks,
  LoaderCircle,
  Pause,
  Play,
  Rows3,
  RefreshCw,
  ShieldAlert,
  Smartphone,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";

import type { AndroidAppGateway, AndroidAppRuntimeState, AndroidAppView } from "../../android-package";
import type { CatalogDetailGateway, CatalogWork } from "../catalog";
import { usePresentation } from "../../preferences/PresentationProvider";
import {
  LibraryCollection,
  type LibraryCollectionEntry,
} from "./LibraryCollection";
import { LibraryArtwork } from "./LibraryArtwork";
import { LibraryPersonalization } from "./LibraryPersonalization";
import { LibraryShelves } from "./LibraryShelves";
import { LibraryTable } from "./LibraryTable";
import { effectiveIdentity } from "./libraryPresentation";
import { LibraryLensRail } from "./LibraryLensRail";
import { useMediaPlayback } from "./MediaPlaybackProvider";
import { isReaderAction, useImageReader } from "./ImageReaderProvider";
import { mediaItemName } from "./mediaSession";
import { NowPlayingBars } from "./NowPlayingBars";
import { playTimeLabel } from "./LibraryPlayTime";
import { TrackWaveform } from "./TrackWaveform";
import {
  featuredInstallationId,
  libraryContentKind,
  libraryDisplayCreator,
  libraryDisplayTitle,
  launchTotalsByInstallation,
  libraryLensCounts,
  matchesLibraryLens,
  resumeForInstallation,
  type LibraryLens,
  type LibraryView,
} from "./libraryHome";
import {
  installationPrimaryAction,
  isMediaLaunchAction,
  mediaActionMessageKey,
} from "./mediaSession";
import type {
  Installation,
  InstallationHealthReport,
  LaunchActionKind,
  LaunchActivity,
  LibraryGateway,
  LocalPersonalization,
  MediaSessionItem,
  PreparedPackageInstallation,
} from "./types";
import { launchActivityIsActive } from "./types";

const AUDIO_VISUALIZER_STORAGE_KEY = "dla.library.audio-visualizer";

type AudioVisualizerMode = "waveform" | "analyser";

function readAudioVisualizerMode(): AudioVisualizerMode {
  if (typeof window === "undefined") return "waveform";
  try {
    return window.localStorage.getItem(AUDIO_VISUALIZER_STORAGE_KEY) === "analyser"
      ? "analyser"
      : "waveform";
  } catch {
    return "waveform";
  }
}

export function LibraryPage({
  gateway,
  catalogGateway,
  onOpenReview,
  onOpenMedia,
  onOpenWork,
  androidAppGateway,
  onReinstallAndroidApp,
}: {
  gateway: LibraryGateway;
  androidAppGateway?: AndroidAppGateway;
  catalogGateway: CatalogDetailGateway;
  onOpenReview: (installationId: string) => void | Promise<void>;
  onOpenMedia: (installationId: string) => void | Promise<void>;
  onOpenWork: (code: string) => void | Promise<void>;
  onReinstallAndroidApp?: (workCode: string) => void | Promise<void>;
}) {
  const { includeUnreviewed, locale, t } = usePresentation();
  const playback = useMediaPlayback();
  const reader = useImageReader();
  const queryClient = useQueryClient();
  const [lens, setLens] = useState<LibraryLens>("all");
  const [view, setView] = useState<LibraryView>("shelves");
  const [refreshing, setRefreshing] = useState(false);
  const [confirmRemoveAndroidApp, setConfirmRemoveAndroidApp] = useState<string | null>(null);
  const previousActiveLaunches = useRef<Set<string>>(new Set());
  const shelves = useQuery({
    queryKey: ["library", "shelves"],
    queryFn: () => gateway.readShelves(),
  });
  const androidApps = useQuery({
    queryKey: ["library", "android-apps"],
    queryFn: () => androidAppGateway?.list() ?? Promise.resolve([]),
    enabled: androidAppGateway !== undefined,
    refetchInterval: androidAppGateway ? 15_000 : false,
  });
  const launches = useQuery({
    queryKey: ["library", "launches", "recent"],
    queryFn: () => gateway.listRecentLaunches(50),
    select: (activities) => activities.map(({ id, installationId, status }) => ({
      id,
      installationId,
      status,
    })),
    refetchInterval: (query) => query.state.data?.some((activity) => (
      launchActivityIsActive(activity.status)
    )) ? 750 : false,
  });
  const activeLaunchIds = (launches.data ?? []).flatMap((activity) => (
    launchActivityIsActive(activity.status) ? [activity.id] : []
  ));
  const activeLaunchKey = [...activeLaunchIds].sort().join("\0");

  useEffect(() => {
    const current = new Set(activeLaunchIds);
    const launchSettled = [...previousActiveLaunches.current].some((id) => !current.has(id));
    previousActiveLaunches.current = current;
    if (launchSettled) {
      void queryClient.invalidateQueries({ queryKey: ["library", "shelves"] });
    }
  }, [activeLaunchKey, queryClient]);
  const personalization = useQuery({
    queryKey: ["library", "personalization"],
    queryFn: () => gateway.readLocalPersonalization(),
  });
  const installations = shelves.data?.installations ?? [];
  const preparedInstallationIds = installations.flatMap((installation) => (
    installation.detection.packageInspection !== null ? [installation.id] : []
  ));
  const preparedPackages = useQuery({
    queryKey: ["library", "prepared-packages", preparedInstallationIds],
    queryFn: () => gateway.readPreparedPackages(preparedInstallationIds),
    enabled: preparedInstallationIds.length > 0,
    staleTime: 30_000,
  });
  const installationIds = installations.map((installation) => installation.id);
  const installationHealth = useQuery({
    queryKey: ["library", "installation-healths", installationIds],
    queryFn: () => gateway.readInstallationHealths(installationIds),
    enabled: installationIds.length > 0,
    staleTime: 30_000,
  });
  const androidAppViews = androidApps.data ?? [];
  const workCodes = [...new Set([
    ...installations.flatMap((installation) => {
    const code = effectiveIdentity(installation);
    return code ? [code] : [];
    }),
    ...androidAppViews.map((item) => item.association.workCode),
  ])];
  const catalogWorks = useQuery({
    queryKey: ["catalog-works", workCodes],
    queryFn: () => catalogGateway.readWorks(workCodes),
    enabled: workCodes.length > 0,
    staleTime: 5 * 60_000,
  });
  const preparedByInstallation = new Map<string, PreparedPackageInstallation | null>(
    (preparedPackages.data ?? []).map((prepared) => [prepared.installationId, prepared]),
  );
  const workByCode = new Map<string, CatalogWork>(
    (catalogWorks.data ?? []).map((work) => [work.code, work]),
  );
  const healthByInstallation = new Map<string, InstallationHealthReport>(
    (installationHealth.data ?? []).map((report) => [report.installationId, report]),
  );
  const totalsByInstallation = launchTotalsByInstallation(shelves.data?.launchTotals ?? []);
  const collectionEntries: LibraryCollectionEntry[] = installations.map((installation) => {
    const code = effectiveIdentity(installation);
    return {
      installation,
      work: code ? workByCode.get(code) : undefined,
      action: installationPrimaryAction(installation, preparedByInstallation.get(installation.id)),
      resume: shelves.data ? resumeForInstallation(shelves.data, installation.id) : null,
      latestLaunch: launches.data?.find((activity) => activity.installationId === installation.id) ?? null,
      launchTotals: totalsByInstallation.get(installation.id) ?? null,
      health: healthByInstallation.get(installation.id) ?? null,
    };
  });
  const entryByInstallation = new Map(collectionEntries.map((entry) => [
    entry.installation.id,
    entry,
  ]));
  const kindByInstallation = new Map(collectionEntries.map((entry) => [
    entry.installation.id,
    libraryContentKind(entry.installation, entry.action),
  ]));
  const countedEntries = includeUnreviewed
    ? collectionEntries
    : collectionEntries.filter((entry) => entry.installation.status !== "needs_review");
  const lensCounts = libraryLensCounts(countedEntries.flatMap((entry) => {
    const kind = kindByInstallation.get(entry.installation.id);
    return kind ? [kind] : [];
  }));
  const needsReviewLenses = new Set<LibraryLens>();
  for (const entry of countedEntries) {
    if (entry.installation.status !== "needs_review") continue;
    needsReviewLenses.add("all");
    const kind = kindByInstallation.get(entry.installation.id);
    if (kind && kind !== "other") needsReviewLenses.add(kind);
  }
  const visibleIds = new Set(collectionEntries.flatMap((entry) => (
    includeUnreviewed || entry.installation.status !== "needs_review" ? [entry.installation.id] : []
  )));
  const inLens = (installationId: string) => {
    if (!visibleIds.has(installationId)) return false;
    const kind = kindByInstallation.get(installationId);
    return kind === undefined ? lens === "all" : matchesLibraryLens(lens, kind);
  };
  const lensEntries = collectionEntries.filter((entry) => inLens(entry.installation.id));
  const lensInstallationIds = new Set(
    lensEntries.map((entry) => entry.installation.id),
  );
  const featuredId = shelves.data
    ? featuredInstallationId(shelves.data, lensInstallationIds)
    : null;
  const featured = lensEntries.find((entry) => entry.installation.id === featuredId)
    ?? lensEntries[0]
    ?? null;
  const libraryWorkKeys = new Set([
    ...installations.map((installation) => (
      effectiveIdentity(installation)?.toLocaleLowerCase() ?? `installation:${installation.id}`
    )),
    ...androidAppViews.map((item) => item.association.workCode.toLocaleLowerCase()),
  ]);
  const readyWorkKeys = new Set([
    ...installations.filter((installation) => installation.status === "ready").map((installation) => (
      effectiveIdentity(installation)?.toLocaleLowerCase() ?? `installation:${installation.id}`
    )),
    ...androidAppViews.filter((item) => item.runtime.state === "ready")
      .map((item) => item.association.workCode.toLocaleLowerCase()),
  ]);
  const readyCount = readyWorkKeys.size;
  const activeCount = shelves.data?.unfinished.length ?? 0;

  const refresh = async () => {
    if (refreshing) return;
    setRefreshing(true);
    try {
      await Promise.all([
        shelves.refetch(),
        androidApps.refetch(),
        launches.refetch(),
        personalization.refetch(),
        preparedPackages.refetch(),
        installationHealth.refetch(),
        catalogWorks.refetch(),
      ]);
    } finally {
      setRefreshing(false);
    }
  };
  const invalidateActivity = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["library", "launches"] }),
      queryClient.invalidateQueries({ queryKey: ["library", "shelves"] }),
    ]);
  };
  const launch = useMutation({
    mutationFn: (installationId: string) => gateway.launchInstallation(installationId),
    onSuccess: (activity) => {
      queryClient.setQueryData<LaunchActivity[]>(["library", "launches", "recent"], (current) => [
        activity,
        ...(current ?? []).filter((item) => item.id !== activity.id),
      ]);
    },
    onSettled: invalidateActivity,
  });
  const updateAndroidApp = (updated: AndroidAppView) => {
    queryClient.setQueryData<AndroidAppView[]>(["library", "android-apps"], (current) => (
      (current ?? []).map((item) => item.association.id === updated.association.id ? updated : item)
    ));
  };
  const launchAndroidApp = useMutation({
    mutationFn: (associationId: string) => {
      if (!androidAppGateway) throw new Error("Android app launch is unavailable");
      return androidAppGateway.launch(associationId);
    },
    onSuccess: updateAndroidApp,
  });
  const removeAndroidApp = useMutation({
    mutationFn: (associationId: string) => {
      if (!androidAppGateway) throw new Error("Android app association is unavailable");
      return androidAppGateway.remove(associationId);
    },
    onSuccess: (_, associationId) => {
      queryClient.setQueryData<AndroidAppView[]>(["library", "android-apps"], (current) => (
        (current ?? []).filter((item) => item.association.id !== associationId)
      ));
      setConfirmRemoveAndroidApp(null);
    },
  });
  const activate = (
    entry: LibraryCollectionEntry,
    requestedAction: LaunchActionKind | null = entry.action,
  ) => {
    if (!requestedAction) {
      void onOpenReview(entry.installation.id);
    } else if (requestedAction === "play_audio") {
      if (playback.session?.installationId === entry.installation.id) playback.toggle();
      else void playback.openWork(entry.installation.id);
    } else if (isReaderAction(requestedAction)) {
      void reader.open(entry.installation.id);
    } else if (isMediaLaunchAction(requestedAction)) {
      void onOpenMedia(entry.installation.id);
    } else if (requestedAction === "launch_executable") {
      if (launch.isPending && launch.variables === entry.installation.id) return;
      if (entry.latestLaunch && launchActivityIsActive(entry.latestLaunch.status)) return;
      launch.mutate(entry.installation.id);
    } else {
      void onOpenReview(entry.installation.id);
    }
  };
  const metadataErrors = [preparedPackages.error, catalogWorks.error];
  const blockedExecutableInstallations = new Set(collectionEntries.flatMap((entry) => (
    entry.action === "launch_executable"
      && entry.latestLaunch
      && launchActivityIsActive(entry.latestLaunch.status)
      ? [entry.installation.id]
      : []
  )));
  if (launch.isPending && launch.variables) {
    blockedExecutableInstallations.add(launch.variables);
  }

  return (
    <main className="library-page">
      <section className="library-home-heading">
        <div>
          <span className="library-eyebrow"><LibraryIcon aria-hidden="true" />{t("library.home.eyebrow")}</span>
          <h1>{t("library.title")}</h1>
          <p>{t("library.home.description")}</p>
        </div>
        <div className="library-home-summary">
          <span><strong>{libraryWorkKeys.size}</strong>{t("library.home.works")}</span>
          <span><strong>{readyCount}</strong>{t("library.home.ready")}</span>
          <span><strong>{activeCount}</strong>{t("library.home.inProgress")}</span>
          <div className="library-view-switch" role="group" aria-label={t("library.view.label")}>
            <button
              type="button"
              aria-pressed={view === "shelves"}
              onClick={() => setView("shelves")}
            >
              <Rows3 aria-hidden="true" />{t("library.view.shelves")}
            </button>
            <button
              type="button"
              aria-pressed={view === "grid"}
              onClick={() => setView("grid")}
            >
              <Grid3X3 aria-hidden="true" />{t("library.view.grid")}
            </button>
            <button
              type="button"
              aria-pressed={view === "list"}
              onClick={() => setView("list")}
            >
              <List aria-hidden="true" />{t("library.view.list")}
            </button>
          </div>
          <button
            className="library-home-refresh"
            type="button"
            title={t("library.refresh")}
            aria-label={t("library.refresh")}
            disabled={refreshing}
            onClick={() => { void refresh(); }}
          >
            {refreshing
              ? <LoaderCircle className="library-spin" aria-hidden="true" />
              : <RefreshCw aria-hidden="true" />}
          </button>
        </div>
      </section>

      {shelves.isPending ? <LibraryLoading /> : null}
      <LibraryErrors
        errors={[
          shelves.error,
          launches.error,
          launch.error,
          personalization.error,
          androidApps.error,
          launchAndroidApp.error,
          removeAndroidApp.error,
          ...metadataErrors,
        ]}
      />

      {androidAppViews.length ? (
        <AndroidAppLibrary
          items={androidAppViews}
          workByCode={workByCode}
          launchingId={launchAndroidApp.isPending ? launchAndroidApp.variables ?? null : null}
          removingId={removeAndroidApp.isPending ? removeAndroidApp.variables ?? null : null}
          confirmRemoveId={confirmRemoveAndroidApp}
          onLaunch={(associationId) => launchAndroidApp.mutate(associationId)}
          onConfirmRemove={setConfirmRemoveAndroidApp}
          onRemove={(associationId) => removeAndroidApp.mutate(associationId)}
          onOpenWork={onOpenWork}
          onReinstall={onReinstallAndroidApp}
        />
      ) : null}

      <div className="library-home-body">
        {installations.length ? (
          <LibraryLensRail
            lens={lens}
            counts={lensCounts}
            needsReview={needsReviewLenses}
            onSelect={setLens}
          />
        ) : null}

        <div className="library-home-content" data-library-kind={lens}>
          {featured ? (
            <LibraryFeature
              entry={featured}
              gateway={gateway}
              preferEnglish={locale !== "ja-JP"}
              busy={launch.isPending && launch.variables === featured.installation.id}
              onActivate={() => activate(featured)}
              onOpenReview={() => void onOpenReview(featured.installation.id)}
              onOpenWork={featured.work ? () => void onOpenWork(featured.work!.code) : undefined}
            />
          ) : null}

          {view === "list" ? (
            <LibraryTable
              entries={lensEntries}
              onActivate={activate}
              onOpenReview={onOpenReview}
            />
          ) : view === "grid" ? (
            <LibraryCollection
              entries={lensEntries}
              activatingInstallationId={launch.isPending ? launch.variables ?? null : null}
              onActivate={activate}
              onOpenReview={onOpenReview}
              onOpenWork={onOpenWork}
            />
          ) : (
            <>
              {shelves.data?.installations.length ? (
                <LibraryShelves
                  shelves={shelves.data}
                  workByCode={workByCode}
                  preparedByInstallation={preparedByInstallation}
                  inLens={inLens}
                  blockedExecutableInstallations={blockedExecutableInstallations}
                  onActivate={(installationId, action) => {
                    const entry = entryByInstallation.get(installationId);
                    if (entry) activate(entry, action);
                  }}
                />
              ) : null}

              {personalization.data && personalizationHasContent(personalization.data) ? (
                <LibraryPersonalization
                  gateway={gateway}
                  personalization={personalization.data}
                  onOpenWork={onOpenWork}
                  onOpenVoiceQueue={() => { void playback.openVoiceQueue(); }}
                />
              ) : null}
            </>
          )}
        </div>
      </div>

      {shelves.data?.installations.length === 0 && androidAppViews.length === 0 ? (
        <section className="library-empty">
          <LibraryIcon aria-hidden="true" />
          <h2>{t("library.emptyTitle")}</h2>
          <p>{t("library.emptyHelp")}</p>
        </section>
      ) : null}
    </main>
  );
}

function AndroidAppLibrary({
  items,
  workByCode,
  launchingId,
  removingId,
  confirmRemoveId,
  onLaunch,
  onConfirmRemove,
  onRemove,
  onOpenWork,
  onReinstall,
}: {
  items: AndroidAppView[];
  workByCode: Map<string, CatalogWork>;
  launchingId: string | null;
  removingId: string | null;
  confirmRemoveId: string | null;
  onLaunch: (associationId: string) => void;
  onConfirmRemove: (associationId: string | null) => void;
  onRemove: (associationId: string) => void;
  onOpenWork: (workCode: string) => void | Promise<void>;
  onReinstall?: (workCode: string) => void | Promise<void>;
}) {
  const { locale, t } = usePresentation();
  const preferEnglish = locale !== "ja-JP";
  return (
    <section className="library-android-apps" aria-labelledby="library-android-apps-title">
      <header>
        <span className="library-android-apps-mark"><Smartphone aria-hidden="true" /></span>
        <div>
          <h2 id="library-android-apps-title">{t("androidApp.libraryTitle")}</h2>
          <p>{t("androidApp.libraryHelp")}</p>
        </div>
        <strong>{items.length}</strong>
      </header>
      <ul>
        {items.map((item) => {
          const { association, runtime } = item;
          const work = workByCode.get(association.workCode);
          const title = work
            ? (preferEnglish && work.titleEnglish ? work.titleEnglish : work.title)
            : association.workCode;
          const busy = launchingId === association.id || removingId === association.id;
          const confirming = confirmRemoveId === association.id;
          return (
            <li key={association.id} data-state={runtime.state}>
              <span className="library-android-app-state" aria-hidden="true">
                {runtime.state === "signer_mismatch"
                  ? <ShieldAlert />
                  : runtime.state === "missing"
                    ? <X />
                    : <Smartphone />}
              </span>
              <div className="library-android-app-copy">
                <small>{title}</small>
                <h3>{runtime.applicationLabel ?? association.applicationLabel}</h3>
                <p>
                  <span>{association.packageName}</span>
                  <span aria-hidden="true"> · </span>
                  <span>{t("androidPackage.version", {
                    version: runtime.versionName ?? runtime.versionCode
                      ?? association.associatedVersionName ?? association.associatedVersionCode,
                  })}</span>
                </p>
                <strong className={`library-android-app-status is-${runtime.state}`}>
                  {t(androidAppStateKey(runtime.state))}
                </strong>
                {runtime.technicalDetail ? (
                  <details>
                    <summary>{t("androidPackage.errorDetails")}</summary>
                    <p>{runtime.technicalDetail}</p>
                  </details>
                ) : null}
              </div>
              <div className="library-android-app-actions">
                <button type="button" disabled={busy} onClick={() => void onOpenWork(association.workCode)}>
                  {t("androidApp.viewWork")}
                </button>
                {runtime.state === "ready" ? (
                  <button
                    className="is-primary"
                    type="button"
                    disabled={busy}
                    onClick={() => onLaunch(association.id)}
                  >
                    {launchingId === association.id
                      ? <LoaderCircle className="library-spin" aria-hidden="true" />
                      : <Play aria-hidden="true" />}
                    {t(launchingId === association.id ? "androidApp.launching" : "androidApp.launch")}
                  </button>
                ) : onReinstall ? (
                  <button type="button" disabled={busy} onClick={() => void onReinstall(association.workCode)}>
                    <RefreshCw aria-hidden="true" />{t("androidApp.reinstall")}
                  </button>
                ) : null}
                {confirming ? (
                  <>
                    <button type="button" disabled={busy} onClick={() => onConfirmRemove(null)}>
                      {t("common.cancel")}
                    </button>
                    <button className="is-danger" type="button" disabled={busy} onClick={() => onRemove(association.id)}>
                      {removingId === association.id
                        ? <LoaderCircle className="library-spin" aria-hidden="true" />
                        : <Trash2 aria-hidden="true" />}
                      {t("androidApp.confirmRemove")}
                    </button>
                  </>
                ) : (
                  <button type="button" disabled={busy} onClick={() => onConfirmRemove(association.id)}>
                    <Trash2 aria-hidden="true" />{t("androidApp.remove")}
                  </button>
                )}
              </div>
            </li>
          );
        })}
      </ul>
      <p className="library-android-app-footnote">{t("androidApp.removeHelp")}</p>
    </section>
  );
}

function androidAppStateKey(state: AndroidAppRuntimeState) {
  switch (state) {
    case "ready": return "androidApp.state.ready" as const;
    case "not_launchable": return "androidApp.state.notLaunchable" as const;
    case "missing": return "androidApp.state.missing" as const;
    case "signer_mismatch": return "androidApp.state.signerMismatch" as const;
    case "unavailable": return "androidApp.state.unavailable" as const;
  }
}

function personalizationHasContent(
  personalization: LocalPersonalization,
): boolean {
  return personalization.favorites.length > 0
    || personalization.becauseYou.length > 0
    || personalization.voiceMix.length > 0;
}

function LibraryFeature({
  entry,
  gateway,
  preferEnglish,
  busy,
  onActivate,
  onOpenReview,
  onOpenWork,
}: {
  entry: LibraryCollectionEntry;
  gateway: LibraryGateway;
  preferEnglish: boolean;
  busy: boolean;
  onActivate: () => void;
  onOpenReview: () => void;
  onOpenWork?: () => void;
}) {
  const { showPlayTime, t } = usePresentation();
  const playback = useMediaPlayback();
  const [seekPreview, setSeekPreview] = useState<number | null>(null);
  const [waveformDuration, setWaveformDuration] = useState<number | null>(null);
  const [visualizerMode, setVisualizerMode] = useState<AudioVisualizerMode>(
    readAudioVisualizerMode,
  );
  const currentHere = playback.session?.installationId === entry.installation.id;
  const playingHere = currentHere && playback.playing;
  const running = entry.latestLaunch !== null
    && launchActivityIsActive(entry.latestLaunch.status);
  const playbackFor = async (
    installationId: string,
    ordinal: number,
    positionSeconds = 0,
  ) => {
    if (playback.session?.installationId === installationId) {
      playback.selectOrdinal(ordinal, positionSeconds);
      return;
    }
    const opened = await playback.openWork(installationId);
    if (opened) playback.selectOrdinal(ordinal, positionSeconds);
  };
  const { installation, work, action, resume } = entry;
  const title = libraryDisplayTitle(installation, work, preferEnglish);
  const creator = libraryDisplayCreator(installation, work, preferEnglish);
  const kind = libraryContentKind(installation, action);
  const actionLabel = resume
    ? t("library.home.resume")
    : action && isMediaLaunchAction(action)
      ? t(mediaActionMessageKey(action))
      : action === "launch_executable"
        ? t("detail.play")
        : t("library.review");
  const featureItems = useQuery({
    queryKey: ["library", "media", "items", installation.id],
    queryFn: () => gateway.listMediaItems(installation.id),
    enabled: action === "play_audio",
    staleTime: 5 * 60_000,
  });
  const waveformOrdinal = currentHere && playback.item
    ? playback.item.ordinal
    : featureItems.data?.find((item) => item.relativePath === resume?.relativePath)?.ordinal
      ?? featureItems.data?.[0]?.ordinal;
  const waveformItem = featureItems.data?.find((item) => item.ordinal === waveformOrdinal);
  const waveformResume = waveformItem?.relativePath === resume?.relativePath ? resume : null;
  const timelineDuration = currentHere
    ? playback.durationSeconds
    : resume?.durationMs ? resume.durationMs / 1_000 : waveformDuration;
  const timelinePosition = seekPreview ?? (currentHere
    ? playback.positionSeconds
    : resume?.positionMs ? resume.positionMs / 1_000 : 0);
  const progress = timelineDuration && timelineDuration > 0
    ? Math.round(Math.max(0, Math.min(100, (timelinePosition / timelineDuration) * 100)))
    : null;

  useEffect(() => {
    setSeekPreview(null);
    setWaveformDuration(null);
  }, [installation.id, waveformOrdinal]);

  const commitSeek = (positionSeconds: number) => {
    setSeekPreview(null);
    if (currentHere) playback.seek(positionSeconds);
    else if (waveformOrdinal !== undefined) {
      void playbackFor(installation.id, waveformOrdinal, positionSeconds);
    }
  };

  const chooseVisualizer = (mode: AudioVisualizerMode) => {
    setVisualizerMode(mode);
    try {
      window.localStorage.setItem(AUDIO_VISUALIZER_STORAGE_KEY, mode);
    } catch {
      // The selected mode still applies for this session when storage is unavailable.
    }
  };

  return (
    <section className="library-feature" aria-label={t("library.home.featured")}>
      <div className="library-feature-art cover-hover-frame">
        <LibraryArtwork kind={kind} title={title} work={work} />
        <span className="library-feature-art-shade" aria-hidden="true" />
        {kind === "audio" ? (
          <div className="library-feature-visualizer-toggle">
            <button
              className={visualizerMode === "waveform" ? "is-active" : undefined}
              type="button"
              aria-pressed={visualizerMode === "waveform"}
              onClick={() => chooseVisualizer("waveform")}
            >
              <AudioWaveform aria-hidden="true" />
              {t("media.visualizer.waveform")}
            </button>
            <button
              className={visualizerMode === "analyser" ? "is-active" : undefined}
              type="button"
              aria-pressed={visualizerMode === "analyser"}
              onClick={() => chooseVisualizer("analyser")}
            >
              <AudioLines aria-hidden="true" />
              {t("media.visualizer.analyser")}
            </button>
          </div>
        ) : null}
        {kind === "audio" ? (
          <div className="library-feature-visualizer-stage">
            {waveformOrdinal !== undefined ? (
              <div
                className={`library-feature-visualizer-layer${visualizerMode === "waveform" ? " is-active" : ""}`}
                aria-hidden={visualizerMode !== "waveform"}
                inert={visualizerMode !== "waveform"}
              >
                <TrackWaveform
                  gateway={gateway}
                  installationId={installation.id}
                  ordinal={waveformOrdinal}
                  resume={waveformResume}
                  onStart={(ordinal, positionSeconds) => playbackFor(
                    installation.id,
                    ordinal,
                    positionSeconds,
                  )}
                  onSeekPreview={setSeekPreview}
                  onDuration={setWaveformDuration}
                />
              </div>
            ) : null}
            <div
              className={`library-feature-visualizer-layer is-analyser${visualizerMode === "analyser" ? " is-active" : ""}`}
              aria-hidden={visualizerMode !== "analyser"}
            >
              <NowPlayingBars
                installationId={installation.id}
                alwaysVisible
                liveSpectrum={visualizerMode === "analyser"}
              />
            </div>
          </div>
        ) : null}
      </div>
      <div className="library-feature-copy">
        <span className="library-feature-kicker">
          {resume ? <Play fill="currentColor" aria-hidden="true" /> : <Sparkles aria-hidden="true" />}
          {t(resume ? "library.home.continue" : "library.home.featured")}
        </span>
        <h2>{title}</h2>
        <p>{creator}</p>
        <div className="library-feature-meta">
          <span>{kindLabel(kind, t)}</span>
          <span>{installation.detection.contentItems.length} {t("library.home.items")}</span>
          {work?.releaseDate ? <span>{work.releaseDate.slice(0, 4)}</span> : null}
          {showPlayTime && entry.launchTotals ? (
            <span>{playTimeLabel(entry.launchTotals.totalDurationMs, t)}</span>
          ) : null}
        </div>
        <LibraryFeatureUpNext
          items={featureItems.data ?? []}
          enabled={action === "play_audio"}
          onPlay={(ordinal) => void playbackFor(installation.id, ordinal)}
        />
        {resume || currentHere || (kind === "audio" && timelineDuration) ? (
          <div className="library-feature-resume">
            <span>{currentHere
              ? playback.item?.relativePath
              : resume?.relativePath ?? waveformItem?.relativePath}</span>
            {progress !== null && timelineDuration ? (
              <input
                className="library-feature-progress"
                type="range"
                min={0}
                max={timelineDuration}
                step={0.1}
                value={timelinePosition}
                aria-label={t("media.seek")}
                aria-valuetext={t("library.shelf.progress", { percent: progress })}
                style={{ "--library-progress": `${progress}%` } as CSSProperties}
                onChange={(event) => setSeekPreview(Number(event.currentTarget.value))}
                onPointerUp={(event) => commitSeek(Number(event.currentTarget.value))}
                onPointerCancel={() => setSeekPreview(null)}
                onKeyUp={(event) => {
                  if (["ArrowLeft", "ArrowRight", "ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
                    commitSeek(Number(event.currentTarget.value));
                  }
                }}
                onBlur={(event) => {
                  if (seekPreview !== null) commitSeek(Number(event.currentTarget.value));
                }}
              />
            ) : null}
          </div>
        ) : null}
        <div className="library-feature-actions">
          <button
            className="library-feature-primary"
            type="button"
            disabled={busy || running}
            onClick={currentHere ? playback.toggle : onActivate}
          >
            {playingHere
              ? <Pause fill="currentColor" aria-hidden="true" />
              : action ? <Play fill="currentColor" aria-hidden="true" /> : <ListChecks aria-hidden="true" />}
            {busy
              ? t("detail.launching")
              : running
                ? t("library.launchStatus.running")
                : playingHere
                  ? t("media.pause")
                  : actionLabel}
          </button>
          {onOpenWork ? (
            <button className="library-feature-secondary" type="button" onClick={onOpenWork}>
              {t("library.home.details")}<ArrowRight aria-hidden="true" />
            </button>
          ) : (
            <button className="library-feature-secondary" type="button" onClick={onOpenReview}>
              {t("library.home.manage")}<ListChecks aria-hidden="true" />
            </button>
          )}
        </div>
      </div>
    </section>
  );
}

function kindLabel(
  kind: ReturnType<typeof libraryContentKind>,
  t: ReturnType<typeof usePresentation>["t"],
): string {
  switch (kind) {
    case "audio": return t("library.home.filterAudio");
    case "images": return t("library.home.filterImages");
    case "video": return t("library.home.filterVideo");
    case "documents": return t("library.home.filterDocuments");
    case "apps": return t("library.home.filterApps");
    default: return t("domain.media.unknown");
  }
}

function LibraryErrors({ errors }: { errors: Array<unknown> }) {
  const { t } = usePresentation();
  return errors.flatMap((error, index) => error ? [(
    <div className="library-callout library-callout-error" role="alert" key={`${String(error)}:${index}`}>
      <AlertTriangle aria-hidden="true" />
      {t("common.requestFailed", { error: String(error) })}
    </div>
  )] : []);
}

function LibraryLoading() {
  const { t } = usePresentation();
  return <div className="library-loading"><LoaderCircle className="library-spin" aria-hidden="true" />{t("library.loading")}</div>;
}


function LibraryFeatureUpNext({
  items,
  enabled,
  onPlay,
}: {
  items: MediaSessionItem[];
  enabled: boolean;
  onPlay: (ordinal: number) => void;
}) {
  const { t } = usePresentation();
  if (!enabled || !items.length) return null;

  return (
    <div className="library-feature-upnext">
      <h3>{t("library.home.upNext")}</h3>
      <ol>
        {items.slice(0, 4).map((item, index) => (
          <li key={item.ordinal}>
            <button type="button" onClick={() => onPlay(item.ordinal)}>
              <span>{String(index + 1).padStart(2, "0")}</span>
              <strong title={mediaItemName(item)}>{mediaItemName(item)}</strong>
            </button>
          </li>
        ))}
      </ol>
    </div>
  );
}
