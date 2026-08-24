export const coverCacheRetentions = ["days_90", "days_180", "days_360", "never"] as const;
export type CoverCacheRetention = (typeof coverCacheRetentions)[number];

export const coverCacheCapacities = ["standard", "large", "very_large", "unlimited"] as const;
export type CoverCacheCapacity = (typeof coverCacheCapacities)[number];

export interface CoverCacheSummary {
  retention: CoverCacheRetention;
  capacity: CoverCacheCapacity;
  entryCount: number;
  storedBytes: number;
  maximumBytes: number | null;
  maximumEntries: number | null;
}

export interface CoverCacheGateway {
  readSummary(): Promise<CoverCacheSummary>;
  configure(
    retention: CoverCacheRetention,
    capacity: CoverCacheCapacity,
  ): Promise<CoverCacheSummary>;
}
