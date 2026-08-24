import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  CircleUserRound,
  Clock3,
  Database,
  History,
  LoaderCircle,
  RotateCw,
  Search,
  Tag,
  Trash2,
  X,
} from "lucide-react";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

import { useDocumentScrollLock } from "../../app/useDocumentScrollLock";
import type { MessageKey } from "../../i18n/catalogs";
import {
  CatalogFilterChip,
  CatalogPager,
  CatalogWorkCard,
  catalogFacetFilterKey,
  catalogFacetState,
  cycleCatalogFacet,
  setCatalogFacetState,
  useCatalogFacetFilters,
  type CatalogFacetGroup,
} from "../catalog";
import { usePresentation } from "../../preferences/PresentationProvider";
import {
  clearRecentSearches,
  readRecentSearches,
  recordRecentSearch,
} from "./recentSearches";
import type {
  SearchGateway,
  SearchIndexStatus,
  SearchRebuildProgress,
  SearchResultItem,
  SearchShortcut,
  SearchShortcutKind,
} from "./types";
import { searchRebuildIsTerminal } from "./types";

const SEARCH_PAGE_SIZE = 24;
const SEARCH_INDEX_QUERY_KEY = ["catalog-search-index"] as const;
const SEARCH_REBUILD_QUERY_KEY = ["catalog-search-rebuild"] as const;

