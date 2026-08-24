import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";

import { AnchoredPopover } from "../../app/AnchoredPopover";
import {
  catalogGridPageSize,
  catalogLinePreviewSize,
  catalogPageLinks,
  catalogPageRange,
  catalogPageSlice,
} from "./catalogPagination";
import {
  dateMarker,
  groupWorksByTimeline,
  type CatalogDateGroup,
  type CatalogViewMode,
} from "./catalogTimeline";
import { PersistentScrollbar } from "./PersistentScrollbar";
import type {
  CatalogFacetCatalog,
  CatalogFacetFilters,
  CatalogSort,
  CatalogTimeline,
  CatalogWork,
} from "./types";
import { useGrabScroll } from "./useGrabScroll";
import { WorkCardImage } from "./WorkCardImage";
import {
  CatalogActiveFilters,
  CatalogFiltersDrawer,
  CatalogFilterTrigger,
} from "./CatalogFiltersDrawer";
import { catalogFacetFilterKey } from "./catalogFilters";
import { usePresentation } from "../../preferences/PresentationProvider";

const VIEW_STORAGE_KEY = "dla-launcher:catalog-view";

interface CatalogResultsProps {
  works: CatalogWork[];
  total: number;
  sort: CatalogSort;
  timeline: CatalogTimeline;
  facets: CatalogFacetCatalog;
  facetFilters: CatalogFacetFilters;
  englishLabels: boolean;
  hasMore: boolean;
  fetchingMore: boolean;
  onLoadMore: () => void | Promise<unknown>;
  onOpenWork?: (code: string) => void | Promise<void>;
  onTimelineChange: (timeline: CatalogTimeline) => void;
  onSortChange?: (sort: CatalogSort) => void;
  onFacetFiltersChange: (filters: CatalogFacetFilters) => void;
}

