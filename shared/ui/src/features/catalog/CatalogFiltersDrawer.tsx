import {
  ArrowDown10,
  ArrowDownAZ,
  ChevronDown,
  CircleUserRound,
  FileType2,
  Languages,
  Search,
  Shapes,
  SlidersHorizontal,
  Sparkles,
  Tags,
  X,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { useDocumentScrollLock } from "../../app/useDocumentScrollLock";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { MessageKey } from "../../preferences/preferences";
import {
  catalogFacetCounts,
  catalogFacetState,
  cycleCatalogFacet,
  emptyCatalogFacetFilters,
  setCatalogFacetState,
} from "./catalogFilters";
import { CatalogFilterChip } from "./CatalogFilterChip";
import {
  catalogFacetGroups,
  type CatalogFacet,
  type CatalogFacetCatalog,
  type CatalogFacetFilters,
  type CatalogFacetGroup,
} from "./types";

interface FacetGroupDefinition {
  key: CatalogFacetGroup;
  labelKey: MessageKey;
  Icon: LucideIcon;
}

const facetGroups: FacetGroupDefinition[] = [
  { key: "ages", labelKey: "facet.age", Icon: Tags },
  { key: "languages", labelKey: "facet.languages", Icon: Languages },
  { key: "categories", labelKey: "facet.categories", Icon: Shapes },
  { key: "genres", labelKey: "facet.genres", Icon: Tags },
  { key: "fileTypes", labelKey: "facet.fileTypes", Icon: FileType2 },
  { key: "miscellanies", labelKey: "facet.miscellaneous", Icon: Sparkles },
  { key: "circles", labelKey: "facet.circles", Icon: CircleUserRound },
];

export function CatalogFiltersDrawer({
  open,
  facets,
  filters,
  onChange,
  onClose,
}: {
  open: boolean;
  facets: CatalogFacetCatalog;
  filters: CatalogFacetFilters;
  onChange: (filters: CatalogFacetFilters) => void;
  onClose: () => void;
}) {
  const { locale, t } = usePresentation();
  const reduceMotion = prefersReducedMotion();
  const drawerRef = useRef<HTMLElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const sectionRefs = useRef<Partial<Record<CatalogFacetGroup, HTMLElement | null>>>({});
  const [query, setQuery] = useState("");
  const [sortBy, setSortBy] = useState<"count" | "name">("count");
  const [collapsed, setCollapsed] = useState<Set<CatalogFacetGroup>>(() => new Set());
  const [limits, setLimits] = useState<Partial<Record<CatalogFacetGroup, number>>>({});
  const [activeGroup, setActiveGroup] = useState<CatalogFacetGroup>("ages");
  const counts = catalogFacetCounts(filters);
  const total = counts.include + counts.exclude;
  useDocumentScrollLock(open);

  useEffect(() => {
    if (!open) return;
    const previouslyFocused = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const focusTimer = window.setTimeout(() => searchRef.current?.focus(), 180);
    const handleDialogKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = drawerRef.current?.querySelectorAll<HTMLElement>(
        'button:not([disabled]), input:not([disabled]), [href], [tabindex]:not([tabindex="-1"])',
      );
      if (!focusable?.length) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    };
    window.addEventListener("keydown", handleDialogKey);
    return () => {
      window.clearTimeout(focusTimer);
      window.removeEventListener("keydown", handleDialogKey);
      previouslyFocused?.focus();
    };
  }, [onClose, open]);

  const visibleFacets = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    return Object.fromEntries(catalogFacetGroups.map((group) => {
      const selected = [...filters[group].include, ...filters[group].exclude];
      const known = new Set(facets[group].map((facet) => facet.key));
      const options = [
        ...facets[group],
        ...selected.filter((key) => !known.has(key)).map((key) => ({
          key,
          label: key,
          labelEnglish: key,
          count: 0,
        })),
      ];
      const filtered = normalizedQuery
        ? options.filter((facet) => `${facet.label} ${facet.labelEnglish} ${facet.key}`
          .toLocaleLowerCase()
          .includes(normalizedQuery))
        : options;
      filtered.sort((left, right) => sortBy === "name"
        ? facetLabel(left, locale).localeCompare(facetLabel(right, locale), locale, { sensitivity: "base" })
        : right.count - left.count || facetLabel(left, locale).localeCompare(facetLabel(right, locale), locale));
      return [group, filtered];
    })) as CatalogFacetCatalog;
  }, [facets, filters, locale, query, sortBy]);

  if (typeof document === "undefined") return null;

  return createPortal(
    <div
      className={`catalog-filter-backdrop ${open ? "open" : ""}`}
      aria-hidden={!open}
      onPointerDown={(event) => {
        if (open && event.target === event.currentTarget) onClose();
      }}
    >
      <aside
        ref={drawerRef}
        className="catalog-filter-drawer"
        role="dialog"
        aria-modal={open ? "true" : undefined}
        aria-label={t("filter.title")}
      >
        <header className="catalog-filter-drawer-header">
          <div>
            <SlidersHorizontal aria-hidden="true" />
            <strong>{t("filter.title")}</strong>
            {total > 0 && <PolarityCount include={counts.include} exclude={counts.exclude} />}
          </div>
          <div>
            <button
              className="catalog-filter-sort"
              type="button"
              onClick={() => setSortBy((current) => current === "count" ? "name" : "count")}
              title={t(sortBy === "count" ? "filter.sortName" : "filter.sortCount")}
            >
              {sortBy === "count" ? <ArrowDown10 aria-hidden="true" /> : <ArrowDownAZ aria-hidden="true" />}
            </button>
            <button className="catalog-filter-close" type="button" onClick={onClose} aria-label={t("filter.close")}>
              <X aria-hidden="true" />
            </button>
          </div>
        </header>

        <label className="catalog-filter-search">
          <Search aria-hidden="true" />
          <input
            ref={searchRef}
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("filter.search")}
          />
        </label>

        <nav className="catalog-filter-tabs" aria-label={t("filter.sections")}>
          {facetGroups.map(({ key, labelKey }) => {
            const selected = filters[key].include.length + filters[key].exclude.length;
            return (
              <button
                className={`facet-${key} ${activeGroup === key ? "active" : ""}`}
                type="button"
                onClick={() => {
                  setActiveGroup(key);
                  setCollapsed((current) => {
                    if (!current.has(key)) return current;
                    const next = new Set(current);
                    next.delete(key);
                    return next;
                  });
                  sectionRefs.current[key]?.scrollIntoView({ behavior: reduceMotion ? "auto" : "smooth" });
                }}
                key={key}
              >
                <i aria-hidden="true" />
                {t(labelKey)}
                {selected > 0 && <small>{selected}</small>}
              </button>
            );
          })}
        </nav>

        {total > 0 && (
          <div className="catalog-filter-drawer-active">
            <span>{t("filter.active")}</span>
            <button type="button" onClick={() => onChange(emptyCatalogFacetFilters())}>{t("filter.clearAll")}</button>
          </div>
        )}

        <div
          className="catalog-filter-sections"
          onScroll={(event) => {
            const top = event.currentTarget.getBoundingClientRect().top + 16;
            const positions = facetGroups.map(({ key }) => ({
              key,
              top: sectionRefs.current[key]?.getBoundingClientRect().top ?? Number.POSITIVE_INFINITY,
            }));
            const current = positions.filter((position) => position.top <= top).at(-1) ?? positions[0];
            if (current) setActiveGroup(current.key);
          }}
        >
          {facetGroups.map(({ key, labelKey, Icon }) => {
            const options = visibleFacets[key];
            const limit = query ? options.length : limits[key] ?? 24;
            const shown = options.slice(0, limit);
            const isCollapsed = collapsed.has(key);
            const selected = filters[key].include.length + filters[key].exclude.length;
            return (
              <section
                className={`catalog-filter-section facet-${key}`}
                ref={(node) => { sectionRefs.current[key] = node; }}
                key={key}
              >
                <button
                  className="catalog-filter-section-heading"
                  type="button"
                  aria-expanded={!isCollapsed}
                  onClick={() => setCollapsed((current) => {
                    const next = new Set(current);
                    if (next.has(key)) next.delete(key);
                    else next.add(key);
                    return next;
                  })}
                >
                  <span><Icon aria-hidden={true} /></span>
                  <strong>{t(labelKey)}</strong>
                  <small>{options.length}</small>
                  {selected > 0 && <PolarityCount include={filters[key].include.length} exclude={filters[key].exclude.length} />}
                  <ChevronDown aria-hidden="true" />
                </button>
                {!isCollapsed && (
                  <div className="catalog-filter-options">
                    {shown.map((facet) => (
                      <CatalogFilterChip
                        label={facetLabel(facet, locale)}
                        count={facet.count}
                        state={catalogFacetState(filters, key, facet.key)}
                        onCycle={() => onChange(cycleCatalogFacet(filters, key, facet.key))}
                        key={facet.key}
                      />
                    ))}
                    {options.length === 0 && <p>{t("filter.noValues")}</p>}
                    {!query && options.length > shown.length && (
                      <button
                        className="catalog-filter-show-more"
                        type="button"
                        onClick={() => setLimits((current) => ({
                          ...current,
                          [key]: Math.min(options.length, limit + 48),
                        }))}
                      >
                        {t("filter.showMore")} <small>+{Math.min(48, options.length - shown.length)}</small>
                      </button>
                    )}
                  </div>
                )}
              </section>
            );
          })}
        </div>
      </aside>
    </div>,
    document.body,
  );
}