export function SearchModal({
  open,
  gateway,
  onClose,
  onOpenWork,
}: {
  open: boolean;
  gateway: SearchGateway;
  onClose: () => void;
  onOpenWork: (code: string) => void | Promise<void>;
}) {
  const { locale, t } = usePresentation();
  const { filters, setFilters } = useCatalogFacetFilters();
  const queryClient = useQueryClient();
  const input = useRef<HTMLInputElement>(null);
  const autoPrepareKey = useRef("");
  const [text, setText] = useState("");
  const [debouncedText, setDebouncedText] = useState("");
  const [page, setPage] = useState(1);
  const [selectedIndex, setSelectedIndex] = useState(-1);
  const [recent, setRecent] = useState(readRecentSearches);
  useDocumentScrollLock(open);

  const indexStatus = useQuery({
    queryKey: SEARCH_INDEX_QUERY_KEY,
    queryFn: () => gateway.status(),
    enabled: open,
    refetchInterval: (query) => query.state.data?.state === "building" ? 500 : false,
  });
  const rebuildProgress = useQuery({
    queryKey: SEARCH_REBUILD_QUERY_KEY,
    queryFn: () => gateway.readRebuildProgress(),
    enabled: open,
    refetchInterval: (query) => query.state.data && !searchRebuildIsTerminal(query.state.data.stage) ? 500 : false,
  });
  const rebuild = useMutation({
    mutationFn: () => gateway.rebuild(),
    onSuccess: (progress) => {
      queryClient.setQueryData(SEARCH_REBUILD_QUERY_KEY, progress);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: SEARCH_INDEX_QUERY_KEY });
      void queryClient.invalidateQueries({ queryKey: SEARCH_REBUILD_QUERY_KEY });
    },
  });
  const cancelRebuild = useMutation({
    mutationFn: (operationId: string) => gateway.cancelRebuild(operationId),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: SEARCH_INDEX_QUERY_KEY });
      void queryClient.invalidateQueries({ queryKey: SEARCH_REBUILD_QUERY_KEY });
    },
  });
  const cleanupCache = useMutation({
    mutationFn: () => gateway.cleanupCache(),
  });

  useEffect(() => {
    if (!open) return;
    const frame = window.requestAnimationFrame(() => input.current?.focus());
    return () => {
      window.cancelAnimationFrame(frame);
    };
  }, [open]);

  useEffect(() => {
    const timer = window.setTimeout(() => setDebouncedText(text.trim()), 160);
    return () => window.clearTimeout(timer);
  }, [text]);

  const filterKey = catalogFacetFilterKey(filters);
  useEffect(() => {
    setPage(1);
    setSelectedIndex(-1);
  }, [debouncedText, filterKey]);

  const status = indexStatus.data;
  const currentRebuild = rebuildProgress.data;
  const rebuildActive = Boolean(currentRebuild && !searchRebuildIsTerminal(currentRebuild.stage));
  useEffect(() => {
    if (!open || !status || rebuild.isPending || rebuildActive) return;
    if (status.state !== "missing" && status.state !== "stale") return;
    const key = `${status.state}:${status.catalogSnapshotId}:${status.generation}`;
    if (autoPrepareKey.current === key) return;
    autoPrepareKey.current = key;
    rebuild.mutate();
  }, [open, rebuild, rebuildActive, status]);

  useEffect(() => {
    if (!open) return;
    let unsubscribe: (() => void) | undefined;
    let disposed = false;
    void gateway.subscribeRebuildProgress((progress) => {
      queryClient.setQueryData<SearchRebuildProgress | null>(SEARCH_REBUILD_QUERY_KEY, progress);
      if (searchRebuildIsTerminal(progress.stage)) {
        void queryClient.invalidateQueries({ queryKey: SEARCH_INDEX_QUERY_KEY });
      }
    }).then((listener) => {
      if (disposed) listener();
      else unsubscribe = listener;
    });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [gateway, open, queryClient]);

  const archiveHash = isArchiveHash(debouncedText);
  const textSearchReady = status?.state === "ready";
  const canSearch = debouncedText.length > 0 && (archiveHash || textSearchReady);
  const results = useQuery({
    queryKey: ["catalog-search", debouncedText, filterKey, page],
    queryFn: () => gateway.search({
      text: debouncedText,
      facets: filters,
      limit: SEARCH_PAGE_SIZE,
      offset: (page - 1) * SEARCH_PAGE_SIZE,
    }),
    enabled: open && canSearch,
    placeholderData: (previous) => previous,
  });
  const shortcuts = useQuery({
    queryKey: ["catalog-search-shortcuts", debouncedText],
    queryFn: () => gateway.shortcuts(debouncedText, 6),
    enabled: open && debouncedText.length >= 2,
    placeholderData: (previous) => previous,
  });
  const items = results.data?.items ?? [];
  const total = results.data?.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / SEARCH_PAGE_SIZE));
  const groupedShortcuts = useMemo(() => ({
    genre: shortcuts.data?.filter((shortcut) => shortcut.kind === "genre") ?? [],
    circle: shortcuts.data?.filter((shortcut) => shortcut.kind === "circle") ?? [],
  }), [shortcuts.data]);

  useEffect(() => {
    if (selectedIndex >= items.length) setSelectedIndex(items.length > 0 ? 0 : -1);
  }, [items.length, selectedIndex]);

  if (!open) return null;

  const openResult = (item: SearchResultItem) => {
    setRecent(recordRecentSearch(debouncedText));
    onClose();
    void onOpenWork(item.work.code);
  };
  const handleInputKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
      return;
    }
    if (items.length === 0) return;
    if (event.key === "ArrowDown" || event.key === "ArrowRight") {
      event.preventDefault();
      setSelectedIndex((current) => (current + 1 + items.length) % items.length);
    }
    if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
      event.preventDefault();
      setSelectedIndex((current) => (current <= 0 ? items.length - 1 : current - 1));
    }
    if (event.key === "Enter") {
      event.preventDefault();
      const selected = items[selectedIndex >= 0 ? selectedIndex : 0];
      if (selected) openResult(selected);
    }
  };
  const cycleShortcut = (shortcut: SearchShortcut) => {
    const group = shortcutGroup(shortcut.kind);
    setFilters((current) => cycleCatalogFacet(current, group, shortcut.key));
  };

  return createPortal(
    <section className="search-modal" role="dialog" aria-modal="true" aria-label={t("search.title")}>
      <header className="search-modal-header">
        <div className="search-modal-heading">
          <Search aria-hidden="true" />
          <strong>{t("search.title")}</strong>
          <span>{t("search.subtitle")}</span>
        </div>
        <button className="search-modal-close" type="button" onClick={onClose} aria-label={t("search.close")}>
          <X aria-hidden="true" />
        </button>
      </header>

      <div className="search-modal-query">
        <Search aria-hidden="true" />
        <input
          ref={input}
          value={text}
          type="search"
          spellCheck="false"
          autoComplete="off"
          placeholder={t("search.placeholder")}
          aria-label={t("search.placeholder")}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={handleInputKeyDown}
        />
        {text && (
          <button type="button" onClick={() => setText("")} aria-label={t("search.clearQuery")}>
            <X aria-hidden="true" />
          </button>
        )}
        <kbd>ESC</kbd>
      </div>

      <SearchIndexStatePanel
        status={status}
        progress={currentRebuild}
        loading={indexStatus.isPending || rebuild.isPending}
        error={rebuild.error ?? cancelRebuild.error ?? rebuildProgress.error ?? indexStatus.error}
        compact={debouncedText.length > 0}
        onRebuild={() => rebuild.mutate()}
        onCancel={(operationId) => cancelRebuild.mutate(operationId)}
        cancelling={cancelRebuild.isPending}
      />

      {debouncedText.length === 0 ? (
        <SearchStart
          recent={recent}
          onSelect={setText}
          onClear={() => {
            clearRecentSearches();
            setRecent([]);
          }}
        />
      ) : (
        <div className="search-modal-results">
          {(groupedShortcuts.genre.length > 0 || groupedShortcuts.circle.length > 0) && (
            <div className="search-shortcuts">
              <ShortcutGroup
                icon={<Tag aria-hidden="true" />}
                title={t("search.tags")}
                shortcuts={groupedShortcuts.genre}
                filters={filters}
                englishLabels={locale !== "ja-JP"}
                onCycle={cycleShortcut}
              />
              <ShortcutGroup
                icon={<CircleUserRound aria-hidden="true" />}
                title={t("search.circles")}
                shortcuts={groupedShortcuts.circle}
                filters={filters}
                englishLabels={locale !== "ja-JP"}
                onCycle={cycleShortcut}
              />
            </div>
          )}

          <SearchActiveFilters />

          {results.isFetching && !results.data && (
            <div className="search-progress" aria-live="polite">
              <LoaderCircle aria-hidden="true" />
              <span>{t("search.searching")}</span>
            </div>
          )}
          {results.isError && (
            <div className="search-error" role="alert">
              <AlertTriangle aria-hidden="true" />
              <strong>{t("search.failed")}</strong>
              <span>{t("common.technicalDetail", { detail: errorMessage(results.error, t("search.unknownError")) })}</span>
              <button type="button" onClick={() => void results.refetch()}>{t("detail.tryAgain")}</button>
            </div>
          )}
          {results.data && (
            <>
              <div className="search-result-summary" aria-live="polite">
                <strong>{t("search.resultCount", { count: total.toLocaleString(locale) })}</strong>
                <span>{results.data.queryKind === "archive_hash" ? t("search.exactHash") : t("search.relevance")}</span>
              </div>
              {items.length > 0 ? (
                <div className="search-result-grid">
                  {items.map((item, index) => (
                    <div
                      className={`search-result-item ${selectedIndex === index ? "selected" : ""}`}
                      onMouseEnter={() => setSelectedIndex(index)}
                      key={item.work.code}
                    >
                      <CatalogWorkCard
                        work={item.work}
                        englishLabels={locale !== "ja-JP"}
                        animationIndex={index}
                        onOpenWork={() => openResult(item)}
                      />
                    </div>
                  ))}
                </div>
              ) : (
                <div className="search-empty">
                  <Search aria-hidden="true" />
                  <strong>{t("search.noResults")}</strong>
                  <span>{t("search.noResultsHelp")}</span>
                </div>
              )}
              <CatalogPager
                currentPage={page}
                visible={items.length}
                total={total}
                totalPages={totalPages}
                onNavigate={setPage}
              />
            </>
          )}
        </div>
      )}

      <footer className="search-modal-footer">
        <span><kbd>↑</kbd><kbd>↓</kbd> {t("search.navigate")}</span>
        <span><kbd>↵</kbd> {t("search.open")}</span>
        <span><kbd>ESC</kbd> {t("search.close")}</span>
        {status?.state === "ready" && (
          <div className="search-index-actions">
            <button type="button" disabled={rebuild.isPending || rebuildActive} onClick={() => rebuild.mutate()}>
              <Database aria-hidden="true" />
              {t("search.indexed", { count: status.indexedDocuments.toLocaleString(locale) })}
              <RotateCw aria-hidden="true" />
            </button>
            <button type="button" disabled={cleanupCache.isPending || rebuildActive} onClick={() => cleanupCache.mutate()}>
              <Trash2 aria-hidden="true" />
              {t("search.cleanCache")}
            </button>
          </div>
        )}
        {cleanupCache.data && (
          <span className="search-cache-result" role="status">
            {cleanupCache.data.removedCompleteGenerations + cleanupCache.data.removedIncompleteGenerations > 0
              ? t("search.cacheCleaned", {
                count: (cleanupCache.data.removedCompleteGenerations + cleanupCache.data.removedIncompleteGenerations).toLocaleString(locale),
              })
              : t("search.cacheAlreadyClean")}
          </span>
        )}
        {cleanupCache.error && (
          <span className="search-cache-result is-error" role="alert">
            {t("common.requestFailed", { error: errorMessage(cleanupCache.error, t("search.unknownError")) })}
          </span>
        )}
      </footer>
    </section>,
    document.body,
  );
}

