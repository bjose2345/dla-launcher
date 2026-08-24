import type {
  SearchGateway,
  SearchCacheCleanupReport,
  SearchIndexStatus,
  SearchRebuildProgress,
  SearchRequest,
  SearchResponse,
  SearchShortcut,
} from "@dla-launcher/shared-ui/search";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { cacheCatalogWork } from "./catalogArtwork";

export const tauriSearchGateway: SearchGateway = {
  status(): Promise<SearchIndexStatus> {
    return invoke("read_search_index_status");
  },
  rebuild(): Promise<SearchRebuildProgress> {
    return invoke("rebuild_search_index");
  },
  cancelRebuild(operationId: string): Promise<boolean> {
    return invoke("cancel_search_index_rebuild", { operationId });
  },
  readRebuildProgress(): Promise<SearchRebuildProgress | null> {
    return invoke("read_search_index_rebuild_progress");
  },
  cleanupCache(): Promise<SearchCacheCleanupReport> {
    return invoke("cleanup_search_index_cache");
  },
  subscribeRebuildProgress(listener): Promise<() => void> {
    return listen<SearchRebuildProgress>("search-rebuild-progress", (event) => {
      listener(event.payload);
    });
  },
  async search(request: SearchRequest): Promise<SearchResponse> {
    const response = await invoke<SearchResponse>("search_catalog", { request });
    return {
      ...response,
      items: response.items.map((item) => ({ ...item, work: cacheCatalogWork(item.work) })),
    };
  },
  shortcuts(text: string, limit: number): Promise<SearchShortcut[]> {
    return invoke("search_catalog_shortcuts", { request: { text, limit } });
  },
};
