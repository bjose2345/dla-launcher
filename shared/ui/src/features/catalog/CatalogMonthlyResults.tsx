import { useInfiniteQuery } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import {
  CatalogActiveFilters,
  CatalogFiltersDrawer,
} from "./CatalogFiltersDrawer";
import { catalogFacetCounts } from "./catalogFilters";
import {
  CatalogPager,
  CatalogViewBar,
  CatalogWorkCard,
  DateMarker,
} from "./CatalogResults";
import { CatalogDayRail } from "./CatalogDayRail";
import { CatalogMonthRail } from "./CatalogMonthRail";
import { PersistentScrollbar } from "./PersistentScrollbar";
import { catalogMonthLabel } from "./catalogMonth";
import { catalogGridPageSize, catalogLinePreviewSize } from "./catalogPagination";
import { groupWorksByTimeline, type CatalogViewMode } from "./catalogTimeline";
import { catalogRequest, nextCatalogOffset } from "./query";
import type {
  CatalogBrowsePage,
  CatalogContext,
  CatalogFacetFilters,
  CatalogFilters,
  CatalogGateway,
  CatalogRouteState,
  CatalogTimeline,
} from "./types";
import { useGrabScroll } from "./useGrabScroll";
import { storeCatalogViewMode, storedCatalogViewMode } from "./catalogViewMode";


interface CatalogMonthlyResultsProps {
  route: CatalogRouteState;
  filters: CatalogFilters;
  context: CatalogContext;
  /** The whole month: its totals, its day buckets, and its listing when no day is locked. */
  page: CatalogBrowsePage;
  /** The locked day's listing, when a day is locked. */
  dayPage: CatalogBrowsePage | null;
  lockedDay: string;
  loading: boolean;
  gateway: CatalogGateway;
  facetFilters: CatalogFacetFilters;
  englishLabels: boolean;
  onFacetFiltersChange: (filters: CatalogFacetFilters) => void;
  onOpenWork?: (code: string) => void | Promise<void>;
  onRouteChange: (
    change: Partial<Pick<CatalogRouteState, "sort" | "timeline" | "month" | "page" | "day">>,
    replace?: boolean,
  ) => void;
}

