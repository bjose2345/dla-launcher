import { useQuery } from "@tanstack/react-query";
import { useMemo, useRef, useState } from "react";

import { formatByteSize } from "../../formatByteSize";
import type { MessageKey } from "../../i18n/catalogs";
import { usePresentation } from "../../preferences/PresentationProvider";
import { ImageGalleryModal } from "../catalog/ImageGalleryModal";
import { GalleryLayoutToggle, useGalleryLayout, WorkGallery } from "../catalog/WorkGallery";
import { DataRow, MetadataRow, WorkCodeCopy, WorkLocalActionButton } from "../catalog/WorkDetailPage";
import { WorkDetailSection } from "../catalog/WorkDetailSection";
import { useHeroBackdropParallax, WorkHeroAmbient } from "../catalog/WorkHeroAmbient";
import { WorkHeroImage } from "../catalog/WorkHeroImage";
import { libraryDisplayCode, libraryDisplayTitle } from "./libraryHome";
import { ownWorkDetailsKey } from "./OwnWorkDetailsEditor";
import type { Installation, LibraryGateway, OwnWorkKind, PreparedPackageInstallation } from "./types";

const KIND_LABELS: Record<OwnWorkKind, MessageKey> = {
  images: "library.own.kind.images",
  manga: "library.own.kind.manga",
  game: "library.own.kind.game",
  voice: "library.own.kind.voice",
  video: "library.own.kind.video",
  document: "library.own.kind.document",
};

/**
 * The page of a work outside the catalog, laid out like a catalog work's
 * page and filled with the metadata the user gave it.
 */