export function CatalogActiveFilters({
  facets,
  filters,
  onChange,
}: {
  facets: CatalogFacetCatalog;
  filters: CatalogFacetFilters;
  onChange: (filters: CatalogFacetFilters) => void;
}) {
  const { locale, t } = usePresentation();
  const active = facetGroups.flatMap(({ key }) => [
    ...filters[key].include.map((value) => ({ group: key, value, state: "include" as const })),
    ...filters[key].exclude.map((value) => ({ group: key, value, state: "exclude" as const })),
  ]);
  if (!active.length) return null;

  return (
    <div className="catalog-active-filters">
      <strong>{t("filter.active")}</strong>
      <div>
        {active.map(({ group, value, state }) => (
          <CatalogFilterChip
            label={facetLabel(facets[group].find((facet) => facet.key === value) ?? {
              key: value,
              label: value,
              labelEnglish: value,
              count: 0,
            }, locale)}
            state={state}
            onCycle={() => onChange(setCatalogFacetState(
              filters,
              group,
              value,
              state === "include" ? "exclude" : "include",
            ))}
            onRemove={() => onChange(setCatalogFacetState(filters, group, value, "off"))}
            key={`${group}:${value}`}
          />
        ))}
      </div>
      <button type="button" onClick={() => onChange(emptyCatalogFacetFilters())}>
        <span aria-hidden="true">↻</span> {t("filter.clearAll")}
      </button>
    </div>
  );
}

