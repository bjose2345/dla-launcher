import { readStoredText, writeStoredText } from "../../preferences/browserStorage";
import type { CatalogViewMode } from "./catalogTimeline";

const VIEW_STORAGE_KEY = "dla-launcher:catalog-view";

export function storedCatalogViewMode(): CatalogViewMode {
  return readStoredText(VIEW_STORAGE_KEY) === "line" ? "line" : "grid";
}

export function storeCatalogViewMode(view: CatalogViewMode): void {
  writeStoredText(VIEW_STORAGE_KEY, view);
}
