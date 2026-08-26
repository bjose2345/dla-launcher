import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowRight, ListChecks, Play } from "lucide-react";
import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";

import type { CatalogWork } from "../catalog";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { MessageKey } from "../../i18n/catalogs";
import { LibraryArtwork } from "./LibraryArtwork";
import {
  libraryContentKind,
  libraryDisplayCreator,
  libraryDisplayTitle,
  type LibraryContentKind,
} from "./libraryHome";
import { mediaActionMessageKey } from "./mediaSession";
import {
  launchActivityIsActive,
  type Installation,
  type InstallationHealthReport,
  type InstallationHealthState,
  type LibraryGateway,
  type LaunchActionKind,
  type LaunchActivity,
  type LibraryLaunchTotals,
  type MediaResume,
} from "./types";
import { LibraryPlayTime } from "./LibraryPlayTime";
import { NowPlayingBars } from "./NowPlayingBars";

export interface LibraryCollectionEntry {
  installation: Installation;
  work?: CatalogWork;
  action: LaunchActionKind | null;
  resume: MediaResume | null;
  latestLaunch: Pick<LaunchActivity, "status"> | null;
  launchTotals: LibraryLaunchTotals | null;
  health: InstallationHealthReport | null;
}

export function LibraryCollection({
  entries,
  activatingInstallationId,
  onActivate,
  onOpenReview,
  onOpenWork,
}: {
  entries: LibraryCollectionEntry[];
  activatingInstallationId: string | null;
  onActivate: (entry: LibraryCollectionEntry) => void;
  onOpenReview: (installationId: string) => void | Promise<void>;
  onOpenWork: (code: string) => void | Promise<void>;
}) {
  const { t } = usePresentation();
  const gridRef = useRef<HTMLDivElement>(null);
  const [scrollElement, setScrollElement] = useState<HTMLElement | null>(null);
  const [columnCount, setColumnCount] = useState(1);
  const [scrollMargin, setScrollMargin] = useState(0);
  const rowCount = Math.ceil(entries.length / columnCount);
  const rowVirtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollElement,
    estimateSize: () => 330,
    overscan: 2,
    scrollMargin,
  });

  useLayoutEffect(() => {
    const grid = gridRef.current;
    if (!grid) return;
    const scroller = grid.closest<HTMLElement>(".library-page");
    setScrollElement(scroller);
    const measure = () => {
      const width = grid.getBoundingClientRect().width;
      setColumnCount(libraryGridColumnCount(width));
      if (scroller) {
        const gridBox = grid.getBoundingClientRect();
        const scrollerBox = scroller.getBoundingClientRect();
        setScrollMargin(gridBox.top - scrollerBox.top + scroller.scrollTop);
      }
    };
    measure();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(grid);
    if (scroller) observer?.observe(scroller);
    window.addEventListener("resize", measure);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, []);

  return (
    <section className="library-collection" aria-labelledby="library-collection-title">
      <header className="library-section-heading">
        <div>
          <span>{t("library.home.collectionEyebrow")}</span>
          <h2 id="library-collection-title">{t("library.home.collection")}</h2>
          <p>{t("library.home.collectionHelp")}</p>
        </div>
      </header>

      {entries.length ? (
        <div
          className="library-collection-grid library-collection-grid-virtual"
          ref={gridRef}
          style={{
            "--library-grid-columns": columnCount,
            height: `${rowVirtualizer.getTotalSize()}px`,
          } as CSSProperties}
        >
          {rowVirtualizer.getVirtualItems().map((virtualRow) => {
            const start = virtualRow.index * columnCount;
            return (
              <div
                className="library-collection-virtual-row"
                data-index={virtualRow.index}
                key={virtualRow.key}
                ref={rowVirtualizer.measureElement}
                style={{ transform: `translateY(${virtualRow.start - scrollMargin}px)` }}
              >
                {entries.slice(start, start + columnCount).map((entry) => (
                  <LibraryCollectionCard
                    entry={entry}
                    busy={activatingInstallationId === entry.installation.id}
                    onActivate={() => onActivate(entry)}
                    onOpenReview={() => void onOpenReview(entry.installation.id)}
                    onOpenWork={entry.work ? () => void onOpenWork(entry.work!.code) : undefined}
                    key={entry.installation.id}
                  />
                ))}
              </div>
            );
          })}
        </div>
      ) : (
        <p className="library-collection-empty">{t("library.home.filterEmpty")}</p>
      )}
    </section>
  );
}

export function libraryGridColumnCount(width: number): number {
  if (width <= 0) return 1;
  if (width < 360) return 1;
  if (width < 680) return 2;
  return Math.max(1, Math.floor((width + 16) / 226));
}

