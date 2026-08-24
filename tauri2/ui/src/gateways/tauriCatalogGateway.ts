import type {
  CatalogBrowsePage,
  CatalogBrowseRequest,
  CatalogContext,
  CatalogContextRequest,
  CatalogDetailGateway,
  CatalogGateway,
  CatalogRecommendations,
  CatalogRomContents,
  CatalogWork,
  CatalogWorkDetail,
} from "@dla-launcher/shared-ui/catalog";
import { invoke } from "@tauri-apps/api/core";
import {
  cacheCatalogBrowsePage,
  cacheCatalogRecommendations,
  cacheCatalogWork,
  cacheCatalogWorkDetail,
} from "./catalogArtwork";

export const tauriCatalogGateway: CatalogGateway & CatalogDetailGateway = {
  async browse(request: CatalogBrowseRequest): Promise<CatalogBrowsePage> {
    return cacheCatalogBrowsePage(await invoke("browse_catalog", { request }));
  },
  context(request: CatalogContextRequest): Promise<CatalogContext> {
    return invoke("read_catalog_context", { request });
  },
  async read(code: string): Promise<CatalogWorkDetail> {
    return cacheCatalogWorkDetail(await invoke("read_catalog_work", { code }));
  },
  async readWorks(codes: string[]): Promise<CatalogWork[]> {
    return (await invoke<CatalogWork[]>("read_catalog_works", { codes })).map(cacheCatalogWork);
  },
  readRomContents(workCode: string, romPosition: number): Promise<CatalogRomContents> {
    return invoke("read_catalog_rom_contents", { workCode, romPosition });
  },
  async readRecommendations(code: string): Promise<CatalogRecommendations> {
    return cacheCatalogRecommendations(await invoke("read_catalog_recommendations", { code }));
  },
};