export function CatalogMonthlyResults({
  route,
  filters,
  context,
  page,
  dayPage,
  lockedDay,
  loading,
  gateway,
  facetFilters,
  englishLabels,
  onFacetFiltersChange,
  onOpenWork,
  onRouteChange,
}: CatalogMonthlyResultsProps) {
  const { locale, t } = usePresentation();
  const [view, setView] = useState<CatalogViewMode>(storedCatalogViewMode);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const viewport = useRef<HTMLDivElement>(null);
  const previousTimeline = useRef(route.timeline);
  useGrabScroll(viewport);
  const listing = dayPage ?? page;
  const groups = useMemo(
    () => isChronologicalSort(route.sort)
      ? groupWorksByTimeline(listing.items, route.timeline, route.sort)
      : null,
    [listing.items, route.sort, route.timeline],
  );
  const dayCounts = useMemo(
    () => new Map(page.dayBuckets.map((bucket) => [bucket.day, bucket.count])),
    [page.dayBuckets],
  );
  const lineDays = useMemo(
    () => orderedDayBuckets(page.dayBuckets, route.sort)
      .filter((bucket) => bucket.count > 0 && (!lockedDay || bucket.day === lockedDay)),
    [lockedDay, page.dayBuckets, route.sort],
  );
  const selectedFacetCounts = catalogFacetCounts(facetFilters);
  const filtersActive = selectedFacetCounts.include > 0
    || selectedFacetCounts.exclude > 0
    || Boolean(filters.category)
    || Boolean(filters.tag);
  const virtualizer = useVirtualizer({
    count: view === "line" ? lineDays.length : 0,
    getScrollElement: () => viewport.current,
    estimateSize: () => 610,
    overscan: 1,
  });
  const totalPages = Math.max(1, Math.ceil(listing.total / catalogGridPageSize));
  const virtualDays = virtualizer.getVirtualItems();
  // The days on screen: the grid's current page, or the Line days in view.
  const markedDays = useMemo(() => new Set(
    (view === "line"
      ? virtualDays.map((item) => lineDays[item.index]?.day ?? "")
      : (groups ?? []).map((group) => group.day ?? "")
    ).filter(Boolean),
  ), [groups, lineDays, view, virtualDays]);

  useEffect(() => {
    storeCatalogViewMode(view);
    viewport.current?.scrollTo({ top: 0 });
    virtualizer.measure();
  }, [view, virtualizer]);

  useEffect(() => {
    if (previousTimeline.current === route.timeline) return;
    previousTimeline.current = route.timeline;
    setView("grid");
    viewport.current?.scrollTo({ top: 0 });
  }, [route.timeline]);

  const changePage = useCallback((next: number) => {
    const pageNumber = Math.min(Math.max(next, 1), totalPages);
    if (pageNumber === route.page) return;
    onRouteChange({ page: pageNumber });
    viewport.current?.scrollTo({ top: 0, behavior: "smooth" });
  }, [onRouteChange, route.page, totalPages]);

  const changeMonth = useCallback((month: string, preserveView = false) => {
    if (!preserveView) setView("grid");
    onRouteChange({ month, page: 1, day: "" });
    viewport.current?.scrollTo({ top: 0 });
  }, [onRouteChange]);

  // A day rail click locks the listing to that day; a second click clears it.
  const toggleDay = useCallback((day: string) => {
    onRouteChange({ day: day === lockedDay ? "" : day, page: 1 });
    viewport.current?.scrollTo({ top: 0 });
  }, [lockedDay, onRouteChange]);

  return (
    <section className="catalog-results catalog-timeline" aria-label={t("catalog.totalAria", { count: listing.total.toLocaleString(locale) })}>
      <CatalogMonthMasthead
        month={route.month}
        timeline={route.timeline}
        filteredTotal={page.total}
        unfilteredTotal={page.unfilteredTotal}
        months={context.months}
        minMonth={context.minMonth}
        maxMonth={context.maxMonth}
        dayBuckets={page.dayBuckets}
        lockedDay={lockedDay}
        markedDays={markedDays}
        filtersActive={filtersActive}
        loading={loading}
        onMonthChange={(month) => changeMonth(month, true)}
        onToggleDay={toggleDay}
      />
      <CatalogViewBar
        view={view}
        sort={route.sort}
        timeline={route.timeline}
        loaded={listing.items.length}
        total={listing.total}
        facetFilters={facetFilters}
        onChange={setView}
        onSortChange={(sort) => onRouteChange({ sort, page: 1, day: "" })}
        onTimelineChange={(timeline) => onRouteChange({ timeline, month: "", page: 1, day: "" })}
        onOpenFilters={() => setFiltersOpen(true)}
      />
      <CatalogActiveFilters
        facets={context.facets}
        filters={facetFilters}
        onChange={onFacetFiltersChange}
      />
      <div className="catalog-scroll-frame">
        <div
          className="catalog-scroll catalog-timeline-scroll"
          id="catalog-timeline-scroll"
          ref={viewport}
        >
          {loading ? (
            <CatalogMonthLoading />
          ) : listing.total === 0 ? (
            <section className="catalog-empty inline">
              <strong>{t("catalog.noMatches")}</strong>
              <span>{t("catalog.changeFilters")}</span>
            </section>
          ) : view === "grid" ? (
            <div className="catalog-grid-view">
              <div className="catalog-grid-page">
                {groups ? groups.map((group) => (
                  <section className="catalog-grid-day" data-timeline-day={group.day ?? "unknown"} key={group.key}>
                    <div className="catalog-grid-day-heading">
                      <DateMarker day={group.day} count={dayCounts.get(group.key) ?? group.works.length} />
                      <span aria-hidden="true" />
                    </div>
                    <div className="catalog-work-grid catalog-flat-grid">
                      {group.works.map((work, index) => (
                        <CatalogWorkCard
                          work={work}
                          englishLabels={englishLabels}
                          showActivity={route.timeline === "activity"}
                          onOpenWork={onOpenWork}
                          animationIndex={index}
                          key={work.code}
                        />
                      ))}
                    </div>
                  </section>
                )) : (
                  <div className="catalog-work-grid catalog-flat-grid">
                    {listing.items.map((work, index) => (
                      <CatalogWorkCard
                        work={work}
                        englishLabels={englishLabels}
                        showActivity={route.timeline === "activity"}
                        onOpenWork={onOpenWork}
                        animationIndex={index}
                        key={work.code}
                      />
                    ))}
                  </div>
                )}
              </div>
              <CatalogPager
                currentPage={route.page}
                visible={listing.items.length}
                total={listing.total}
                totalPages={totalPages}
                onNavigate={changePage}
              />
            </div>
          ) : (
            <div className="virtual-space" style={{ height: virtualizer.getTotalSize() }}>
              {virtualDays.map((virtualDay) => {
                const bucket = lineDays[virtualDay.index];
                if (!bucket) return null;
                return (
                  <CatalogMonthlyLineDay
                    day={bucket.day}
                    count={bucket.count}
                    filters={filters}
                    route={route}
                    facetFilters={facetFilters}
                    gateway={gateway}
                    englishLabels={englishLabels}
                    onOpenWork={onOpenWork}
                    dataIndex={virtualDay.index}
                    measure={virtualizer.measureElement}
                    offset={virtualDay.start}
                    key={bucket.day}
                  />
                );
              })}
            </div>
          )}
        </div>
        <PersistentScrollbar
          viewport={viewport}
          controls="catalog-timeline-scroll"
          label={t("catalog.scrollPosition")}
        />
      </div>
      <CatalogFiltersDrawer
        open={filtersOpen}
        facets={context.facets}
        filters={facetFilters}
        onChange={onFacetFiltersChange}
        onClose={() => setFiltersOpen(false)}
      />
    </section>
  );
}