function LibraryCollectionCard({
  entry,
  busy,
  onActivate,
  onOpenReview,
  onOpenWork,
}: {
  entry: LibraryCollectionEntry;
  busy: boolean;
  onActivate: () => void;
  onOpenReview: () => void;
  onOpenWork?: () => void;
}) {
  const { locale, showPlayTime, t } = usePresentation();
  const preferEnglish = locale !== "ja-JP";
  const { action, installation, work, resume } = entry;
  const title = libraryDisplayTitle(installation, work, preferEnglish);
  const creator = libraryDisplayCreator(installation, work, preferEnglish);
  const kind = libraryContentKind(installation, action);
  const progress = resumePercent(resume);
  const running = entry.latestLaunch !== null
    && launchActivityIsActive(entry.latestLaunch.status);
  const activateLabel = action && action !== "launch_executable"
    ? t(mediaActionMessageKey(action))
    : action === "launch_executable"
      ? t("detail.play")
      : t("library.review");
  const openPrimary = onOpenWork ?? onActivate;

  return (
    <article className="library-collection-card" data-library-kind={kind}>
      <button className="library-collection-cover cover-hover-trigger" type="button" onClick={openPrimary}>
        <LibraryArtwork kind={kind} title={title} work={work} />
        <span className="library-collection-shade" aria-hidden="true" />
        <span className="library-collection-kind">{contentKindLabel(kind, t)}</span>
        <CollectionStateChip entry={entry} />
        <span className="library-collection-play" aria-hidden="true"><Play fill="currentColor" /></span>
        <NowPlayingBars installationId={installation.id} alwaysVisible={kind === "audio"} />
        {progress !== null ? (
          <span className="library-collection-progress" aria-label={t("library.shelf.progress", { percent: progress })}>
            <i style={{ width: `${progress}%` }} />
          </span>
        ) : null}
      </button>
      <div className="library-collection-copy">
        <strong title={title}>{title}</strong>
        <span title={creator}>{creator}</span>
        {showPlayTime && entry.launchTotals ? <LibraryPlayTime totals={entry.launchTotals} /> : null}
      </div>
      <footer>
        <button className="library-collection-action" type="button" disabled={busy || running} onClick={onActivate}>
          {action ? <Play fill="currentColor" aria-hidden="true" /> : <ListChecks aria-hidden="true" />}
          {busy
            ? t("detail.launching")
            : running
              ? t("library.launchStatus.running")
              : resume
                ? t("library.home.resume")
                : activateLabel}
        </button>
        <button className="library-collection-manage" type="button" title={t("library.home.manage")} onClick={onOpenReview}>
          <ListChecks aria-hidden="true" /><span>{t("library.home.manage")}</span>
        </button>
        {onOpenWork ? (
          <button className="library-collection-details" type="button" title={t("library.home.details")} onClick={onOpenWork}>
            <ArrowRight aria-hidden="true" /><span>{t("library.home.details")}</span>
          </button>
        ) : null}
      </footer>
    </article>
  );
}

function CollectionStateChip({ entry }: { entry: LibraryCollectionEntry }) {
  const { t } = usePresentation();
  const state = collectionState(entry);
  if (!state) return null;
  return (
    <span className={`library-collection-state library-collection-state-${state}`}>
      {state === "running" ? <i className="library-collection-state-dot" aria-hidden="true" /> : null}
      {t(stateLabels[state])}
    </span>
  );
}

type CollectionState =
  | "running"
  | "review"
  | "new"
  | Exclude<InstallationHealthState, "unknown" | "healthy" | "needs_review">;

const stateLabels: Record<CollectionState, MessageKey> = {
  running: "library.launchStatus.running",
  review: "library.status.needsReview",
  new: "library.state.new",
  missing_files: "library.maintenance.state.missing_files",
  modified_files: "library.maintenance.state.modified_files",
  moved: "library.maintenance.state.moved",
  inaccessible: "library.maintenance.state.inaccessible",
  repairable: "library.maintenance.state.repairable",
};

export function collectionState(entry: LibraryCollectionEntry): CollectionState | null {
  if (entry.latestLaunch && launchActivityIsActive(entry.latestLaunch.status)) return "running";
  if (entry.health && !["unknown", "healthy", "needs_review"].includes(entry.health.state)) {
    return entry.health.state as CollectionState;
  }
  if (entry.installation.status === "needs_review") return "review";
  if (!entry.resume && !entry.latestLaunch) return "new";
  return null;
}

function contentKindLabel(
  kind: LibraryContentKind,
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

function resumePercent(resume: MediaResume | null): number | null {
  if (!resume || resume.durationMs === null || resume.durationMs <= 0) return null;
  return Math.round(Math.max(0, Math.min(100, (resume.positionMs / resume.durationMs) * 100)));
}