export function CatalogResults({
  works,
  total,
  sort,
  timeline,
  facets,
  facetFilters,
  englishLabels,
  hasMore,
  fetchingMore,
  onLoadMore,
  onOpenWork,
  onTimelineChange,
  onSortChange,
  onFacetFiltersChange,
}: CatalogResultsProps) {
  const { locale, t } = usePresentation();
  const [view, setView] = useState<CatalogViewMode>(storedViewMode);
  const [gridPage, setGridPage] = useState(1);
  const [expandedDays, setExpandedDays] = useState<Set<string>>(() => new Set());
  const [filtersOpen, setFiltersOpen] = useState(false);
  const openFilters = useCallback(() => setFiltersOpen(true), []);
  const closeFilters = useCallback(() => setFiltersOpen(false), []);
  const lineGroups = useMemo(
    () => groupWorksByTimeline(works, timeline, sort),
    [sort, timeline, works],
  );
  const pageWorks = useMemo(() => catalogPageSlice(works, gridPage), [gridPage, works]);
  const gridGroups = useMemo(
    () => isChronologicalSort(sort) ? groupWorksByTimeline(pageWorks, timeline, sort) : null,
    [pageWorks, sort, timeline],
  );
  const dayCounts = useMemo(
    () => new Map(lineGroups.map((group) => [group.key, group.works.length])),
    [lineGroups],
  );
  const totalPages = Math.max(1, Math.ceil(total / catalogGridPageSize));
  const expectedLoaded = Math.min(total, gridPage * catalogGridPageSize);
  const gridPageReady = works.length >= expectedLoaded || !hasMore;
  const resultIdentity = `${timeline}:${sort}:${catalogFacetFilterKey(facetFilters)}:${total}:${works.slice(0, 3).map((work) => work.code).join(":")}`;
  const viewport = useRef<HTMLDivElement>(null);
  useGrabScroll(viewport);
  const virtualizer = useVirtualizer({
    count: view === "line" ? lineGroups.length : 0,
    getScrollElement: () => viewport.current,
    estimateSize: () => 620,
    overscan: 1,
  });
  const virtualGroups = virtualizer.getVirtualItems();

  useEffect(() => {
    window.localStorage.setItem(VIEW_STORAGE_KEY, view);
    viewport.current?.scrollTo({ top: 0 });
    virtualizer.measure();
  }, [view, virtualizer]);

  useEffect(() => {
    setGridPage(1);
    setExpandedDays((current) => current.size > 0 ? new Set() : current);
    viewport.current?.scrollTo({ top: 0 });
  }, [resultIdentity]);

  useEffect(() => {
    if (gridPage > totalPages) setGridPage(1);
  }, [gridPage, totalPages]);

  useEffect(() => {
    if (view !== "grid" || gridPageReady || fetchingMore || !hasMore) return;
    void onLoadMore();
  }, [fetchingMore, gridPageReady, hasMore, onLoadMore, view]);

  useLoadMore(
    view === "line" ? virtualGroups.at(-1)?.index : undefined,
    lineGroups.length,
    hasMore,
    fetchingMore,
    onLoadMore,
  );

  const changePage = (page: number) => {
    const next = Math.min(Math.max(page, 1), totalPages);
    if (next === gridPage) return;
    setGridPage(next);
    viewport.current?.scrollTo({ top: 0, behavior: "smooth" });
  };

  return (
    <section className="catalog-results catalog-timeline" aria-label={t("catalog.totalAria", { count: total.toLocaleString(locale) })}>
      <CatalogViewBar
        view={view}
        sort={sort}
        timeline={timeline}
        loaded={works.length}
        total={total}
        facetFilters={facetFilters}
        onChange={setView}
        onSortChange={onSortChange}
        onTimelineChange={onTimelineChange}
        onOpenFilters={openFilters}
      />
      <CatalogActiveFilters
        facets={facets}
        filters={facetFilters}
        onChange={onFacetFiltersChange}
      />
      <div className="catalog-scroll-frame">
        <div
          className="catalog-scroll catalog-timeline-scroll"
          id="catalog-timeline-scroll"
          ref={viewport}
        >
          {works.length === 0 ? (
            <section className="catalog-empty inline">
              <strong>{t("catalog.noMatches")}</strong>
              <span>{t("catalog.changeFilters")}</span>
            </section>
          ) : view === "grid" ? (
            <CatalogGridView
              groups={gridGroups}
              works={pageWorks}
              dayCounts={dayCounts}
              englishLabels={englishLabels}
              page={gridPage}
              total={total}
              totalPages={totalPages}
              ready={gridPageReady}
              onChangePage={changePage}
              onOpenWork={onOpenWork}
            />
          ) : (
            <div className="virtual-space" style={{ height: virtualizer.getTotalSize() }}>
              {virtualGroups.map((virtualGroup) => {
                const group = lineGroups[virtualGroup.index];
                if (!group) return null;
                return (
                  <CatalogLineGroup
                    group={group}
                    englishLabels={englishLabels}
                    expanded={expandedDays.has(group.key)}
                    onExpand={() => setExpandedDays((current) => new Set(current).add(group.key))}
                    onOpenWork={onOpenWork}
                    dataIndex={virtualGroup.index}
                    key={group.key}
                    measure={virtualizer.measureElement}
                    offset={virtualGroup.start}
                  />
                );
              })}
            </div>
          )}
          {fetchingMore && <p className="loading-more">{t("catalog.loadingMore")}</p>}
        </div>
        <PersistentScrollbar
          viewport={viewport}
          controls="catalog-timeline-scroll"
          label={t("catalog.scrollPosition")}
        />
      </div>
      <CatalogFiltersDrawer
        open={filtersOpen}
        facets={facets}
        filters={facetFilters}
        onChange={onFacetFiltersChange}
        onClose={closeFilters}
      />
    </section>
  );
}

export function CatalogViewBar({
  view,
  sort,
  timeline,
  loaded,
  total,
  facetFilters,
  onChange,
  onSortChange,
  onTimelineChange,
  onOpenFilters,
}: {
  view: CatalogViewMode;
  sort: CatalogSort;
  timeline: CatalogTimeline;
  loaded: number;
  total: number;
  facetFilters: CatalogFacetFilters;
  onChange: (view: CatalogViewMode) => void;
  onSortChange?: (sort: CatalogSort) => void;
  onTimelineChange: (timeline: CatalogTimeline) => void;
  onOpenFilters: () => void;
}) {
  const { locale, t } = usePresentation();
  return (
    <div className="catalog-view-bar">
      <div className="catalog-view-toggle" aria-label={t("catalog.view")}>
        <ViewButton view="grid" selected={view === "grid"} onSelect={onChange} />
        <ViewButton view="line" selected={view === "line"} onSelect={onChange} />
      </div>
      <span className="catalog-view-divider" aria-hidden="true" />
      <TimelineSelector value={timeline} onChange={onTimelineChange} />
      {onSortChange && <SortSelector value={sort} onChange={onSortChange} />}
      <CatalogFilterTrigger filters={facetFilters} onOpen={onOpenFilters} />
      <span className="catalog-view-count">{t("catalog.loaded", {
        loaded: loaded.toLocaleString(locale),
        total: total.toLocaleString(locale),
      })}</span>
    </div>
  );
}

