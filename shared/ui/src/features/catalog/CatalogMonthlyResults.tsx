import { useInfiniteQuery } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type RefObject,
} from "react";

import { AnchoredPopover } from "../../app/AnchoredPopover";
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
import { PersistentScrollbar } from "./PersistentScrollbar";
import {
  catalogMonthLabel,
  catalogMonthNavigation,
  catalogPageForDay,
  currentCatalogMonth,
  parseCatalogMonth,
} from "./catalogMonth";
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
} from "./types";
import { useGrabScroll } from "./useGrabScroll";

const VIEW_STORAGE_KEY = "dla-launcher:catalog-view";

interface CatalogMonthlyResultsProps {
  route: CatalogRouteState;
  filters: CatalogFilters;
  context: CatalogContext;
  page: CatalogBrowsePage;
  loading: boolean;
  gateway: CatalogGateway;
  facetFilters: CatalogFacetFilters;
  englishLabels: boolean;
  onFacetFiltersChange: (filters: CatalogFacetFilters) => void;
  onOpenWork?: (code: string) => void | Promise<void>;
  onRouteChange: (
    change: Partial<Pick<CatalogRouteState, "sort" | "timeline" | "month" | "page">>,
    replace?: boolean,
  ) => void;
}

export function CatalogMonthlyResults({
  route,
  filters,
  context,
  page,
  loading,
  gateway,
  facetFilters,
  englishLabels,
  onFacetFiltersChange,
  onOpenWork,
  onRouteChange,
}: CatalogMonthlyResultsProps) {
  const { locale, t } = usePresentation();
  const [view, setView] = useState<CatalogViewMode>(storedViewMode);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [dayJump, setDayJump] = useState<string | null>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const previousTimeline = useRef(route.timeline);
  useGrabScroll(viewport);
  const groups = useMemo(
    () => isChronologicalSort(route.sort)
      ? groupWorksByTimeline(page.items, route.timeline, route.sort)
      : null,
    [page.items, route.sort, route.timeline],
  );
  const dayCounts = useMemo(
    () => new Map(page.dayBuckets.map((bucket) => [bucket.day, bucket.count])),
    [page.dayBuckets],
  );
  const lineDays = useMemo(
    () => orderedDayBuckets(page.dayBuckets, route.sort).filter((bucket) => bucket.count > 0),
    [page.dayBuckets, route.sort],
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
  const totalPages = Math.max(1, Math.ceil(page.total / catalogGridPageSize));

  useEffect(() => {
    window.localStorage.setItem(VIEW_STORAGE_KEY, view);
    viewport.current?.scrollTo({ top: 0 });
    virtualizer.measure();
  }, [view, virtualizer]);

  useEffect(() => {
    if (previousTimeline.current === route.timeline) return;
    previousTimeline.current = route.timeline;
    setView("grid");
    setDayJump(null);
    viewport.current?.scrollTo({ top: 0 });
  }, [route.timeline]);

  useEffect(() => {
    if (!dayJump || loading || view !== "grid") return;
    const marker = viewport.current?.querySelector<HTMLElement>(`[data-timeline-day="${dayJump}"]`);
    marker?.scrollIntoView({ block: "start", behavior: "smooth" });
    setDayJump(null);
  }, [dayJump, loading, page.items, view]);

  const changePage = useCallback((next: number) => {
    const pageNumber = Math.min(Math.max(next, 1), totalPages);
    if (pageNumber === route.page) return;
    onRouteChange({ page: pageNumber });
    viewport.current?.scrollTo({ top: 0, behavior: "smooth" });
  }, [onRouteChange, route.page, totalPages]);

  const changeMonth = useCallback((month: string, preserveView = false) => {
    if (!preserveView) setView("grid");
    setPickerOpen(false);
    setDayJump(null);
    onRouteChange({ month, page: 1 });
    viewport.current?.scrollTo({ top: 0 });
  }, [onRouteChange]);

  const jumpToDay = useCallback((day: string) => {
    if (view === "line") {
      const index = lineDays.findIndex((bucket) => bucket.day === day);
      if (index >= 0) virtualizer.scrollToIndex(index, { align: "start", behavior: "smooth" });
      return;
    }
    const targetPage = catalogPageForDay(day, page.dayBuckets, route.sort);
    if (targetPage === null) return;
    setDayJump(day);
    if (targetPage !== route.page) onRouteChange({ page: targetPage });
  }, [lineDays, onRouteChange, page.dayBuckets, route.page, route.sort, view, virtualizer]);

  return (
    <section className="catalog-results catalog-timeline" aria-label={t("catalog.totalAria", { count: page.total.toLocaleString(locale) })}>
      <CatalogMonthMasthead
        month={route.month}
        filteredTotal={page.total}
        unfilteredTotal={page.unfilteredTotal}
        months={context.months}
        minMonth={context.minMonth}
        maxMonth={context.maxMonth}
        dayBuckets={page.dayBuckets}
        filtersActive={filtersActive}
        loading={loading}
        pickerOpen={pickerOpen}
        onPickerOpenChange={setPickerOpen}
        onMonthChange={(month) => changeMonth(month)}
        onMonthStep={(month) => changeMonth(month, true)}
        onDayClick={jumpToDay}
      />
      <CatalogViewBar
        view={view}
        sort={route.sort}
        timeline={route.timeline}
        loaded={page.items.length}
        total={page.total}
        facetFilters={facetFilters}
        onChange={setView}
        onSortChange={(sort) => onRouteChange({ sort, page: 1 })}
        onTimelineChange={(timeline) => onRouteChange({ timeline, month: "", page: 1 })}
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
          ) : page.total === 0 ? (
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
                          onOpenWork={onOpenWork}
                          animationIndex={index}
                          key={work.code}
                        />
                      ))}
                    </div>
                  </section>
                )) : (
                  <div className="catalog-work-grid catalog-flat-grid">
                    {page.items.map((work, index) => (
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
              </div>
              <CatalogPager
                currentPage={route.page}
                visible={page.items.length}
                total={page.total}
                totalPages={totalPages}
                onNavigate={changePage}
              />
            </div>
          ) : (
            <div className="virtual-space" style={{ height: virtualizer.getTotalSize() }}>
              {virtualizer.getVirtualItems().map((virtualDay) => {
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

function CatalogMonthMasthead({
  month,
  filteredTotal,
  unfilteredTotal,
  months,
  minMonth,
  maxMonth,
  dayBuckets,
  filtersActive,
  loading,
  pickerOpen,
  onPickerOpenChange,
  onMonthChange,
  onMonthStep,
  onDayClick,
}: {
  month: string;
  filteredTotal: number;
  unfilteredTotal: number;
  months: CatalogContext["months"];
  minMonth: string;
  maxMonth: string;
  dayBuckets: CatalogBrowsePage["dayBuckets"];
  filtersActive: boolean;
  loading: boolean;
  pickerOpen: boolean;
  onPickerOpenChange: (open: boolean) => void;
  onMonthChange: (month: string) => void;
  onMonthStep: (month: string) => void;
  onDayClick: (day: string) => void;
}) {
  const { locale, t } = usePresentation();
  const pickerAnchor = useRef<HTMLButtonElement>(null);
  const navigation = catalogMonthNavigation(month, months);
  return (
    <header className="catalog-month-masthead">
      <div className="catalog-month-title">
        <small>{t("catalog.monthEyebrow")}</small>
        <div>
          <h1>{catalogMonthLabel(month, locale)}</h1>
          <span>{loading
            ? "—"
            : filtersActive
              ? `${filteredTotal.toLocaleString(locale)} / ${unfilteredTotal.toLocaleString(locale)}`
              : unfilteredTotal.toLocaleString(locale)}</span>
        </div>
      </div>
      <div className="catalog-month-tools">
        <div className="catalog-month-navigation">
          <MonthArrow
            direction="previous"
            disabled={!navigation.previous}
            onClick={() => navigation.previous && onMonthStep(navigation.previous)}
          />
          <div className="catalog-month-picker-root">
            <button
              ref={pickerAnchor}
              className="catalog-month-calendar"
              type="button"
              aria-expanded={pickerOpen}
              aria-haspopup="dialog"
              aria-label={t("catalog.pickMonth")}
              onClick={() => onPickerOpenChange(!pickerOpen)}
            >
              <CalendarIcon />
            </button>
            {pickerOpen && (
              <CatalogMonthPicker
                anchorRef={pickerAnchor}
                value={month}
                minMonth={minMonth}
                maxMonth={maxMonth}
                enabledMonths={new Set(months.map((bucket) => bucket.month))}
                onPick={onMonthChange}
                onClose={() => onPickerOpenChange(false)}
              />
            )}
          </div>
          <MonthArrow
            direction="next"
            disabled={!navigation.next}
            onClick={() => navigation.next && onMonthStep(navigation.next)}
          />
        </div>
        <CatalogDayDensity buckets={dayBuckets} onDayClick={onDayClick} />
      </div>
    </header>
  );
}

function CatalogMonthPicker({
  anchorRef,
  value,
  minMonth,
  maxMonth,
  enabledMonths,
  onPick,
  onClose,
}: {
  anchorRef: RefObject<HTMLElement | null>;
  value: string;
  minMonth: string;
  maxMonth: string;
  enabledMonths: ReadonlySet<string>;
  onPick: (month: string) => void;
  onClose: () => void;
}) {
  const { locale, t } = usePresentation();
  const selected = parseCatalogMonth(value) ?? { year: new Date().getFullYear(), month: 1 };
  const minimum = parseCatalogMonth(minMonth) ?? selected;
  const maximum = parseCatalogMonth(maxMonth) ?? selected;
  const [view, setView] = useState<"months" | "years">("months");
  const [displayYear, setDisplayYear] = useState(selected.year);
  const [yearBase, setYearBase] = useState(() => yearPageBase(selected.year, minimum.year));

  useEffect(() => {
    const year = Math.min(maximum.year, Math.max(minimum.year, selected.year));
    setDisplayYear(year);
    setYearBase(yearPageBase(year, minimum.year));
  }, [maximum.year, minimum.year, selected.year]);

  const years = Array.from({ length: 24 }, (_, index) => yearBase + index);
  const monthNames = Array.from({ length: 12 }, (_, index) =>
    new Date(Date.UTC(2000, index, 1)).toLocaleDateString(locale, { month: "short", timeZone: "UTC" }));
  const monthEnabled = (year: number, month: number) => enabledMonths.has(`${year}-${pad2(month)}`);
  const yearEnabled = (year: number) => Array.from({ length: 12 }, (_, index) => index + 1)
    .some((month) => monthEnabled(year, month));
  const today = currentCatalogMonth();

  return (
    <AnchoredPopover
      anchorRef={anchorRef}
      className="catalog-month-picker"
      role="dialog"
      ariaLabel={t("catalog.pickMonth")}
      align="center"
      gap={9}
      maximumWidth={310}
      onClose={onClose}
    >
      <div className="catalog-month-picker-header">
        <button type="button" onClick={() => setView((current) => current === "months" ? "years" : "months")}>
          {view === "months" ? displayYear : `${years[0]} – ${years.at(-1)}`}
          <ChevronIcon expanded={view === "years"} />
        </button>
        <span>
          <button
            type="button"
            disabled={view === "months" ? displayYear <= minimum.year : yearBase <= minimum.year}
            onClick={() => view === "months"
              ? setDisplayYear((year) => Math.max(minimum.year, year - 1))
              : setYearBase((year) => Math.max(minimum.year, year - 24))}
          ><Chevron direction="previous" /></button>
          <button
            type="button"
            disabled={view === "months" ? displayYear >= maximum.year : yearBase + 23 >= maximum.year}
            onClick={() => view === "months"
              ? setDisplayYear((year) => Math.min(maximum.year, year + 1))
              : setYearBase((year) => Math.min(Math.max(minimum.year, maximum.year - 23), year + 24))}
          ><Chevron direction="next" /></button>
          <button type="button" disabled={!enabledMonths.has(today)} onClick={() => onPick(today)}>
            {t("catalog.today")}
          </button>
        </span>
      </div>
      <div className="catalog-month-picker-grid">
        {view === "months" ? monthNames.map((label, index) => {
          const monthNumber = index + 1;
          const key = `${displayYear}-${pad2(monthNumber)}`;
          return (
            <button
              className={key === value ? "active" : ""}
              type="button"
              disabled={!monthEnabled(displayYear, monthNumber)}
              onClick={() => onPick(key)}
              key={key}
            >{label}</button>
          );
        }) : years.map((year) => (
          <button
            className={year === selected.year ? "active" : ""}
            type="button"
            disabled={!yearEnabled(year)}
            onClick={() => {
              setDisplayYear(year);
              setView("months");
            }}
            key={year}
          >{year}</button>
        ))}
      </div>
    </AnchoredPopover>
  );
}

function CatalogDayDensity({
  buckets,
  onDayClick,
}: {
  buckets: CatalogBrowsePage["dayBuckets"];
  onDayClick: (day: string) => void;
}) {
  const { locale, t } = usePresentation();
  const maximum = Math.max(1, ...buckets.map((bucket) => bucket.count));
  if (buckets.length === 0) return null;
  return (
    <div className="catalog-day-density" aria-label={t("catalog.worksPerDay")}>
      <div>
        {buckets.map((bucket) => (
          <button
            type="button"
            disabled={bucket.count === 0}
            title={t("catalog.dayCountAria", { day: bucket.day, count: bucket.count.toLocaleString(locale) })}
            aria-label={t("catalog.dayCountAria", { day: bucket.day, count: bucket.count.toLocaleString(locale) })}
            onClick={() => onDayClick(bucket.day)}
            style={{
              "--density-height": `${4 + (26 * Math.max(0.08, bucket.count / maximum))}px`,
            } as CSSProperties}
            key={bucket.day}
          />
        ))}
      </div>
      <small>{t("catalog.worksPerDay")}</small>
    </div>
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

function MonthArrow({
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
      className="catalog-month-arrow"
      type="button"
      disabled={disabled}
      aria-label={t(direction === "previous" ? "catalog.previousMonth" : "catalog.nextMonth")}
      onClick={onClick}
    ><Chevron direction={direction} /></button>
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

function storedViewMode(): CatalogViewMode {
  return window.localStorage.getItem(VIEW_STORAGE_KEY) === "line" ? "line" : "grid";
}

function yearPageBase(year: number, minimum: number): number {
  return minimum + Math.floor(Math.max(0, year - minimum) / 24) * 24;
}

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

function CalendarIcon() {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 3v3M18 3v3M4 8h16M5 5h14a1 1 0 0 1 1 1v14H4V6a1 1 0 0 1 1-1Z" /></svg>;
}

function Chevron({ direction }: { direction: "previous" | "next" }) {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><polyline points={direction === "previous" ? "15 18 9 12 15 6" : "9 18 15 12 9 6"} /></svg>;
}

function ChevronIcon({ expanded }: { expanded: boolean }) {
  return <svg className={expanded ? "expanded" : ""} viewBox="0 0 24 24" aria-hidden="true"><polyline points="7 9 12 14 17 9" /></svg>;
}