export function OwnWorkPage({
  installationId,
  gateway,
  onBack,
  onEditMetadata,
  onOpenScanner,
  onOpenInstallation,
  onOpenMedia,
  onOpenExternal,
}: {
  installationId: string;
  gateway: LibraryGateway;
  onBack: () => void;
  onEditMetadata: () => void;
  onOpenScanner: () => void;
  onOpenInstallation: (installationId: string, intent: "install" | "review") => void;
  onOpenMedia: (installationId: string) => void;
  onOpenExternal: (url: string) => Promise<void>;
}) {
  const { locale, theme, t } = usePresentation();
  const installation = useQuery({
    queryKey: ["library", "installation", installationId],
    queryFn: () => gateway.readInstallation(installationId),
  });
  const own = useQuery({
    queryKey: ownWorkDetailsKey(installationId),
    queryFn: () => gateway.readOwnWorkDetails(installationId),
  });
  const prepared = useQuery({
    queryKey: ["library", "prepared-package", installationId],
    queryFn: () => gateway.readPreparedPackage(installationId),
  });
  const [galleryIndex, setGalleryIndex] = useState<number | null>(null);
  const heroRef = useHeroBackdropParallax();
  const galleryRoot = useRef<HTMLDivElement>(null);
  const gallery = useGalleryLayout(galleryRoot);

  const details = own.data ?? undefined;
  const coverUrls = useMemo(
    () => (details?.cover ? [gateway.ownWorkArtworkUrl(installationId, details.cover)] : []),
    [details?.cover, gateway, installationId],
  );
  const samples = useMemo(
    () => (details?.samples ?? []).map((artwork) => [gateway.ownWorkArtworkUrl(installationId, artwork)]),
    [details?.samples, gateway, installationId],
  );
  const galleryImages = useMemo(() => [coverUrls, ...samples].filter((chain) => chain.length > 0), [coverUrls, samples]);

  if (installation.isPending || own.isPending) {
    return (
      <main className="work-detail-loading" aria-live="polite" aria-label={t("detail.loading")}>
        <div className="work-detail-loading-layout"><span /><span /></div>
      </main>
    );
  }
  if (installation.isError || !installation.data) {
    return (
      <main className="work-detail-state">
        <strong>{t("ownWork.loadFailed")}</strong>
        {installation.error ? (
          <span>{t("common.technicalDetail", { detail: installation.error instanceof Error ? installation.error.message : String(installation.error) })}</span>
        ) : null}
        <div><button type="button" onClick={onBack}>{t("library.back")}</button></div>
      </main>
    );
  }

  const title = libraryDisplayTitle(installation.data, undefined, locale !== "ja-JP");
  const code = libraryDisplayCode(installation.data, details);
  const releaseYear = /^\d{4}/.exec(details?.releaseDate ?? "")?.[0] ?? "";
  const totals = installedTotals(installation.data, prepared.data ?? null);
  const description = details?.description?.trim() ?? "";
  const kind = details?.kind ?? null;

  return (
    <main className="work-detail-shell own-work-page">
      <div className="work-detail-content">
        <section className="work-detail-hero" ref={heroRef}>
          <span className="work-hero-corner work-hero-corner-br" aria-hidden="true" />
          <div className="work-hero-stage">
            <span className="work-hero-corner work-hero-corner-tl" aria-hidden="true" />
            <button className="work-detail-back" type="button" onClick={onBack} aria-label={t("library.back")}>
              <span aria-hidden="true">←</span>
            </button>
            <WorkHeroImage title={title} urls={coverUrls} onOpen={() => setGalleryIndex(0)} />
          </div>
          <WorkHeroAmbient theme={theme} />
          <div className="work-detail-copy">
            <span className="work-panel-tab work-hero-enter work-hero-enter-1">
              {t("detail.resume")} <small>{t("detail.resumeSecondary")}</small>
            </span>
            <section className="work-resume-panel work-hero-enter work-hero-enter-1">
              <div className="work-title-line">
                <h1>{title}</h1>
                <div className="work-title-tools">
                  {releaseYear ? <span className="work-release-level">LV.<strong>{releaseYear}</strong></span> : null}
                </div>
              </div>
              <p className="work-circle">
                {details?.circle ?? t("detail.unknownCircle")}
                <small> · {t("library.own.yourWork")}</small>
              </p>
              {code ? <WorkCodeCopy code={code} /> : null}
              <MetadataRow
                label={t("detail.productFormat")}
                tone="pink"
                values={kind ? [{ key: kind, label: t(KIND_LABELS[kind]) }] : []}
                muted={!kind}
                emptyLabel={t("ownWork.notSet")}
              />
              <MetadataRow
                label={t("detail.genre")}
                tone="red"
                values={(details?.tags ?? []).map((tag) => ({ key: tag, label: tag }))}
                muted={!details?.tags.length}
                emptyLabel={t("ownWork.notSet")}
              />
            </section>

            <span className="work-panel-tab work-hero-enter work-hero-enter-4">
              {t("detail.workData")} <small>{t("detail.workDataSecondary")}</small>
            </span>
            <dl className="work-data-panel work-hero-enter work-hero-enter-4">
              <DataRow label={t("detail.released")} value={details?.releaseDate || t("ownWork.notSet")} />
              <DataRow label={t("ownWork.files")} value={totals.files.toLocaleString(locale)} />
              {totals.bytes > 0 ? <DataRow label={t("ownWork.size")} value={formatByteSize(totals.bytes, locale)} /> : null}
            </dl>

            <div className="work-source-actions work-hero-enter work-hero-enter-5">
              <button className="work-source-button" type="button" onClick={onEditMetadata}>
                {t("ownWork.editMetadata")} <span aria-hidden="true">→</span>
              </button>
              <WorkLocalActionButton
                installationId={installationId}
                gateway={gateway}
                onOpenScanner={onOpenScanner}
                onOpenInstallation={onOpenInstallation}
                onOpenMedia={onOpenMedia}
                onOpenExternal={onOpenExternal}
              />
            </div>
          </div>
        </section>

        <WorkDetailSection
          title={t("detail.gallery")}
          count={samples.length}
          headingAccessory={samples.length >= 3
            ? <GalleryLayoutToggle layout={gallery.layout} onChange={gallery.change} />
            : undefined}
        >
          {samples.length ? (
            <div ref={galleryRoot}>
              <WorkGallery
                images={samples}
                layout={gallery.layout}
                animate={gallery.animate}
                morphing={gallery.morphing}
                onOpen={(index) => setGalleryIndex(index + (coverUrls.length ? 1 : 0))}
              />
            </div>
          ) : (
            <p className="work-detail-empty">{t("ownWork.noSamples")}</p>
          )}
        </WorkDetailSection>
        <WorkDetailSection title={t("detail.description")}>
          {description ? (
            <div className="work-description-content own-work-description">
              {description.split(/\n{2,}/).map((paragraph, index) => <p key={index}>{paragraph}</p>)}
            </div>
          ) : (
            <p className="work-detail-empty">{t("ownWork.noDescription")}</p>
          )}
        </WorkDetailSection>
      </div>
      <ImageGalleryModal
        images={galleryImages}
        openIndex={galleryIndex}
        title={title}
        onClose={() => setGalleryIndex(null)}
      />
    </main>
  );
}

function installedTotals(
  installation: Installation,
  prepared: PreparedPackageInstallation | null,
): { files: number; bytes: number } {
  if (prepared) return { files: prepared.installedFileCount, bytes: prepared.installedBytes };
  const items = installation.detection.contentItems;
  return { files: items.length, bytes: items.reduce((total, item) => total + (item.sizeBytes ?? 0), 0) };
}
