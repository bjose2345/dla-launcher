import { useInfiniteQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

import { useCatalogFacetFilters } from "./CatalogFacetFiltersProvider";
import { CatalogResults } from "./CatalogResults";
import { catalogRequest, nextCatalogOffset } from "./query";
import type { CatalogGateway, CatalogRouteState, CatalogTimeline } from "./types";
import { usePresentation } from "../../preferences/PresentationProvider";
import { CatalogMonthlyPage } from "./CatalogMonthlyPage";

interface CatalogPageProps {
  filters: CatalogRouteState;
  gateway: CatalogGateway;
  onOpenWork?: (code: string) => void | Promise<void>;
  onRouteChange?: (
    change: Partial<Pick<CatalogRouteState, "sort" | "timeline" | "month" | "page">>,
    replace?: boolean,
  ) => void;
}

export function CatalogPage(props: CatalogPageProps) {
  if (props.gateway.context && props.onRouteChange) {
    return (
      <CatalogMonthlyPage
        route={props.filters}
        gateway={{ browse: props.gateway.browse, context: props.gateway.context }}
        onOpenWork={props.onOpenWork}
        onRouteChange={props.onRouteChange}
      />
    );
  }
  return <LegacyCatalogPage filters={props.filters} gateway={props.gateway} onOpenWork={props.onOpenWork} />;
}

function LegacyCatalogPage({ filters, gateway, onOpenWork }: Omit<CatalogPageProps, "onRouteChange">) {
  const { locale, t } = usePresentation();
  const [timeline, setTimeline] = useState<CatalogTimeline>("added");
  const { filters: facetFilters, setFilters: setFacetFilters } = useCatalogFacetFilters();
  const catalog = useInfiniteQuery({
    queryKey: ["catalog", filters, facetFilters, timeline],
    initialPageParam: 0,
    queryFn: ({ pageParam }) => gateway.browse(catalogRequest(filters, pageParam, timeline, facetFilters)),
    getNextPageParam: nextCatalogOffset,
    placeholderData: (previousData) => previousData,
  });
  const pages = catalog.data?.pages ?? [];
  const firstPage = pages[0];
  const works = useMemo(() => pages.flatMap((page) => page.items), [pages]);
  const total = firstPage?.total ?? 0;

  return (
    <main className="catalog-shell">
      {catalog.isError && (
        <section className="catalog-error" role="alert">
          <strong>{t("catalog.bindingFailed")}</strong>
          <span>{t("common.technicalDetail", { detail: errorMessage(catalog.error) })}</span>
          <button type="button" onClick={() => void catalog.refetch()}>{t("detail.tryAgain")}</button>
        </section>
      )}

      {catalog.isPending && <CatalogLoading />}

      {firstPage && (
        <CatalogResults
          works={works}
          total={total}
          sort={filters.sort}
          timeline={timeline}
          facets={firstPage.facets}
          facetFilters={facetFilters}
          englishLabels={locale !== "ja-JP"}
          hasMore={!catalog.isPlaceholderData && catalog.hasNextPage}
          fetchingMore={catalog.isFetchingNextPage}
          onLoadMore={catalog.fetchNextPage}
          onOpenWork={onOpenWork}
          onTimelineChange={setTimeline}
          onFacetFiltersChange={setFacetFilters}
        />
      )}
    </main>
  );
}

function CatalogLoading() {
  const { t } = usePresentation();
  return (
    <section className="catalog-loading" aria-label={t("catalog.loading")}>
      {Array.from({ length: 7 }, (_, index) => <span key={index} />)}
    </section>
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