function CatalogMonthlyLineDay({
  day,
  count,
  filters,
  route,
  facetFilters,
  gateway,
  englishLabels,
  onOpenWork,
  dataIndex,
  measure,
  offset,
}: {
  day: string;
  count: number;
  filters: CatalogFilters;
  route: CatalogRouteState;
  facetFilters: CatalogFacetFilters;
  gateway: CatalogGateway;
  englishLabels: boolean;
  onOpenWork?: (code: string) => void | Promise<void>;
  dataIndex: number;
  measure: (node: Element | null) => void;
  offset: number;
}) {
  const { t } = usePresentation();
  const [expanded, setExpanded] = useState(false);
  const result = useInfiniteQuery({
    queryKey: ["catalog-month-day", filters, facetFilters, route.timeline, route.month, day],
    initialPageParam: 0,
    queryFn: ({ pageParam }) => gateway.browse(catalogRequest(
      filters,
      pageParam,
      route.timeline,
      facetFilters,
      route.month,
      day,
      catalogLinePreviewSize,
    )),
    getNextPageParam: nextCatalogOffset,
  });
  const works = useMemo(
    () => result.data?.pages.flatMap((page) => page.items) ?? [],
    [result.data?.pages],
  );

  useEffect(() => {
    if (expanded && result.hasNextPage && !result.isFetchingNextPage) {
      void result.fetchNextPage();
    }
  }, [expanded, result.fetchNextPage, result.hasNextPage, result.isFetchingNextPage]);

  return (
    <section
      className="catalog-line-day"
      data-index={dataIndex}
      data-timeline-day={day}
      ref={measure}
      style={{ transform: `translateY(${offset}px)` }}
    >
      <div className="catalog-day-spine">
        <DateMarker day={day} count={count} />
        <span aria-hidden="true" />
      </div>
      <div className="catalog-work-grid catalog-line-grid">
        {works.map((work, index) => (
          <CatalogWorkCard
            work={work}
            englishLabels={englishLabels}
            lead={index === 0 && count >= 3}
            showActivity={route.timeline === "activity"}
            onOpenWork={onOpenWork}
            animationIndex={index}
            key={work.code}
          />
        ))}
        {!result.isError && !expanded && count > works.length && (
          <button className="catalog-show-all" type="button" onClick={() => setExpanded(true)}>
            {t("catalog.showAll", { count })}
          </button>
        )}
        {(result.isPending || result.isFetchingNextPage) && (
          <span className="loading-more">{t("catalog.loadingMore")}</span>
        )}
        {result.isError && (
          <div className="catalog-day-query-error" role="alert">
            <span>{t("catalog.dayLoadFailed")}</span>
            <button type="button" onClick={() => void result.refetch()}>{t("detail.tryAgain")}</button>
          </div>
        )}
      </div>
    </section>
  );
}

