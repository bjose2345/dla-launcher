import type { CatalogFacetFilters, CatalogWork } from "../catalog/types";

export type SearchIndexState = "missing" | "building" | "ready" | "stale" | "failed";
export type SearchQueryKind = "text" | "archive_hash";
export type SearchShortcutKind = "genre" | "circle";
export type SearchRebuildStage =
  | "queued"
  | "indexing"
  | "committing"
  | "cleaning"
  | "completed"
  | "cancelled"
  | "failed";

export interface SearchIndexStatus {
  state: SearchIndexState;
  schemaVersion: number;
  catalogSnapshotId: string;
  indexedDocuments: number;
  generation: string;
  indexPath: string;
  detail: string;
}

export interface SearchRebuildProgress {
  operationId: string;
  stage: SearchRebuildStage;
  indexedDocuments: number;
  totalDocuments: number;
  detail: string;
}

export interface SearchCacheCleanupReport {
  removedIncompleteGenerations: number;
  removedCompleteGenerations: number;
  reclaimedBytes: number;
  retainedCompleteGenerations: number;
}

export interface SearchRequest {
  text: string;
  facets: CatalogFacetFilters;
  limit: number;
  offset: number;
}

export interface SearchResultItem {
  work: CatalogWork;
  score: number;
}

export interface SearchResponse {
  items: SearchResultItem[];
  total: number;
  limit: number;
  offset: number;
  queryKind: SearchQueryKind;
}

export interface SearchShortcut {
  kind: SearchShortcutKind;
  key: string;
  label: string;
  labelEnglish: string;
  count: number;
}

export interface SearchGateway {
  status(): Promise<SearchIndexStatus>;
  rebuild(): Promise<SearchRebuildProgress>;
  cancelRebuild(operationId: string): Promise<boolean>;
  readRebuildProgress(): Promise<SearchRebuildProgress | null>;
  cleanupCache(): Promise<SearchCacheCleanupReport>;
  subscribeRebuildProgress(
    listener: (progress: SearchRebuildProgress) => void,
  ): Promise<() => void>;
  search(request: SearchRequest): Promise<SearchResponse>;
  shortcuts(text: string, limit: number): Promise<SearchShortcut[]>;
}

export function searchRebuildIsTerminal(stage: SearchRebuildStage | undefined): boolean {
  return stage === "completed" || stage === "cancelled" || stage === "failed";
}