const sortOptions: Array<{
  value: CatalogSort;
  label: "catalog.newest" | "catalog.oldest" | "catalog.titleAscending" | "catalog.titleDescending" | "catalog.mostFavorited";
}> = [
  { value: "release_desc", label: "catalog.newest" },
  { value: "release_asc", label: "catalog.oldest" },
  { value: "title_asc", label: "catalog.titleAscending" },
  { value: "title_desc", label: "catalog.titleDescending" },
  { value: "favorites", label: "catalog.mostFavorited" },
];

function SortSelector({
  value,
  onChange,
}: {
  value: CatalogSort;
  onChange: (sort: CatalogSort) => void;
}) {
  const { t } = usePresentation();
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const selectedLabel = sortOptions.find((option) => option.value === value)?.label
    ?? "catalog.newest";

  return (
    <div className="catalog-timeline-selector catalog-sort-selector">
      <button
        ref={triggerRef}
        className="catalog-timeline-basis catalog-sort-basis"
        type="button"
        aria-expanded={open}
        aria-haspopup="menu"
        onClick={() => setOpen((current) => !current)}
      >
        <SortIcon />
        <small>{t("catalog.sort")}</small>
        <strong>{t(selectedLabel)}</strong>
        <ChevronIcon />
      </button>
      {open && (
        <AnchoredPopover
          anchorRef={triggerRef}
          className="catalog-timeline-menu catalog-sort-menu"
          role="menu"
          ariaLabel={t("catalog.sortBy")}
          maximumWidth={242}
          onClose={() => setOpen(false)}
        >
          <span>{t("catalog.sortBy")}</span>
          {sortOptions.map((option) => (
            <button
              className={option.value === value ? "active" : ""}
              type="button"
              role="menuitemradio"
              aria-checked={option.value === value}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
              }}
              key={option.value}
            >
              <SortOptionIcon sort={option.value} />
              {t(option.label)}
              {option.value === value && <CheckIcon />}
            </button>
          ))}
        </AnchoredPopover>
      )}
    </div>
  );
}

const timelineOptions: Array<{
  value: CatalogTimeline;
  menuLabel: "catalog.dateAdded" | "catalog.dateReleased" | "catalog.dateUpdated";
}> = [
  { value: "added", menuLabel: "catalog.dateAdded" },
  { value: "release", menuLabel: "catalog.dateReleased" },
  { value: "updated", menuLabel: "catalog.dateUpdated" },
];

function TimelineSelector({
  value,
  onChange,
}: {
  value: CatalogTimeline;
  onChange: (timeline: CatalogTimeline) => void;
}) {
  const { t } = usePresentation();
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const selectedLabel = value === "added"
    ? t("catalog.added")
    : value === "updated"
      ? t("catalog.updated")
      : t("catalog.released");

  return (
    <div className="catalog-timeline-selector">
      <button
        ref={triggerRef}
        className="catalog-timeline-basis"
        type="button"
        aria-expanded={open}
        aria-haspopup="menu"
        onClick={() => setOpen((current) => !current)}
      >
        <CalendarIcon />
        <small>{t("catalog.timeline")}</small>
        <strong>{selectedLabel}</strong>
        <ChevronIcon />
      </button>
      {open && (
        <AnchoredPopover
          anchorRef={triggerRef}
          className="catalog-timeline-menu"
          role="menu"
          ariaLabel={t("catalog.timeline")}
          maximumWidth={220}
          onClose={() => setOpen(false)}
        >
          <span>{t("catalog.timeline")}</span>
          {timelineOptions.map((option) => (
            <button
              className={option.value === value ? "active" : ""}
              type="button"
              role="menuitemradio"
              aria-checked={option.value === value}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
              }}
              key={option.value}
            >
              <TimelineOptionIcon timeline={option.value} />
              {t(option.menuLabel)}
              {option.value === value && <CheckIcon />}
            </button>
          ))}
        </AnchoredPopover>
      )}
    </div>
  );
}

