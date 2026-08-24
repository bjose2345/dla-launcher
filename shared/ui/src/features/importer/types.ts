export type CatalogPackageProfile = "compact" | "full" | "custom";
export type CatalogPayloadKind = "dat" | "enrichment" | "relations";
export type CatalogGenerationKind = "embedded" | "imported";
export type CatalogGenerationState = "active" | "available" | "failed";
export type CatalogImportStage =
  | "queued"
  | "validating"
  | "building_catalog"
  | "applying_enrichment"
  | "applying_relations"
  | "finalizing_catalog"
  | "checkpointing_catalog"
  | "validating_catalog"
  | "activating_catalog"
  | "rebuilding_search"
  | "completed"
  | "cancelled"
  | "failed";
export type CatalogImportOperationKind = "import" | "activation";

export interface CatalogPackageManifest {
  format: string;
  formatVersion: number;
  catalogSchemaVersion: number;
  minimumLauncherVersion: string;
  snapshotId: string;
  createdAt: string;
  profile: CatalogPackageProfile;
  source: { id: string; name: string };
  fields: string[];
  counts: {
    workEntries: number;
    uniqueWorks: number;
    roms: number;
    files: number;
    relations: number;
  };
  payloads: Array<{
    path: string;
    kind: CatalogPayloadKind;
    mediaType: string;
    records: number;
    uncompressedBytes: number;
    sha256: string;
  }>;
}

export interface SelectedCatalogPackage {
  accessHandle: string;
  displayName: string;
}

export interface CatalogImportPreview {
  accessHandle: string;
  displayName: string;
  compressedBytes: number;
  uncompressedBytes: number;
  requiredDiskBytes: number;
  availableDiskBytes: number;
  compatible: boolean;
  blockingIssues: string[];
  warnings: string[];
  manifest: CatalogPackageManifest;
  omittedFields: string[];
}

export interface CatalogImportCounters {
  processedBytes: number;
  totalBytes: number;
  workEntries: number;
  uniqueWorks: number;
  roms: number;
  files: number;
  relations: number;
}

export interface CatalogImportProgress {
  operationId: string;
  operationKind: CatalogImportOperationKind;
  snapshotId: string;
  stage: CatalogImportStage;
  counters: CatalogImportCounters;
  currentPayload: string;
  detail: string;
}

export interface CatalogGenerationSummary {
  id: string;
  snapshotId: string;
  kind: CatalogGenerationKind;
  state: CatalogGenerationState;
  profile: CatalogPackageProfile;
  sourceName: string;
  packageName: string;
  importedAt: string;
  workCount: number;
  romCount: number;
  databaseBytes: number;
  fields: string[];
  failureDetail: string;
}

export interface CatalogImportGateway {
  selectPackage(): Promise<SelectedCatalogPackage | null>;
  inspect(accessHandle: string): Promise<CatalogImportPreview>;
  start(accessHandle: string): Promise<CatalogImportProgress>;
  cancel(operationId: string): Promise<boolean>;
  readProgress(): Promise<CatalogImportProgress | null>;
  listGenerations(): Promise<CatalogGenerationSummary[]>;
  activate(generationId: string): Promise<CatalogImportProgress>;
  removeGeneration(generationId: string): Promise<void>;
  subscribeProgress(listener: (progress: CatalogImportProgress) => void): Promise<() => void>;
}

export function catalogImportIsTerminal(stage: CatalogImportStage | undefined): boolean {
  return stage === "completed" || stage === "cancelled" || stage === "failed";
}

export function catalogImportIsIndeterminate(stage: CatalogImportStage | undefined): boolean {
  return stage === "finalizing_catalog"
    || stage === "checkpointing_catalog"
    || stage === "validating_catalog"
    || stage === "activating_catalog"
    || stage === "rebuilding_search";
}

export type CatalogImportPhase = "checking" | "building" | "details" | "finishing";

export const catalogImportPhases: readonly CatalogImportPhase[] = [
  "checking",
  "building",
  "details",
  "finishing",
];

export function catalogImportPhase(stage: CatalogImportStage): CatalogImportPhase {
  switch (stage) {
    case "queued":
    case "validating":
      return "checking";
    case "building_catalog":
      return "building";
    case "applying_enrichment":
    case "applying_relations":
      return "details";
    default:
      return "finishing";
  }
}

export function catalogImportPercent(progress: CatalogImportProgress): number | null {
  if (catalogImportIsIndeterminate(progress.stage)) return null;
  const { processedBytes, totalBytes } = progress.counters;
  if (totalBytes <= 0 || processedBytes === 0) return null;
  const percent = Math.max(0, Math.min(100, Math.round((processedBytes / totalBytes) * 100)));
  return processedBytes > 0 && percent === 0 ? null : percent;
}
