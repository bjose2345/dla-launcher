import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef } from "react";

import { errorMessage } from "../../app/errorMessage";
import { usePresentation } from "../../preferences/PresentationProvider";
import { useCatalogFacetFilters } from "./CatalogFacetFiltersProvider";
import { CatalogEmptyState, catalogSnapshotIsEmpty } from "./CatalogEmptyState";
import { CatalogMonthlyResults } from "./CatalogMonthlyResults";
import { catalogFacetFilterKey, catalogFacetCounts } from "./catalogFilters";
import { reconcileCatalogMonth } from "./catalogMonth";
import { catalogGridPageSize } from "./catalogPagination";
import { catalogContextRequest, catalogRequest, isCatalogMonth } from "./query";
import type {
  CatalogBrowsePage,
  CatalogContext,
  CatalogContextRequest,
  CatalogGateway,
  CatalogRouteState,
} from "./types";

interface MonthlyGateway extends CatalogGateway {
  context(request: CatalogContextRequest): Promise<CatalogContext>;
}

interface CatalogMonthlyPageProps {
  route: CatalogRouteState;
  gateway: MonthlyGateway;
  onOpenWork?: (code: string) => void | Promise<void>;
  onRouteChange: (
    change: Partial<Pick<CatalogRouteState, "sort" | "timeline" | "month" | "page" | "day">>,
    replace?: boolean,
  ) => void;
}

export function CatalogMonthlyPage({
  route,
  gateway,
  onOpenWork,
  onRouteChange,
}: CatalogMonthlyPageProps) {
  const { locale, t } = usePresentation();
  const { filters: facetFilters, setFilters: setFacetFilters } = useCatalogFacetFilters();
  const filterKey = `${route.category}:${route.tag}:${catalogFacetFilterKey(facetFilters)}`;
  const facetCounts = catalogFacetCounts(facetFilters);
  const filterCount = facetCounts.include
    + facetCounts.exclude
    + Number(Boolean(route.category))
    + Number(Boolean(route.tag));
  const previousFilters = useRef({ key: filterKey, count: filterCount });
  const preferLatest = useRef(false);
  const filtersChanged = useRef(false);

  useEffect(() => {
    if (previousFilters.current.key === filterKey) return;
    preferLatest.current = previousFilters.current.count > 0 && filterCount === 0;
    filtersChanged.current = true;
    previousFilters.current = { key: filterKey, count: filterCount };
  }, [filterCount, filterKey]);

  const context = useQuery({
    queryKey: ["catalog-context", route.category, route.tag, facetFilters, route.timeline],
    queryFn: () => gateway.context(catalogContextRequest(route, route.timeline, facetFilters)),
  });
  const contextValue = context.data;

  useEffect(() => {
    if (!contextValue) return;
    const nextMonth = reconcileCatalogMonth(
      route.month,
      contextValue.months,
      contextValue.defaultMonth,
      preferLatest.current,
    );
    const resetPage = filtersChanged.current || nextMonth !== route.month;
    preferLatest.current = false;
    filtersChanged.current = false;
    if (nextMonth !== route.month) {
      onRouteChange({ month: nextMonth, page: 1, day: "" }, true);
    } else if (resetPage && route.page !== 1) {
      onRouteChange({ page: 1 }, true);
    }
  }, [contextValue, onRouteChange, route.month, route.page]);

  const enabled = Boolean(contextValue && isCatalogMonth(route.month));
  const lockedDay = route.day.startsWith(route.month) ? route.day : "";
  // The month page always describes the whole month (totals and the day
  // rail); a locked day lists through its own page query.
  const page = useQuery({
    queryKey: [
      "catalog-month",
      route.search,
      route.category,
      route.tag,
      route.sort,
      facetFilters,
      route.timeline,
      route.month,
      lockedDay ? 1 : route.page,
    ],
    queryFn: () => gateway.browse(catalogRequest(
      route,
      lockedDay ? 0 : (route.page - 1) * catalogGridPageSize,
      route.timeline,
      facetFilters,
      route.month,
      "",
      catalogGridPageSize,
    )),
    enabled,
  });
  const dayPage = useQuery({
    queryKey: [
      "catalog-month-day-page",
      route.search,
      route.category,
      route.tag,
      route.sort,
      facetFilters,
      route.timeline,
      lockedDay,
      route.page,
    ],
    queryFn: () => gateway.browse(catalogRequest(
      route,
      (route.page - 1) * catalogGridPageSize,
      route.timeline,
      facetFilters,
      route.month,
      lockedDay,
      catalogGridPageSize,
    )),
    enabled: enabled && Boolean(lockedDay),
  });
  const listing = lockedDay ? dayPage.data : page.data;

  useEffect(() => {
    if (!listing) return;
    const totalPages = Math.max(1, Math.ceil(listing.total / catalogGridPageSize));
    if (route.page > totalPages) onRouteChange({ page: totalPages }, true);
  }, [listing, onRouteChange, route.page]);

  const filters = useMemo(() => ({
    search: route.search,
    category: route.category,
    tag: route.tag,
    sort: route.sort,
  }), [route.category, route.search, route.sort, route.tag]);
  const changeFacetFilters = useCallback((next: typeof facetFilters) => {
    setFacetFilters(next);
    if (route.page !== 1 || route.day) onRouteChange({ page: 1, day: "" }, true);
  }, [onRouteChange, route.day, route.page, setFacetFilters]);

  return (
    <main className="catalog-shell">
      {(context.isError || page.isError || dayPage.isError) && (
        <section className="catalog-error" role="alert">
          <strong>{t("catalog.bindingFailed")}</strong>
          <span>{t("common.technicalDetail", { detail: errorMessage(context.error ?? page.error ?? dayPage.error) })}</span>
          <button
            type="button"
            onClick={() => void (context.isError ? context.refetch() : page.isError ? page.refetch() : dayPage.refetch())}
          >
            {t("detail.tryAgain")}
          </button>
        </section>
      )}
      {context.isPending && <CatalogMonthlyLoading />}
      {contextValue && catalogSnapshotIsEmpty(contextValue.snapshot) ? (
        <CatalogEmptyState />
      ) : contextValue ? (
        <CatalogMonthlyResults
          route={route}
          filters={filters}
          context={contextValue}
          page={page.data ?? emptyPage(contextValue)}
          dayPage={lockedDay ? dayPage.data ?? null : null}
          lockedDay={lockedDay}
          loading={enabled && (page.isPending || (Boolean(lockedDay) && dayPage.isPending))}
          gateway={gateway}
          facetFilters={facetFilters}
          englishLabels={locale !== "ja-JP"}
          onFacetFiltersChange={changeFacetFilters}
          onOpenWork={onOpenWork}
          onRouteChange={onRouteChange}
        />
      ) : null}
    </main>
  );
}

function emptyPage(context: CatalogContext): CatalogBrowsePage {
  return {
    items: [],
    total: 0,
    unfilteredTotal: 0,
    limit: catalogGridPageSize,
    offset: 0,
    hasMore: false,
    categories: [],
    tags: [],
    facets: context.facets,
    dayBuckets: [],
    snapshot: context.snapshot,
  };
}

function CatalogMonthlyLoading() {
  const { t } = usePresentation();
  return (
    <section className="catalog-loading" aria-label={t("catalog.loadingMonth")}>
      {Array.from({ length: 7 }, (_, index) => <span key={index} />)}
    </section>
  );
}
