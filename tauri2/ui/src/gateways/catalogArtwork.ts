import type {
  CatalogBrowsePage,
  CatalogRecommendations,
  CatalogWork,
  CatalogWorkDetail,
} from "@dla-launcher/shared-ui/catalog";
import { convertFileSrc } from "@tauri-apps/api/core";

export function cacheCatalogArtworkUrl(source: string): string {
  try {
    if (new URL(source).protocol !== "https:") return source;
  } catch {
    return source;
  }
  return convertFileSrc(source, "dla-cover");
}

export function cacheCatalogWork<T extends CatalogWork>(work: T): T {
  return {
    ...work,
    mainImageUrls: work.mainImageUrls.map(cacheCatalogArtworkUrl),
    thumbnailUrls: work.thumbnailUrls.map(cacheCatalogArtworkUrl),
  };
}

export function cacheCatalogBrowsePage(page: CatalogBrowsePage): CatalogBrowsePage {
  return {
    ...page,
    items: page.items.map(cacheCatalogWork),
  };
}

export function cacheCatalogWorkDetail(work: CatalogWorkDetail): CatalogWorkDetail {
  return {
    ...cacheCatalogWork(work),
    sampleImageUrls: work.sampleImageUrls.map(cacheCatalogArtworkUrl),
    relatedWorks: work.relatedWorks.map((related) => ({
      ...related,
      thumbnailUrls: related.thumbnailUrls.map(cacheCatalogArtworkUrl),
    })),
  };
}

export function cacheCatalogRecommendations(
  recommendations: CatalogRecommendations,
): CatalogRecommendations {
  return {
    ...recommendations,
    lanes: recommendations.lanes.map((lane) => ({
      ...lane,
      items: lane.items.map((item) => ({
        ...item,
        work: cacheCatalogWork(item.work),
      })),
    })),
  };
}