function ViewButton({
  view,
  selected,
  onSelect,
}: {
  view: CatalogViewMode;
  selected: boolean;
  onSelect: (view: CatalogViewMode) => void;
}) {
  const { t } = usePresentation();
  return (
    <button
      className={selected ? "active" : ""}
      type="button"
      aria-pressed={selected}
      onClick={() => onSelect(view)}
    >
      {view === "grid" ? <GridIcon /> : <LineIcon />}
      {t(view === "grid" ? "catalog.gridView" : "catalog.lineView")}
    </button>
  );
}

function CatalogGridView({
  groups,
  works,
  dayCounts,
  englishLabels,
  page,
  total,
  totalPages,
  ready,
  onChangePage,
  onOpenWork,
}: {
  groups: CatalogDateGroup[] | null;
  works: CatalogWork[];
  dayCounts: Map<string, number>;
  englishLabels: boolean;
  page: number;
  total: number;
  totalPages: number;
  ready: boolean;
  onChangePage: (page: number) => void;
  onOpenWork?: (code: string) => void | Promise<void>;
}) {
  const { t } = usePresentation();
  return (
    <div className="catalog-grid-view">
      <div className={`catalog-grid-page ${ready ? "" : "is-loading"}`} aria-busy={!ready}>
        {groups ? groups.map((group) => (
          <section className="catalog-grid-day" key={group.key}>
            <div className="catalog-grid-day-heading">
              <DateMarker day={group.day} count={dayCounts.get(group.key) ?? group.works.length} />
              <span aria-hidden="true" />
            </div>
            <div className="catalog-work-grid catalog-flat-grid">
              {group.works.map((work, index) => (
                <CatalogWorkCard
                  work={work}
                  englishLabels={englishLabels}
                  onOpenWork={onOpenWork}
                  animationIndex={index}
                  key={work.code}
                />
              ))}
            </div>
          </section>
        )) : (
          <div className="catalog-work-grid catalog-flat-grid">
            {works.map((work, index) => (
              <CatalogWorkCard
                work={work}
                englishLabels={englishLabels}
                onOpenWork={onOpenWork}
                animationIndex={index}
                key={work.code}
              />
            ))}
          </div>
        )}
        {!ready && <div className="catalog-page-loading">{t("catalog.loadingPage", { page })}</div>}
      </div>
      <CatalogPager
        currentPage={page}
        visible={works.length}
        total={total}
        totalPages={totalPages}
        onNavigate={onChangePage}
      />
    </div>
  );
}

function CatalogLineGroup({
  group,
  englishLabels,
  expanded,
  onExpand,
  onOpenWork,
  dataIndex,
  measure,
  offset,
}: {
  group: CatalogDateGroup;
  englishLabels: boolean;
  expanded: boolean;
  onExpand: () => void;
  onOpenWork?: (code: string) => void | Promise<void>;
  dataIndex: number;
  measure: (node: Element | null) => void;
  offset: number;
}) {
  const { t } = usePresentation();
  const shown = expanded ? group.works : group.works.slice(0, catalogLinePreviewSize);
  const hidden = group.works.length - shown.length;
  return (
    <section
      className="catalog-line-day"
      data-index={dataIndex}
      data-timeline-day={group.day ?? "unknown"}
      ref={measure}
      style={{ transform: `translateY(${offset}px)` }}
    >
      <div className="catalog-day-spine">
        <DateMarker day={group.day} count={group.works.length} />
        <span aria-hidden="true" />
      </div>
      <div className="catalog-work-grid catalog-line-grid">
        {shown.map((work, index) => (
          <CatalogWorkCard
            work={work}
            englishLabels={englishLabels}
            lead={index === 0 && group.works.length >= 3}
            onOpenWork={onOpenWork}
            animationIndex={index}
            key={work.code}
          />
        ))}
        {hidden > 0 && (
          <button className="catalog-show-all" type="button" onClick={onExpand}>
            {t("catalog.showAll", { count: group.works.length })}
          </button>
        )}
      </div>
    </section>
  );
}