export function CatalogFilterTrigger({
  filters,
  onOpen,
}: {
  filters: CatalogFacetFilters;
  onOpen: () => void;
}) {
  const { t } = usePresentation();
  const counts = catalogFacetCounts(filters);
  const total = counts.include + counts.exclude;
  return (
    <button
      className={`catalog-filter-trigger ${total > 0 ? "active" : ""}`}
      type="button"
      aria-haspopup="dialog"
      onClick={onOpen}
    >
      <SlidersHorizontal aria-hidden="true" />
      <span>{t("filter.title")}</span>
      {total === 0 ? <strong>{t("filter.any")}</strong> : <PolarityCount include={counts.include} exclude={counts.exclude} />}
    </button>
  );
}

function PolarityCount({ include, exclude }: { include: number; exclude: number }) {
  const { t } = usePresentation();
  return (
    <span className="catalog-polarity-count" aria-label={t("filter.includedExcluded", { include, exclude })}>
      {include > 0 && <i className="include">{exclude > 0 ? `+${include}` : include}</i>}
      {exclude > 0 && <i className="exclude">−{exclude}</i>}
    </span>
  );
}

function facetLabel(facet: CatalogFacet, locale: string): string {
  return locale === "ja-JP"
    ? facet.label || facet.labelEnglish || facet.key
    : facet.labelEnglish || facet.label || facet.key;
}

function prefersReducedMotion(): boolean {
  return typeof window !== "undefined"
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}
