import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Heart, LoaderCircle, Smartphone } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

import { ImageGalleryModal, WorkGallery } from "./WorkGallery";
import { RelatedWorks } from "./RelatedWorks";
import type { CatalogDetailGateway, CatalogFacetGroup, CatalogWorkDetail } from "./types";
import { WorkAgeBadge } from "./WorkAgeBadge";
import { WorkFilesInfo } from "./WorkFilesInfo";
import { WorkDescription } from "./WorkDescription";
import { WorkDetailSection } from "./WorkDetailSection";
import { displayName, dlsiteWorkUrl, heroImageUrls, sampleImageChains } from "./workDetail";
import { WorkHeroImage } from "./WorkHeroImage";
import { WorkRatingPanel } from "./WorkRatingPanel";
import { WorkRecommendations } from "./WorkRecommendations";
import { usePresentation } from "../../preferences/PresentationProvider";
import { useCatalogFacetFilters } from "./CatalogFacetFiltersProvider";
import { setCatalogFacetState } from "./catalogFilters";
import { useHeroBackdropParallax, WorkHeroAmbient } from "./WorkHeroAmbient";
import { readWorkLibraryAction, type WorkLibraryAction } from "./workLibraryAction";
import { LaunchActivityList, formatDuration } from "../library/LaunchHistory";
import { isMediaLaunchAction, mediaActionMessageKey } from "../library/mediaSession";
import { useMediaPlayback } from "../library/MediaPlaybackProvider";
import type { LaunchActivity, LibraryGateway, WorkPreference } from "../library/types";
import { launchActivityIsActive } from "../library/types";

interface WorkDetailPageProps {
  code: string;
  gateway: CatalogDetailGateway;
  libraryGateway: LibraryGateway;
  onBack: () => void;
  onOpenCatalog: () => void;
  onOpenWork: (code: string) => void;
  onOpenScanner: () => void;
  onOpenInstallation: (installationId: string, intent: "install" | "review") => void;
  onOpenMedia: (installationId: string) => void;
  onInstallAndroidApp?: (workCode: string) => void;
  onOpenExternal: (url: string) => Promise<void>;
}

export function WorkDetailPage({
  code,
  gateway,
  libraryGateway,
  onBack,
  onOpenCatalog,
  onOpenWork,
  onOpenScanner,
  onOpenInstallation,
  onOpenMedia,
  onInstallAndroidApp,
  onOpenExternal,
}: WorkDetailPageProps) {
  const { t } = usePresentation();
  const query = useQuery({
    queryKey: ["catalog-work", code],
    queryFn: () => gateway.read(code),
  });

  if (query.isPending) return <WorkDetailLoading />;
  if (query.isError) {
    return (
      <main className="work-detail-state">
        <strong>{t("detail.loadFailed", { code })}</strong>
        <span>{t("common.technicalDetail", { detail: query.error instanceof Error ? query.error.message : String(query.error) })}</span>
        <div>
          <button type="button" onClick={onBack}>{t("detail.back")}</button>
          <button type="button" onClick={() => void query.refetch()}>{t("detail.tryAgain")}</button>
        </div>
      </main>
    );
  }

  return (
    <WorkDetail
      key={query.data.code}
      work={query.data}
      gateway={gateway}
      libraryGateway={libraryGateway}
      onBack={onBack}
      onOpenCatalog={onOpenCatalog}
      onOpenWork={onOpenWork}
      onOpenScanner={onOpenScanner}
      onOpenInstallation={onOpenInstallation}
      onOpenMedia={onOpenMedia}
      onInstallAndroidApp={onInstallAndroidApp}
      onOpenExternal={onOpenExternal}
    />
  );
}