export function DateMarker({ day, count }: { day: string | null; count: number }) {
  const { locale, t } = usePresentation();
  const marker = dateMarker(day, locale, {
    undated: t("catalog.undated"),
    unknownWeekday: t("catalog.unknownWeekday"),
  });
  return (
    <header className="catalog-date-marker">
      <span className="catalog-date-bracket top" aria-hidden="true" />
      <span className="catalog-date-bracket bottom" aria-hidden="true" />
      <span className="catalog-date-eyebrow">{marker.weekday} · {marker.month}</span>
      <strong>{marker.day}</strong>
      <small>{count} {t(count === 1 ? "catalog.work" : "catalog.works")}</small>
    </header>
  );
}

export function CatalogWorkCard({
  work,
  englishLabels,
  lead = false,
  animationIndex,
  onOpenWork,
}: {
  work: CatalogWork;
  englishLabels: boolean;
  lead?: boolean;
  animationIndex: number;
  onOpenWork?: (code: string) => void | Promise<void>;
}) {
  const { t } = usePresentation();
  const circle = work.circles[0];
  const content = (
    <article className={`catalog-work-card cover-hover-frame ${work.synthetic ? "synthetic" : ""}`}>
      <WorkCardImage
        code={work.code}
        title={work.title}
        mainImageUrls={work.mainImageUrls}
        thumbnailUrls={work.thumbnailUrls}
      />
      <span className="catalog-work-card-shade" aria-hidden="true" />
      <span className="catalog-work-card-copy">
        <strong>{work.title}</strong>
        <small>
          {circle
            ? englishLabels && circle.nameEnglish
              ? circle.nameEnglish
              : circle.name
            : work.code}
        </small>
      </span>
    </article>
  );
  const style = { "--catalog-card-index": animationIndex } as CSSProperties;

  if (!onOpenWork) {
    return <div className={`catalog-work-link cover-hover-trigger ${lead ? "lead" : ""}`} style={style}>{content}</div>;
  }
  return (
    <button
      className={`catalog-work-link cover-hover-trigger ${lead ? "lead" : ""}`}
      type="button"
      data-grab-scroll-ignore
      aria-label={t("catalog.openDetails", { title: work.title })}
      onClick={() => void onOpenWork(work.code)}
      style={style}
    >
      {content}
    </button>
  );
}

export function CatalogPager({
  currentPage,
  visible,
  total,
  totalPages,
  onNavigate,
}: {
  currentPage: number;
  visible: number;
  total: number;
  totalPages: number;
  onNavigate: (page: number) => void;
}) {
  const { locale, t } = usePresentation();
  const range = catalogPageRange(currentPage, visible, total);
  return (
    <footer className="catalog-pager-footer">
      <span>{t("catalog.showing", { from: range.from, to: range.to, total: total.toLocaleString(locale) })}</span>
      {totalPages > 1 && (
        <nav className="catalog-pager" aria-label={t("catalog.pages")}>
          <PagerArrow
            direction="previous"
            disabled={currentPage <= 1}
            onClick={() => onNavigate(currentPage - 1)}
          />
          {catalogPageLinks(currentPage, totalPages).map((item) => typeof item === "number" ? (
            item === currentPage ? (
              <span className="catalog-page active" aria-current="page" key={item}>{item}</span>
            ) : (
              <button className="catalog-page" type="button" onClick={() => onNavigate(item)} key={item}>
                {item}
              </button>
            )
          ) : (
            <CatalogPageJump
              side={item === "…left" ? "left" : "right"}
              currentPage={currentPage}
              totalPages={totalPages}
              onNavigate={onNavigate}
              key={item}
            />
          ))}
          <PagerArrow
            direction="next"
            disabled={currentPage >= totalPages}
            onClick={() => onNavigate(currentPage + 1)}
          />
        </nav>
      )}
    </footer>
  );
}

function CatalogPageJump({
  side,
  currentPage,
  totalPages,
  onNavigate,
}: {
  side: "left" | "right";
  currentPage: number;
  totalPages: number;
  onNavigate: (page: number) => void;
}) {
  const { t } = usePresentation();
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const cancel = () => {
    setEditing(false);
    setValue("");
  };
  const commit = () => {
    const page = Math.min(totalPages, Math.max(1, Math.floor(Number(value))));
    if (Number.isFinite(page) && page !== currentPage) onNavigate(page);
    cancel();
  };

  if (!editing) {
    return (
      <button
        className="catalog-page ellipsis"
        type="button"
        data-ellipsis={side}
        aria-label={t("catalog.goToPage")}
        title={t("catalog.goToPage")}
        onClick={() => setEditing(true)}
      >…</button>
    );
  }
  return (
    <span className="catalog-page ellipsis editing">
      <input
        autoFocus
        inputMode="numeric"
        pattern="[0-9]*"
        aria-label={t("catalog.goToPage")}
        placeholder="…"
        value={value}
        onChange={(event) => setValue(event.target.value.replace(/\D/g, ""))}
        onBlur={() => {
          if (!value) cancel();
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            commit();
          } else if (event.key === "Escape") {
            event.preventDefault();
            cancel();
          }
        }}
      />
    </span>
  );
}