function SearchStart({
  recent,
  onSelect,
  onClear,
}: {
  recent: string[];
  onSelect: (query: string) => void;
  onClear: () => void;
}) {
  const { t } = usePresentation();
  return (
    <div className="search-start">
      <div className="search-start-mark"><Search aria-hidden="true" /></div>
      <strong>{t("search.startTyping")}</strong>
      <span>{t("search.startHelp")}</span>
      {recent.length > 0 && (
        <section className="search-recent">
          <header>
            <span><History aria-hidden="true" />{t("search.recent")}</span>
            <button type="button" onClick={onClear}>{t("search.clearRecent")}</button>
          </header>
          <div>
            {recent.map((query) => (
              <button type="button" onClick={() => onSelect(query)} key={query}>
                <Clock3 aria-hidden="true" />
                {query}
              </button>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

function ShortcutGroup({
  icon,
  title,
  shortcuts,
  filters,
  englishLabels,
  onCycle,
}: {
  icon: ReactNode;
  title: string;
  shortcuts: SearchShortcut[];
  filters: ReturnType<typeof useCatalogFacetFilters>["filters"];
  englishLabels: boolean;
  onCycle: (shortcut: SearchShortcut) => void;
}) {
  const first = shortcuts[0];
  if (!first) return null;
  const group = shortcutGroup(first.kind);
  return (
    <section>
      <header>{icon}<span>{title}</span></header>
      <div>
        {shortcuts.map((shortcut) => (
          <CatalogFilterChip
            label={englishLabels && shortcut.labelEnglish ? shortcut.labelEnglish : shortcut.label}
            count={shortcut.count}
            state={catalogFacetState(filters, group, shortcut.key)}
            onCycle={() => onCycle(shortcut)}
            key={`${shortcut.kind}:${shortcut.key}`}
          />
        ))}
      </div>
    </section>
  );
}

function SearchActiveFilters() {
  const { t } = usePresentation();
  const { filters, setFilters } = useCatalogFacetFilters();
  const selected = (["genres", "circles"] as const).flatMap((group) => [
    ...filters[group].include.map((key) => ({ group, key, state: "include" as const })),
    ...filters[group].exclude.map((key) => ({ group, key, state: "exclude" as const })),
  ]);
  if (selected.length === 0) return null;
  return (
    <div className="search-active-filters">
      <strong>{t("filter.active")}</strong>
      {selected.map(({ group, key, state }) => (
        <CatalogFilterChip
          label={key}
          state={state}
          onCycle={() => setFilters((current) => cycleCatalogFacet(current, group, key))}
          onRemove={() => setFilters((current) => setCatalogFacetState(current, group, key, "off"))}
          key={`${group}:${key}`}
        />
      ))}
    </div>
  );
}

function SearchIndexStatePanel({
  status,
  progress,
  loading,
  error,
  compact,
  onRebuild,
  onCancel,
  cancelling,
}: {
  status: SearchIndexStatus | undefined;
  progress: SearchRebuildProgress | null | undefined;
  loading: boolean;
  error: unknown;
  compact: boolean;
  onRebuild: () => void;
  onCancel: (operationId: string) => void;
  cancelling: boolean;
}) {
  const { locale, t } = usePresentation();
  const active = Boolean(progress && !searchRebuildIsTerminal(progress.stage));
  const cancelled = progress?.stage === "cancelled";
  const failed = progress?.stage === "failed" || status?.state === "failed" || Boolean(error);
  const state = active || loading ? "building" : status?.state;
  if (state === "ready" && !failed && !cancelled) return null;
  const percent = progress && progress.totalDocuments > 0
    ? Math.min(100, (progress.indexedDocuments / progress.totalDocuments) * 100)
    : null;
  return (
    <div className={`search-index-state ${compact ? "compact" : ""} ${failed ? "failed" : ""} ${cancelled ? "cancelled" : ""}`} role={failed ? "alert" : "status"}>
      {failed ? <AlertTriangle aria-hidden="true" /> : cancelled ? <X aria-hidden="true" /> : <LoaderCircle aria-hidden="true" />}
      <div>
        <strong>{t(failed ? "search.indexFailed" : cancelled ? "search.indexCancelled" : "search.preparingIndex")}</strong>
        {active && progress && (
          <>
            <span>{t(`search.rebuildStage.${progress.stage}` as MessageKey)}</span>
            <div
              className={`search-index-progress${percent === null ? " is-indeterminate" : ""}`}
              role="progressbar"
              aria-label={t("search.indexProgressLabel")}
              aria-valuemin={0}
              aria-valuemax={progress.totalDocuments || undefined}
              aria-valuenow={progress.totalDocuments > 0 ? progress.indexedDocuments : undefined}
            >
              <i style={percent === null ? undefined : { width: `${percent}%` }} />
            </div>
            {progress.totalDocuments > 0 && (
              <span>{t("search.indexProgress", {
                current: progress.indexedDocuments.toLocaleString(locale),
                total: progress.totalDocuments.toLocaleString(locale),
              })}</span>
            )}
          </>
        )}
        {failed && <span>{t("common.technicalDetail", { detail: errorMessage(error ?? progress?.detail ?? status?.detail, t("search.unknownError")) })}</span>}
      </div>
      {active && progress ? (
        <button type="button" disabled={cancelling} onClick={() => onCancel(progress.operationId)}>
          {t(cancelling ? "search.cancellingIndex" : "search.cancelIndex")}
        </button>
      ) : (failed || cancelled) ? (
        <button type="button" onClick={onRebuild}>{t(failed ? "search.retryIndex" : "search.resumeIndex")}</button>
      ) : null}
    </div>
  );
}

function shortcutGroup(kind: SearchShortcutKind): Extract<CatalogFacetGroup, "genres" | "circles"> {
  return kind === "genre" ? "genres" : "circles";
}

function isArchiveHash(text: string): boolean {
  return /^(?:[a-f\d]{32}|[a-f\d]{40}|[a-f\d]{64})$/i.test(text);
}

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : String(error ?? fallback);
}
