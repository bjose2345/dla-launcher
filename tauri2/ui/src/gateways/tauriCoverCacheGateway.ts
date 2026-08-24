import type {
  CoverCacheCapacity,
  CoverCacheGateway,
  CoverCacheRetention,
  CoverCacheSummary,
} from "@dla-launcher/shared-ui/preferences";
import { invoke } from "@tauri-apps/api/core";

export const tauriCoverCacheGateway: CoverCacheGateway = {
  readSummary(): Promise<CoverCacheSummary> {
    return invoke("read_cover_cache_summary");
  },
  configure(
    retention: CoverCacheRetention,
    capacity: CoverCacheCapacity,
  ): Promise<CoverCacheSummary> {
    return invoke("configure_cover_cache", { retention, capacity });
  },
};
