export type ScanStatus =
  | "queued"
  | "running"
  | "completed"
  | "cancelled"
  | "interrupted"
  | "failed";

export type ScanMatchConfidence = "possible" | "strong" | "exact";

export interface ScanCounters {
  discoveredFiles: number;
  discoveredDirectories: number;
  inspectedFiles: number;
  matched: number;
  ambiguous: number;
  unmatched: number;
  recoverableErrors: number;
}

export interface ScanRoot {
  id: string;
  platform: string;
  pathKey: string;
  displayPath: string;
  createdAt: string;
  updatedAt: string;
}

export interface ScanSession {
  id: string;
  rootId: string;
  status: ScanStatus;
  options: {
    followSymlinks: boolean;
    hashPolicy: "candidate_archives";
    workerLimit: number;
    purpose?: "library" | "install";
  };
  counters: ScanCounters;
  startedAt: string;
  finishedAt: string | null;
  fatalErrorCode: string | null;
  fatalErrorMessage: string | null;
}

export interface ScanSessionView {
  root: ScanRoot;
  session: ScanSession;
}

export interface SelectedScanRoot {
  accessHandle: string;
  displayPath: string;
}

export type ScanRootPreferenceSource = "configured" | "platform_default" | "unavailable";

export interface ScanRootPreference {
  platform: string;
  displayPath: string | null;
  source: ScanRootPreferenceSource;
  available: boolean;
  canPrepare: boolean;
}

export const scannerRootPreferenceKey = ["scanner", "root-preference"] as const;

export interface ScannerRootPreferenceGateway {
  readRootPreference(): Promise<ScanRootPreference>;
  selectPreferredRoot(): Promise<ScanRootPreference | null>;
  resetPreferredRoot(): Promise<ScanRootPreference>;
  preparePreferredRoot(): Promise<SelectedScanRoot>;
}

export interface ScanProgress {
  sessionId: string;
  status: ScanStatus;
  counters: ScanCounters;
  currentRelativePath: string | null;
}

export type ScanFindingKind = "folder" | "archive" | "file";

/** What adding a finding needs from the user. */
export type ScanFindingState = "ready" | "choose" | "unknown" | "mixed" | "incomplete";

export interface ScanFindingCandidate {
  workCode: string;
  confidence: ScanMatchConfidence;
}

/** One work a scan found: an archive set, a folder, a file or the whole root. */
export interface ScanFinding {
  key: string;
  /** Below the scan root; empty when the whole root is one work. */
  relativePath: string;
  kind: ScanFindingKind;
  state: ScanFindingState;
  workCode: string | null;
  candidates: ScanFindingCandidate[];
  fileCount: number;
  sizeBytes: number;
  volumeCount: number;
  /** Why an archive set is incomplete: parts no file holds, and files to rename so 7-Zip finds them. */
  incompleteSet: { missingParts: number[]; renames: Array<{ from: string; to: string }> } | null;
  /** The Library entry for this work, and the catalog work it is matched to now. */
  inLibrary: { installationId: string; sameFiles: boolean; workCode: string | null } | null;
  ignored: boolean;
}

export interface ScanFindingList {
  sessionId: string;
  rootId: string;
  rootPath: string;
  findings: ScanFinding[];
}

export interface AddScanFinding {
  key: string;
  /** The catalog work the user picked. */
  workCode?: string | null;
  /** Adds the finding without a catalog work. */
  unknown?: boolean;
}

export type ScanAddOutcome = "added" | "installing" | "already_in_library" | "refused";
export type ScanFindingRefusal = "not_found" | "choose_work" | "mixed" | "incomplete" | "failed";

export interface ScanAddResult {
  key: string;
  outcome: ScanAddOutcome;
  installationId: string | null;
  refusal: ScanFindingRefusal | null;
  detail: string | null;
}

export type ScanInstallStatus =
  | "queued"
  | "installing"
  | "installed"
  | "needs_review"
  | "failed"
  | "cancelled";

/** An archive the scanner found, installing so it can join the Library. */
export interface ScanInstall {
  installationId: string;
  key: string;
  name: string;
  workCode: string | null;
  status: ScanInstallStatus;
  detail: string | null;
}

export interface ScanInstallView {
  /** Counts changes; the newest view wins when replies and events cross. */
  revision: number;
  installFolder: string | null;
  installs: ScanInstall[];
}

export interface ScanAddResponse {
  results: ScanAddResult[];
  installs: ScanInstallView;
}

export const scanInstallsKey = ["scanner", "installs"] as const;

export function scanInstallIsActive(status: ScanInstallStatus): boolean {
  return status === "queued" || status === "installing";
}

/** Installs archives into the install folder Install from DLA also uses. */
export interface ScanInstallGateway {
  readInstalls(): Promise<ScanInstallView>;
  subscribeInstalls(listener: (view: ScanInstallView) => void): Promise<() => void>;
  selectInstallFolder(): Promise<ScanInstallView | null>;
  installArchive(installationId: string): Promise<ScanInstallView>;
  cancelInstall(installationId: string): Promise<ScanInstallView>;
}

export interface ScanIssue {
  id: string;
  sessionId: string;
  entryId: string | null;
  relativePath: string | null;
  code: string;
  message: string;
  recoverable: boolean;
  createdAt: string;
}

export interface ScanIssuePage {
  items: ScanIssue[];
  total: number;
  limit: number;
  offset: number;
}

export interface ScannerGateway extends ScannerRootPreferenceGateway, ScanInstallGateway {
  selectRoot(): Promise<SelectedScanRoot | null>;
  start(accessHandle: string): Promise<ScanSessionView>;
  cancel(sessionId: string): Promise<boolean>;
  readLatest(): Promise<ScanSessionView | null>;
  listFindings(sessionId: string): Promise<ScanFindingList>;
  setFindingIgnored(rootId: string, key: string, ignored: boolean): Promise<void>;
  addFindings(sessionId: string, findings: AddScanFinding[]): Promise<ScanAddResponse>;
  browseIssues(request: {
    sessionId: string;
    limit?: number;
    offset?: number;
  }): Promise<ScanIssuePage>;
  subscribeProgress(listener: (progress: ScanProgress) => void): Promise<() => void>;
}
