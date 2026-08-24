export type AndroidPackageCapabilityStatus = "unavailable" | "approval_required" | "ready";
export type AndroidPackageBlockReason =
  | "incompatible_sdk"
  | "split_package"
  | "self_update"
  | "missing_signature";
export type AndroidPackageInstallState =
  | "approval_required"
  | "preparing"
  | "awaiting_user_confirmation"
  | "installed"
  | "cancelled"
  | "failed";

export interface AndroidPackageCapability {
  status: AndroidPackageCapabilityStatus;
  deviceSdk: number | null;
}

export interface AndroidPackageInspection {
  selectionId: string;
  displayName: string;
  applicationLabel: string;
  packageName: string;
  versionName: string | null;
  versionCode: string;
  sizeBytes: number;
  sha256: string;
  minimumSdk: number | null;
  targetSdk: number | null;
  signingCertificateSha256: string[];
  installable: boolean;
  blockReason: AndroidPackageBlockReason | null;
}

export interface AndroidPackageInstallStatus {
  operationId: string;
  selectionId: string;
  state: AndroidPackageInstallState;
  technicalDetail: string | null;
}

export interface AndroidPackageState {
  capability: AndroidPackageCapability;
  inspection: AndroidPackageInspection | null;
  installStatus: AndroidPackageInstallStatus | null;
}

export interface AndroidPackageGateway {
  readState(): Promise<AndroidPackageState>;
  selectAndInspect(): Promise<AndroidPackageState>;
  clearSelection(): Promise<AndroidPackageState>;
  openSourceApproval(): Promise<AndroidPackageState>;
  requestInstall(): Promise<AndroidPackageState>;
}

export type AndroidAppRuntimeState =
  | "ready"
  | "not_launchable"
  | "missing"
  | "signer_mismatch"
  | "unavailable";

export interface AndroidAppAssociation {
  id: string;
  workCode: string;
  packageName: string;
  applicationLabel: string;
  expectedSigningCertificateSha256: string[];
  associatedVersionName: string | null;
  associatedVersionCode: string;
  associatedAt: string;
  updatedAt: string;
  lastLaunchedAt: string | null;
  launchCount: number;
}

export interface AndroidAppRuntimeStatus {
  state: AndroidAppRuntimeState;
  applicationLabel: string | null;
  versionName: string | null;
  versionCode: string | null;
  technicalDetail: string | null;
}

export interface AndroidAppView {
  association: AndroidAppAssociation;
  runtime: AndroidAppRuntimeStatus;
}

export interface AndroidAppGateway {
  list(): Promise<AndroidAppView[]>;
  associateInstalled(workCode: string): Promise<AndroidAppView>;
  launch(associationId: string): Promise<AndroidAppView>;
  remove(associationId: string): Promise<void>;
}
