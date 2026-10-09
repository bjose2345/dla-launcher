import { browserStorage } from "../../preferences/browserStorage";
import type { CatalogViewMode } from "./catalogTimeline";

const VIEW_STORAGE_KEY = "dla-launcher:catalog-view";

export function storedCatalogViewMode(): CatalogViewMode {
  return browserStorage()?.getItem(VIEW_STORAGE_KEY) === "line" ? "line" : "grid";
}

export function storeCatalogViewMode(view: CatalogViewMode): void {
  try {
    browserStorage()?.setItem(VIEW_STORAGE_KEY, view);
  } catch {
    return;
  }
}