function PagerArrow({
  direction,
  disabled,
  onClick,
}: {
  direction: "previous" | "next";
  disabled: boolean;
  onClick: () => void;
}) {
  const { t } = usePresentation();
  return (
    <button
      className="catalog-page-arrow"
      type="button"
      disabled={disabled}
      aria-label={t(direction === "previous" ? "catalog.previousPage" : "catalog.nextPage")}
      onClick={onClick}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <polyline points={direction === "previous" ? "15 18 9 12 15 6" : "9 18 15 12 9 6"} />
      </svg>
    </button>
  );
}

function useLoadMore(
  lastIndex: number | undefined,
  count: number,
  hasMore: boolean,
  fetchingMore: boolean,
  onLoadMore: () => void | Promise<unknown>,
) {
  useEffect(() => {
    if (lastIndex !== undefined && lastIndex >= count - 2 && hasMore && !fetchingMore) {
      void onLoadMore();
    }
  }, [count, fetchingMore, hasMore, lastIndex, onLoadMore]);
}

function storedViewMode(): CatalogViewMode {
  const value = window.localStorage.getItem(VIEW_STORAGE_KEY);
  return value === "line" ? "line" : "grid";
}

function isChronologicalSort(sort: CatalogSort): boolean {
  return sort === "release_asc" || sort === "release_desc";
}

function GridIcon() {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="4" y="4" width="6" height="6" /><rect x="14" y="4" width="6" height="6" /><rect x="4" y="14" width="6" height="6" /><rect x="14" y="14" width="6" height="6" /></svg>;
}

function LineIcon() {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="4" y="4" width="16" height="4" /><rect x="4" y="10" width="16" height="4" /><rect x="4" y="16" width="16" height="4" /></svg>;
}

function CalendarIcon() {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 3v3M18 3v3M4 8h16M5 5h14a1 1 0 0 1 1 1v14H4V6a1 1 0 0 1 1-1Z" /></svg>;
}

function SortIcon() {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 6h11M4 12h8M4 18h5M18 4v16M15 17l3 3 3-3" /></svg>;
}

function SortOptionIcon({ sort }: { sort: CatalogSort }) {
  if (sort === "favorites") {
    return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1.1-1.1a5.5 5.5 0 0 0-7.8 7.8l1.1 1.1L12 21l7.8-7.5 1.1-1.1a5.5 5.5 0 0 0-.1-7.8Z" /></svg>;
  }
  if (sort === "title_asc") {
    return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6h9M3 12h6M3 18h3M18 4v16M15 17l3 3 3-3" /></svg>;
  }
  if (sort === "title_desc") {
    return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6h3M3 12h6M3 18h9M18 20V4M15 7l3-3 3 3" /></svg>;
  }
  if (sort === "release_asc") {
    return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 6h11M4 12h8M4 18h5M18 20V4M15 7l3-3 3 3" /></svg>;
  }
  return <SortIcon />;
}

function TimelineOptionIcon({ timeline }: { timeline: CatalogTimeline }) {
  if (timeline === "added") {
    return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16v13H4zM3 4h18v3H3zM9 12h6" /></svg>;
  }
  if (timeline === "updated") {
    return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 6v5h-5M4 18v-5h5M18.5 10A7 7 0 0 0 6.2 6.2L4 8M5.5 14A7 7 0 0 0 17.8 17.8L20 16" /></svg>;
  }
  return <CalendarIcon />;
}

function ChevronIcon() {
  return <svg className="catalog-timeline-chevron" viewBox="0 0 24 24" aria-hidden="true"><polyline points="7 9 12 14 17 9" /></svg>;
}

function CheckIcon() {
  return <svg className="catalog-timeline-check" viewBox="0 0 24 24" aria-hidden="true"><polyline points="5 12 10 17 19 7" /></svg>;
}
