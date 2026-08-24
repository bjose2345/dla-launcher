import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { useCatalogFacetFilters } from "./CatalogFacetFiltersProvider";
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
    change: Partial<Pick<CatalogRouteState, "sort" | "timeline" | "month" | "page">>,
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
    if (nextMonth !== route.month || (resetPage && route.page !== 1)) {
      onRouteChange({ month: nextMonth, page: 1 }, true);
    }
  }, [contextValue, onRouteChange, route.month, route.page]);

  const enabled = Boolean(contextValue && isCatalogMonth(route.month));
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
      route.page,
    ],
    queryFn: () => gateway.browse(catalogRequest(
      route,
      (route.page - 1) * catalogGridPageSize,
      route.timeline,
      facetFilters,
      route.month,
      "",
      catalogGridPageSize,
    )),
    enabled,
  });

  useEffect(() => {
    if (!page.data) return;
    const totalPages = Math.max(1, Math.ceil(page.data.total / catalogGridPageSize));
    if (route.page > totalPages) onRouteChange({ page: totalPages }, true);
  }, [onRouteChange, page.data, route.page]);

  const filters = useMemo(() => ({
    search: route.search,
    category: route.category,
    tag: route.tag,
    sort: route.sort,
  }), [route.category, route.search, route.sort, route.tag]);
  const changeFacetFilters = useCallback((next: typeof facetFilters) => {
    setFacetFilters(next);
    if (route.page !== 1) onRouteChange({ page: 1 }, true);
  }, [onRouteChange, route.page, setFacetFilters]);

  return (
    <main className="catalog-shell">
      {(context.isError || page.isError) && (
        <section className="catalog-error" role="alert">
          <strong>{t("catalog.bindingFailed")}</strong>
          <span>{t("common.technicalDetail", { detail: errorMessage(context.error ?? page.error) })}</span>
          <button type="button" onClick={() => void (context.isError ? context.refetch() : page.refetch())}>
            {t("detail.tryAgain")}
          </button>
        </section>
      )}
      {context.isPending && <CatalogMonthlyLoading />}
      {contextValue && (
        <CatalogMonthlyResults
          route={route}
          filters={filters}
          context={contextValue}
          page={page.data ?? emptyPage(contextValue)}
          loading={enabled && page.isPending}
          gateway={gateway}
          facetFilters={facetFilters}
          englishLabels={locale !== "ja-JP"}
          onFacetFiltersChange={changeFacetFilters}
          onOpenWork={onOpenWork}
          onRouteChange={onRouteChange}
        />
      )}
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

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
