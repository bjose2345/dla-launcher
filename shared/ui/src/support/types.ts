export type SupportFaultKind =
  | "rustPanic"
  | "frontendRender"
  | "frontendError"
  | "unhandledRejection"
  | "startupFailure";

export interface FrontendFaultReport {
  kind: SupportFaultKind;
  message: string;
  stack: string;
  componentStack: string;
}

export interface SupportFaultSummary {
  kind: SupportFaultKind;
  occurredAt: string;
  message: string;
}

export interface SupportStatus {
  schemaVersion: number;
  previousShutdownUnclean: boolean;
  previousRunId: string;
  lastFault: SupportFaultSummary | null;
  retainedLogFiles: number;
  retainedFaultFiles: number;
  estimatedBundleBytes: number;
  maxBundleBytes: number;
  summary: string;
}

export interface SupportSaveResult {
  outcome: "saved" | "cancelled";
  fileName: string;
  bytes: number;
}

export interface SupportGateway {
  readStatus(): Promise<SupportStatus>;
  acknowledgeUncleanShutdown(): Promise<void>;
  recordFrontendFault(report: FrontendFaultReport): Promise<void>;
  saveBundle(): Promise<SupportSaveResult>;
  openIssue(): Promise<void>;
  openProject(): Promise<void>;
}
