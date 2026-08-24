import type {
  CatalogGenerationSummary,
  CatalogImportGateway,
  CatalogImportPreview,
  CatalogImportProgress,
  SelectedCatalogPackage,
} from "@dla-launcher/shared-ui/importer";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const tauriCatalogImportGateway: CatalogImportGateway = {
  selectPackage(): Promise<SelectedCatalogPackage | null> {
    return invoke("select_catalog_package");
  },
  inspect(accessHandle: string): Promise<CatalogImportPreview> {
    return invoke("inspect_catalog_package", { accessHandle });
  },
  start(accessHandle: string): Promise<CatalogImportProgress> {
    return invoke("start_catalog_import", { accessHandle });
  },
  cancel(operationId: string): Promise<boolean> {
    return invoke("cancel_catalog_import", { operationId });
  },
  readProgress(): Promise<CatalogImportProgress | null> {
    return invoke("read_catalog_import_progress");
  },
  listGenerations(): Promise<CatalogGenerationSummary[]> {
    return invoke("list_catalog_generations");
  },
  activate(generationId: string): Promise<CatalogImportProgress> {
    return invoke("activate_catalog_generation", { generationId });
  },
  removeGeneration(generationId: string): Promise<void> {
    return invoke("remove_catalog_generation", { generationId });
  },
  subscribeProgress(listener): Promise<() => void> {
    return listen<CatalogImportProgress>("catalog-import-progress", (event) => listener(event.payload));
  },
};
