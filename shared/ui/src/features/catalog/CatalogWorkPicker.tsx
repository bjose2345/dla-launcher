import { useQuery } from "@tanstack/react-query";
import { LoaderCircle, Search } from "lucide-react";
import { useEffect, useId, useState } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import type { SearchGateway } from "../search/types";
import { emptyCatalogFacetFilters } from "./catalogFilters";
import type { CatalogDetailGateway, CatalogWork } from "./types";
import { WorkCardImage } from "./WorkCardImage";

const RESULT_LIMIT = 6;
const workCodePattern = /^[A-Z]{2}\d{5,10}$/i;

/**
 * Finds a catalog work by its code, or by title and circle once the search
 * index is ready, and hands back the one the user picks.
 */
export function CatalogWorkPicker({
  catalogGateway,
  searchGateway,
  onPick,
  autoFocus = false,
}: {
  catalogGateway: Pick<CatalogDetailGateway, "readWorks">;
  searchGateway?: Pick<SearchGateway, "search">;
  onPick: (work: CatalogWork) => void;
  autoFocus?: boolean;
}) {
  const { locale, t } = usePresentation();
  const inputId = useId();
  const [text, setText] = useState("");
  const query = useDebounced(text.trim(), 250);
  const byCode = workCodePattern.test(query);
  const results = useQuery({
    queryKey: ["catalog-work-picker", query],
    queryFn: async (): Promise<CatalogWork[]> => {
      if (byCode) return catalogGateway.readWorks([query.toUpperCase()]);
      const response = await searchGateway!.search({
        text: query,
        facets: emptyCatalogFacetFilters(),
        limit: RESULT_LIMIT,
        offset: 0,
      });
      return response.items.map((item) => item.work);
    },
    enabled: byCode || (searchGateway !== undefined && query.length >= 2),
    staleTime: 60_000,
  });
  const preferEnglish = locale !== "ja-JP";
  const works = results.data ?? [];

  return (
    <div className="catalog-work-picker">
      <label className="catalog-work-picker-field" htmlFor={inputId}>
        <Search aria-hidden="true" />
        <input
          id={inputId}
          type="search"
          value={text}
          autoFocus={autoFocus}
          placeholder={t(searchGateway ? "catalog.picker.placeholder" : "catalog.picker.codePlaceholder")}
          aria-label={t("catalog.picker.label")}
          onChange={(event) => setText(event.target.value)}
        />
        {results.isFetching ? <LoaderCircle className="catalog-work-picker-spin" aria-hidden="true" /> : null}
      </label>
      {results.isSuccess && query ? (
        works.length ? (
          <ul className="catalog-work-picker-results">
            {works.slice(0, RESULT_LIMIT).map((work) => {
              const title = preferEnglish && work.titleEnglish.trim() ? work.titleEnglish : work.title;
              const circle = work.circles[0];
              return (
                <li key={work.code}>
                  <button type="button" onClick={() => onPick(work)}>
                    <span className="catalog-work-picker-cover">
                      <WorkCardImage
                        code={work.code}
                        title={title}
                        mainImageUrls={work.mainImageUrls}
                        thumbnailUrls={work.thumbnailUrls}
                      />
                    </span>
                    <span className="catalog-work-picker-copy">
                      <strong>{title}</strong>
                      <small>
                        {circle ? `${preferEnglish && circle.nameEnglish.trim() ? circle.nameEnglish : circle.name} · ` : ""}
                        {work.code}
                      </small>
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        ) : (
          <p className="catalog-work-picker-empty">{t("catalog.picker.noResults")}</p>
        )
      ) : null}
      {results.isError ? (
        <p className="catalog-work-picker-empty">{t("catalog.picker.unavailable")}</p>
      ) : null}
    </div>
  );
}

function useDebounced(value: string, delayMs: number): string {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = window.setTimeout(() => setDebounced(value), delayMs);
    return () => window.clearTimeout(timer);
  }, [delayMs, value]);
  return debounced;
}