function WorkDetail({
  work,
  gateway,
  libraryGateway,
  onBack,
  onOpenCatalog,
  onOpenWork,
  onOpenScanner,
  onOpenInstallation,
  onOpenMedia,
  onInstallAndroidApp,
  onOpenExternal,
}: {
  work: CatalogWorkDetail;
  gateway: CatalogDetailGateway;
  libraryGateway: LibraryGateway;
  onBack: () => void;
  onOpenCatalog: () => void;
  onOpenWork: (code: string) => void;
  onOpenScanner: () => void;
  onOpenInstallation: (installationId: string, intent: "install" | "review") => void;
  onOpenMedia: (installationId: string) => void;
  onInstallAndroidApp?: (workCode: string) => void;
  onOpenExternal: (url: string) => Promise<void>;
}) {
  const { locale, theme, t } = usePresentation();
  const queryClient = useQueryClient();
  const { setFilters } = useCatalogFacetFilters();
  const preferEnglish = locale !== "ja-JP";
  const sourceUrl = dlsiteWorkUrl(work);
  const [sourceError, setSourceError] = useState("");
  const [galleryIndex, setGalleryIndex] = useState<number | null>(null);
  const recommendations = useQuery({
    queryKey: ["catalog-recommendations", work.code],
    queryFn: () => gateway.readRecommendations(work.code),
    staleTime: 5 * 60 * 1_000,
  });
  const preference = useQuery({
    queryKey: ["library", "preference", work.code],
    queryFn: () => libraryGateway.readWorkPreference(work.code),
  });
  const favorite = preference.data?.preference === "favorite";
  const replaceFavorite = useMutation({
    mutationFn: () => libraryGateway.replaceWorkPreference(
      work.code,
      favorite ? null : "favorite",
    ),
    onSuccess: (updated) => {
      queryClient.setQueryData<WorkPreference | null>(
        ["library", "preference", work.code],
        updated,
      );
    },
    onSettled: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["library", "preference", work.code] }),
        queryClient.invalidateQueries({ queryKey: ["library", "personalization"] }),
      ]);
    },
  });
  const heroRef = useHeroBackdropParallax();
  const sampleImages = useMemo(() => sampleImageChains(work.sampleImageUrls), [work.sampleImageUrls]);
  const galleryImages = useMemo(
    () => [heroImageUrls(work), ...sampleImages].filter((chain) => chain.length > 0),
    [sampleImages, work],
  );
  const openSource = async () => {
    if (!sourceUrl) return;
    setSourceError("");
    try {
      await onOpenExternal(sourceUrl);
    } catch (error) {
      setSourceError(error instanceof Error ? error.message : String(error));
    }
  };
  const openFacet = useCallback((group: CatalogFacetGroup, key: string) => {
    if (!key.trim()) return;
    setFilters((current) => setCatalogFacetState(current, group, key, "include"));
    onOpenCatalog();
  }, [onOpenCatalog, setFilters]);
  const releaseYear = /^\d{4}/.exec(work.releaseDate)?.[0] ?? "";
  return (
    <main className="work-detail-shell">
      <div className="work-detail-content">
        <section className="work-detail-hero" ref={heroRef}>
          <span className="work-hero-corner work-hero-corner-tl" aria-hidden="true" />
          <span className="work-hero-corner work-hero-corner-br" aria-hidden="true" />
          <button className="work-detail-back" type="button" onClick={onBack} aria-label={t("detail.back")}>
            <span aria-hidden="true">←</span>
          </button>
          <WorkHeroImage title={work.title} urls={heroImageUrls(work)} onOpen={() => setGalleryIndex(0)} />
          <WorkHeroAmbient theme={theme} />
          <div className="work-detail-copy">
            <span className="work-panel-tab work-hero-enter work-hero-enter-1">
              {t("detail.resume")} <small>{t("detail.resumeSecondary")}</small>
            </span>
            <section className="work-resume-panel work-hero-enter work-hero-enter-1">
              <div className="work-title-line">
                <h1>{work.title}</h1>
                <div className="work-title-tools">
                  <button
                    className={`work-favorite-button${favorite ? " is-favorite" : ""}`}
                    type="button"
                    disabled={preference.isPending || replaceFavorite.isPending}
                    title={favorite ? t("detail.removeFavorite") : t("detail.addFavorite")}
                    aria-label={favorite ? t("detail.removeFavorite") : t("detail.addFavorite")}
                    aria-pressed={favorite}
                    onClick={() => replaceFavorite.mutate()}
                  >
                    {replaceFavorite.isPending
                      ? <LoaderCircle className="library-spin" aria-hidden="true" />
                      : <Heart aria-hidden="true" />}
                    <span>{t("detail.favorite")}</span>
                  </button>
                  {releaseYear && (
                    <span className="work-release-level">LV.<strong>{releaseYear}</strong></span>
                  )}
                </div>
              </div>
              {replaceFavorite.error ? (
                <p className="work-favorite-error" role="alert">
                  {t("detail.favoriteFailed", { error: String(replaceFavorite.error) })}
                </p>
              ) : null}
              {work.titleEnglish && work.titleEnglish !== work.title && (
                <p className="work-title-english">{work.titleEnglish}</p>
              )}
              <p className="work-circle">
                {work.circles.length
                  ? work.circles.map((circle, index) => (
                    <span key={`${circle.name}:${index}`}>
                      {index > 0 && " · "}
                      <button
                        type="button"
                        title={t("detail.filterBy", { label: displayName(circle.name, circle.nameEnglish, preferEnglish) })}
                        onClick={() => openFacet("circles", circle.name)}
                      >
                        {displayName(circle.name, circle.nameEnglish, preferEnglish)}
                      </button>
                    </span>
                  ))
                  : t("detail.unknownCircle")}
                {work.updatedDate && <small> · {t("detail.updated")} {work.updatedDate}</small>}
              </p>
              <p className="work-code">UID: {work.code}</p>

              <AgeMetadataRow age={work.ageRating} label={t("facet.age")} onSelect={() => openFacet("ages", work.ageRating)} />
              <MetadataRow
                label={t("detail.supportedLanguages")}
                values={work.supportedLanguages.map((language) => ({ key: language.code, label: displayName(language.name, language.nameEnglish, preferEnglish) }))}
                onSelect={(key) => openFacet("languages", key)}
                muted={!work.supportedLanguages.length}
              />
              <MetadataRow
                label={t("detail.productFormat")}
                tone="pink"
                values={work.categories.map((category) => ({ key: category.code, label: displayName(category.name, category.nameEnglish, preferEnglish) }))}
                onSelect={(key) => openFacet("categories", key)}
              />
              <MetadataRow
                label={t("detail.fileFormat")}
                tone="blue"
                values={work.fileFormats.map((format) => ({ key: format.code, label: displayName(format.name, format.nameEnglish, preferEnglish) }))}
                onSelect={(key) => openFacet("fileTypes", key)}
                muted={!work.fileFormats.length}
              />
              <MetadataRow
                label={t("detail.genre")}
                tone="red"
                values={work.tags.map((tag) => ({ key: tag.name, label: displayName(tag.name, tag.nameEnglish, preferEnglish) }))}
                onSelect={(key) => openFacet("genres", key)}
              />
              <MetadataRow
                label={t("detail.miscellaneous")}
                tone="green"
                values={work.miscellanies.map((item) => ({ key: item.code, label: displayName(item.name, item.nameEnglish, preferEnglish) }))}
                onSelect={(key) => openFacet("miscellanies", key)}
                muted={!work.miscellanies.length}
              />
            </section>

            <WorkRatingPanel rating={work.rating} />

            <span className="work-panel-tab work-hero-enter work-hero-enter-4">
              {t("detail.workData")} <small>{t("detail.workDataSecondary")}</small>
            </span>
            <dl className="work-data-panel work-hero-enter work-hero-enter-4">
              <DataRow label={t("detail.released")} value={work.releaseDate || t("detail.notCataloged")} />
              <DataRow label={t("detail.distribution")} value={work.releaseType || t("detail.notCataloged")} />
              <DataRow label={t("detail.source")} value={work.sourceCode || t("detail.notCataloged")} />
              <DataRow
                label={t("detail.record")}
                value={t(work.synthetic ? "detail.syntheticRecord" : "detail.catalogRecord")}
              />
            </dl>

            <div className="work-source-actions work-hero-enter work-hero-enter-5">
              <button
                className="work-source-button"
                type="button"
                disabled={!sourceUrl}
                onClick={() => void openSource()}
              >
                {t(sourceUrl ? "detail.viewDlsite" : "detail.sourceUnavailable")} <span aria-hidden="true">→</span>
              </button>
              {onInstallAndroidApp ? (
                <button
                  className="work-local-button work-local-button-install"
                  type="button"
                  onClick={() => onInstallAndroidApp(work.code)}
                >
                  <Smartphone aria-hidden="true" />
                  {t("detail.installAndroidApp")}
                </button>
              ) : null}
              <WorkLocalActionButton
                workCode={work.code}
                gateway={libraryGateway}
                onOpenScanner={onOpenScanner}
                onOpenInstallation={onOpenInstallation}
                onOpenMedia={onOpenMedia}
              />
            </div>
            {sourceError && (
              <p className="work-source-error" role="alert">
                {t("detail.sourceOpenFailed", { error: sourceError })}
              </p>
            )}
          </div>
        </section>

        <WorkDetailSection title={t("detail.relatedWorks")} count={work.relatedWorks.length}>
          <RelatedWorks works={work.relatedWorks} onOpenWork={onOpenWork} />
        </WorkDetailSection>
        <WorkDetailSection title={t("detail.filesInfo")} count={work.roms.length}>
          <WorkFilesInfo workCode={work.code} roms={work.roms} releaseDate={work.releaseDate} gateway={gateway} />
        </WorkDetailSection>
        <WorkDetailSection title={t("detail.gallery")} count={sampleImages.length}>
          <WorkGallery images={sampleImages} onOpen={(index) => setGalleryIndex(index + 1)} />
        </WorkDetailSection>
        <WorkDescription descriptions={work.descriptions} onOpenExternal={onOpenExternal} />
        <WorkRecommendations
          recommendations={recommendations.data}
          loading={recommendations.isPending}
          onOpenWork={onOpenWork}
        />
      </div>
      <ImageGalleryModal images={galleryImages} openIndex={galleryIndex} title={work.titleEnglish || work.title} onClose={() => setGalleryIndex(null)} />
    </main>
  );
}