const monthEyebrows = {
  activity: "catalog.monthEyebrowActivity",
  added: "catalog.monthEyebrowAdded",
  release: "catalog.monthEyebrowReleased",
  updated: "catalog.monthEyebrowUpdated",
} as const satisfies Record<CatalogTimeline, string>;

function CatalogMonthMasthead({
  month,
  timeline,
  filteredTotal,
  unfilteredTotal,
  months,
  minMonth,
  maxMonth,
  dayBuckets,
  lockedDay,
  markedDays,
  filtersActive,
  loading,
  onMonthChange,
  onToggleDay,
}: {
  month: string;
  timeline: CatalogTimeline;
  filteredTotal: number;
  unfilteredTotal: number;
  months: CatalogContext["months"];
  minMonth: string;
  maxMonth: string;
  dayBuckets: CatalogBrowsePage["dayBuckets"];
  lockedDay: string;
  markedDays: ReadonlySet<string>;
  filtersActive: boolean;
  loading: boolean;
  onMonthChange: (month: string) => void;
  onToggleDay: (day: string) => void;
}) {
  const { locale, t } = usePresentation();
  return (
    <header className="catalog-month-masthead">
      <div className="catalog-month-head">
        <div className="catalog-month-title">
          <small>{t(monthEyebrows[timeline])}</small>
          <div>
            <h1>{catalogMonthLabel(month, locale)}</h1>
            <span>{loading
              ? "—"
              : filtersActive
                ? `${filteredTotal.toLocaleString(locale)} / ${unfilteredTotal.toLocaleString(locale)}`
                : unfilteredTotal.toLocaleString(locale)}</span>
          </div>
        </div>
      </div>
      <CatalogMonthRail
        month={month}
        minMonth={minMonth}
        maxMonth={maxMonth}
        months={months}
        onPick={onMonthChange}
        onStep={onMonthChange}
      />
      <CatalogDayRail
        buckets={dayBuckets}
        lockedDay={lockedDay}
        markedDays={markedDays}
        onToggleDay={onToggleDay}
      />
    </header>
  );
}

function CatalogMonthLoading() {
  const { t } = usePresentation();
  return (
    <section className="catalog-month-loading" aria-label={t("catalog.loadingSelectedMonth")}>
      {Array.from({ length: 12 }, (_, index) => <span key={index} />)}
    </section>
  );
}

function orderedDayBuckets(
  buckets: CatalogBrowsePage["dayBuckets"],
  sort: CatalogRouteState["sort"],
) {
  return sort === "release_asc" ? buckets : [...buckets].reverse();
}

function isChronologicalSort(sort: CatalogRouteState["sort"]): boolean {
  return sort === "release_asc" || sort === "release_desc";
}


