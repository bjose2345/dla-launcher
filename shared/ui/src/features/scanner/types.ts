export type ScanStatus =
  | "queued"
  | "running"
  | "completed"
  | "cancelled"
  | "interrupted"
  | "failed";

export type ScanMatchOutcome = "matched" | "ambiguous" | "unmatched";
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

export interface ScanMatchCandidate {
  workCode: string;
  confidence: ScanMatchConfidence;
  reasonCodes: string[];
  rank: number;
}

export interface ScanEvidence {
  id: string;
  resultId: string;
  sourceEntryId: string | null;
  kind: string;
  normalizedValue: string;
  reasonCode: string;
  createdAt: string;
}

export interface ScanResult {
  id: string;
  sessionId: string;
  candidateEntryId: string | null;
  outcome: ScanMatchOutcome;
  selectedWorkCode: string | null;
  confidence: ScanMatchConfidence | null;
  candidates: ScanMatchCandidate[];
  evidence: ScanEvidence[];
  createdAt: string;
  updatedAt: string;
}

export interface ScanResultItem {
  result: ScanResult;
  relativePath: string | null;
}

export interface ScanResultPage {
  items: ScanResultItem[];
  total: number;
  limit: number;
  offset: number;
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

export interface ScannerGateway extends ScannerRootPreferenceGateway {
  selectRoot(): Promise<SelectedScanRoot | null>;
  start(accessHandle: string): Promise<ScanSessionView>;
  cancel(sessionId: string): Promise<boolean>;
  readLatest(): Promise<ScanSessionView | null>;
  browseResults(request: {
    sessionId: string;
    outcome?: ScanMatchOutcome;
    limit?: number;
    offset?: number;
  }): Promise<ScanResultPage>;
  browseIssues(request: {
    sessionId: string;
    limit?: number;
    offset?: number;
  }): Promise<ScanIssuePage>;
  createInstallation(sessionId: string, selectedResultId: string): Promise<{ id: string }>;
  subscribeProgress(listener: (progress: ScanProgress) => void): Promise<() => void>;
}