function WorkLocalActionButton({
  workCode,
  gateway,
  onOpenScanner,
  onOpenInstallation,
  onOpenMedia,
}: {
  workCode: string;
  gateway: LibraryGateway;
  onOpenScanner: () => void;
  onOpenInstallation: (installationId: string, intent: "install" | "review") => void;
  onOpenMedia: (installationId: string) => void;
}) {
  const { t } = usePresentation();
  const playback = useMediaPlayback();
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: ["library", "work-action", workCode],
    queryFn: () => readWorkLibraryAction(gateway, workCode),
  });
  const action: WorkLibraryAction | null = query.data ?? null;
  const mediaAction = action?.kind === "play" && isMediaLaunchAction(action.action)
    ? action.action
    : null;
  const installationId = action?.kind === "play" && mediaAction === null
    ? action.installationId
    : null;
  const historyKey = ["library", "launches", "installation", installationId] as const;
  const history = useQuery({
    queryKey: historyKey,
    queryFn: () => installationId
      ? gateway.listInstallationLaunchHistory(installationId, 8)
      : Promise.resolve([]),
    enabled: installationId !== null,
    refetchInterval: (launchQuery) => launchQuery.state.data?.some((activity) => (
      launchActivityIsActive(activity.status)
    )) ? 750 : false,
  });
  const updateHistory = (updated: LaunchActivity) => {
    queryClient.setQueryData<LaunchActivity[]>(historyKey, (current) => [
      updated,
      ...(current ?? []).filter((activity) => activity.id !== updated.id),
    ]);
  };
  const launch = useMutation({
    mutationFn: (installationId: string) => gateway.launchInstallation(installationId),
    onSuccess: updateHistory,
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["library", "launches"] }),
  });
  const stopLaunch = useMutation({
    mutationFn: (activityId: string) => gateway.stopLaunch(activityId),
    onSuccess: updateHistory,
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["library", "launches"] }),
  });
  const activities = history.data?.length
    ? history.data
    : launch.data
      ? [launch.data]
      : [];
  const activeActivity = activities.find((activity) => launchActivityIsActive(activity.status)) ?? null;
  const latestActivity = activities[0] ?? null;
  const messageKey = query.isPending
    ? "detail.checkingLibrary"
    : query.isError
      ? "detail.libraryUnavailable"
      : action?.kind === "install"
        ? "detail.install"
        : action?.kind === "review"
          ? "detail.reviewInstallation"
          : action?.kind === "play"
            ? mediaAction === null
              ? "detail.play"
              : mediaActionMessageKey(mediaAction)
            : action?.kind === "installed"
              ? "detail.installed"
              : "detail.scanToInstall";
  const operationError = launch.error ?? stopLaunch.error;
  const stopping = stopLaunch.isPending || activeActivity?.status === "stopping";
  const disabled = query.isPending
    || query.isError
    || launch.isPending
    || stopping
    || activeActivity?.status === "starting"
    || action?.kind === "installed";
  const title = operationError
    ? t("detail.launchFailed", {
      error: operationError instanceof Error ? operationError.message : String(operationError),
    })
    : undefined;
  const activate = () => {
    if (activeActivity?.status === "running") {
      if (!stopLaunch.isPending) stopLaunch.mutate(activeActivity.id);
      return;
    }
    if (!action || disabled) return;
    if (action.kind === "scan") onOpenScanner();
    else if (action.kind === "install") onOpenInstallation(action.installationId, "install");
    else if (action.kind === "review") onOpenInstallation(action.installationId, "review");
    else if (action.kind === "play" && mediaAction === "play_audio") {
      void playback.openWork(action.installationId);
    }
    else if (action.kind === "play" && mediaAction !== null) onOpenMedia(action.installationId);
    else if (action.kind === "play") launch.mutate(action.installationId);
  };

  const buttonKey = activeActivity?.status === "running"
    ? "detail.stop"
    : stopping
      ? "detail.stopping"
      : launch.isPending || activeActivity?.status === "starting"
        ? "detail.launching"
        : messageKey;
  const launchFeedback = operationError
      ? t("detail.launchFailed", {
        error: operationError instanceof Error ? operationError.message : String(operationError),
      })
      : latestActivity
        ? launchActivityFeedback(latestActivity, t)
        : launch.isPending
          ? t("detail.launching")
          : "";

  return (
    <>
      <button
        className={`work-local-button work-local-button-${action?.kind ?? "loading"}${launch.isPending || stopping ? " work-local-button-launching" : ""}`}
        type="button"
        disabled={disabled}
        title={title}
        aria-busy={launch.isPending || stopping}
        onClick={activate}
      >
        {t(buttonKey)} <span aria-hidden="true">→</span>
      </button>
      {launchFeedback && (
        <p
          className={`work-launch-feedback${operationError || latestActivity?.status === "failed" ? " is-error" : ""}`}
          role={operationError || latestActivity?.status === "failed" ? "alert" : "status"}
        >
          {launchFeedback}
        </p>
      )}
      {activities.length ? (
        <div className="work-launch-history">
          <strong>{t("detail.recentLaunches")}</strong>
          <LaunchActivityList
            activities={activities.slice(0, 3)}
            compact
            stoppingActivityId={stopLaunch.variables ?? null}
            onStop={(activityId) => stopLaunch.mutate(activityId)}
          />
        </div>
      ) : null}
    </>
  );
}

