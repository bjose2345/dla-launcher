// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PresentationProvider } from "../../preferences/PresentationProvider";
import { CatalogFacetFiltersProvider } from "./CatalogFacetFiltersProvider";
import { CatalogPage } from "./CatalogPage";
import { defaultCatalogRouteState } from "./query";
import type { CatalogBrowsePage, CatalogContext, CatalogGateway } from "./types";

afterEach(cleanup);

Object.defineProperty(HTMLElement.prototype, "scrollTo", {
  configurable: true,
  value: vi.fn(),
});

describe("CatalogPage empty states", () => {
  it("welcomes the user when the active catalog contains no works", async () => {
    const gateway = catalogGateway(emptyContext(), emptyPage());

    renderCatalog(gateway, { ...defaultCatalogRouteState });

    expect(await screen.findByRole("heading", { name: "No catalog imported yet" })).toBeTruthy();
    expect(screen.getByText("Import a .dla catalog package to start browsing works.")).toBeTruthy();
    expect(screen.queryByText("No matching works")).toBeNull();
    expect(gateway.browse).not.toHaveBeenCalled();
  });

  it("keeps filter guidance when a populated catalog has no matching works", async () => {
    const context = {
      ...emptyContext(),
      minMonth: "2026-08",
      maxMonth: "2026-08",
      defaultMonth: "2026-08",
      months: [{ month: "2026-08", count: 1 }],
      snapshot: { id: "catalog", realWorks: 1, syntheticWorks: 0 },
    };
    const gateway = catalogGateway(context, {
      ...emptyPage(),
      snapshot: context.snapshot,
    });

    renderCatalog(gateway, { ...defaultCatalogRouteState, month: "2026-08" });

    expect(await screen.findByText("No matching works")).toBeTruthy();
    expect(screen.getByText("Change or clear an active catalog filter.")).toBeTruthy();
    expect(screen.queryByText("No catalog imported yet")).toBeNull();
  });
});

function renderCatalog(gateway: CatalogGateway, filters: typeof defaultCatalogRouteState) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PresentationProvider>
        <CatalogFacetFiltersProvider>
          <CatalogPage
            filters={filters}
            gateway={gateway}
            onRouteChange={vi.fn()}
          />
        </CatalogFacetFiltersProvider>
      </PresentationProvider>
    </QueryClientProvider>,
  );
}

function catalogGateway(context: CatalogContext, page: CatalogBrowsePage): CatalogGateway {
  return {
    browse: vi.fn().mockResolvedValue(page),
    context: vi.fn().mockResolvedValue(context),
  };
}

function emptyContext(): CatalogContext {
  return {
    minMonth: "",
    maxMonth: "",
    defaultMonth: "",
    months: [],
    facets: {
      ages: [],
      languages: [],
      categories: [],
      genres: [],
      fileTypes: [],
      miscellanies: [],
      circles: [],
    },
    snapshot: { id: "empty", realWorks: 0, syntheticWorks: 0 },
  };
}

function emptyPage(): CatalogBrowsePage {
  const context = emptyContext();
  return {
    items: [],
    total: 0,
    unfilteredTotal: 0,
    limit: 24,
    offset: 0,
    hasMore: false,
    categories: [],
    tags: [],
    facets: context.facets,
    dayBuckets: [],
    snapshot: context.snapshot,
  };
}