function launchActivityFeedback(
  activity: LaunchActivity,
  t: ReturnType<typeof usePresentation>["t"],
): string {
  const processId = activity.processId ?? "—";
  switch (activity.status) {
    case "starting": return t("detail.launching");
    case "running": return t("detail.launchRunning", { processId });
    case "stopping": return t("detail.stopping");
    case "exited": return t("detail.launchExited", {
      duration: formatDuration(activity.durationMs ?? 0, t),
      code: activity.exitCode ?? 0,
    });
    case "failed": return t("detail.launchFailed", { error: activity.error ?? t("detail.launchUnknownError") });
    case "stopped": return t("detail.launchStopped", { duration: formatDuration(activity.durationMs ?? 0, t) });
    case "interrupted": return t("detail.launchInterrupted");
  }
}

function AgeMetadataRow({ age, label, onSelect }: { age: string; label: string; onSelect: () => void }) {
  const { t } = usePresentation();
  return (
    <div className="work-metadata-row tone-gold">
      <span className="work-metadata-label"><i aria-hidden="true">◇</i>{label}</span>
      <span className="work-metadata-values">
        {age.trim()
          ? (
            <button
              className="work-metadata-filter work-age-filter"
              type="button"
              title={t("detail.filterBy", { label: age })}
              onClick={onSelect}
            >
              <WorkAgeBadge age={age} />
            </button>
          )
          : <WorkAgeBadge age={age} />}
      </span>
    </div>
  );
}

function MetadataRow({
  label,
  values,
  onSelect,
  tone = "neutral",
  muted = false,
}: {
  label: string;
  values: Array<{ key: string; label: string }>;
  onSelect: (key: string) => void;
  tone?: string;
  muted?: boolean;
}) {
  const { t } = usePresentation();
  const shown = values.length ? values : [{ key: "", label: t("detail.notCataloged") }];
  return (
    <div className={`work-metadata-row tone-${tone}`}>
      <span className="work-metadata-label"><i aria-hidden="true">◇</i>{label}</span>
      <span className="work-metadata-values">
        {shown.map((value, index) => value.key
          ? (
            <button
              className="work-metadata-filter work-metadata-chip"
              type="button"
              title={t("detail.filterBy", { label: value.label })}
              onClick={() => onSelect(value.key)}
              key={`${value.key}:${index}`}
            >
              {value.label}
            </button>
          )
          : <span className={muted ? "muted" : ""} key={`empty:${index}`}>{value.label}</span>)}
      </span>
    </div>
  );
}

function DataRow({ label, value }: { label: string; value: string }) {
  return <div><dt>{label}</dt><dd>{value}</dd></div>;
}

function WorkDetailLoading() {
  const { t } = usePresentation();
  return (
    <main className="work-detail-loading" aria-live="polite" aria-label={t("detail.loading")}>
      <div className="work-detail-loading-layout"><span /><span /></div>
      <div className="route-loading-overlay"><i /><strong>{t("detail.loading")}</strong></div>
    </main>
  );
}
