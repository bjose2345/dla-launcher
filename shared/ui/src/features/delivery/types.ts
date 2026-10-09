export type DeliveryStatus =
  | "queued"
  | "downloading"
  | "installing"
  | "paused"
  | "installed"
  | "needs_review"
  | "failed"
  | "cancelled";

export type DeliveryProblem =
  | "sign_in_required"
  | "credentials"
  | "too_many_attempts"
  | "unreachable"
  | "not_in_archive"
  | "allowance"
  | "busy"
  | "disk_space"
  | "corrupt"
  | "install_folder"
  | "install"
  | "unavailable";

export type DeliveryPhase = "locating" | "checking" | "receiving" | "verifying" | "unpacking" | "installing";

export interface DeliveryArchive {
  fileName: string;
  size: number;
  sha256: string;
  remotePath: string | null;
  complete: boolean;
}

export interface DeliveryJob {
  id: string;
  workCode: string;
  releaseDate: string;
  version: string;
  updateDate: string;
  archives: DeliveryArchive[];
  installFolder: string;
  status: DeliveryStatus;
  receivedBytes: number;
  problem: DeliveryProblem | null;
  detail: string | null;
  installationId: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface DeliveryProgress {
  jobId: string;
  workCode: string;
  phase: DeliveryPhase;
  receivedBytes: number;
  totalBytes: number;
}

export interface DeliveryView {
  /** Counts changes on the Rust side; the higher revision is the newer view. */
  revision: number;
  /** Whether this build can reach DLA at all. */
  available: boolean;
  account: string | null;
  installFolder: string | null;
  jobs: DeliveryJob[];
  progress: DeliveryProgress | null;
}

/** A refused command: a stable code plus untranslated technical detail. */
export interface DeliveryFailure {
  problem: DeliveryProblem;
  detail: string;
}

export interface DeliveryEvents {
  onChange(view: DeliveryView): void;
  onProgress(progress: DeliveryProgress): void;
}

export interface DeliveryGateway {
  read(): Promise<DeliveryView>;
  signIn(username: string, password: string): Promise<DeliveryView>;
  signOut(): Promise<DeliveryView>;
  install(workCode: string): Promise<DeliveryView>;
  pause(jobId: string): Promise<DeliveryView>;
  resume(jobId: string): Promise<DeliveryView>;
  cancel(jobId: string): Promise<DeliveryView>;
  remove(jobId: string): Promise<DeliveryView>;
  /** Resolves to null when the folder picker is closed. */
  chooseInstallFolder(): Promise<DeliveryView | null>;
  subscribe(events: DeliveryEvents): () => void;
}

const PROBLEMS: readonly DeliveryProblem[] = [
  "sign_in_required",
  "credentials",
  "too_many_attempts",
  "unreachable",
  "not_in_archive",
  "allowance",
  "busy",
  "disk_space",
  "corrupt",
  "install_folder",
  "install",
  "unavailable",
];

export function deliveryFailure(error: unknown): DeliveryFailure {
  if (typeof error === "object" && error !== null && "problem" in error) {
    const { problem, detail } = error as { problem: unknown; detail?: unknown };
    if (PROBLEMS.includes(problem as DeliveryProblem)) {
      return { problem: problem as DeliveryProblem, detail: typeof detail === "string" ? detail : "" };
    }
  }
  return { problem: "unavailable", detail: error instanceof Error ? error.message : String(error) };
}

export function isActive(job: DeliveryJob): boolean {
  return job.status === "queued" || job.status === "downloading" || job.status === "installing";
}

/** The newest job for a work, if any. */
export function latestJob(view: DeliveryView | undefined, workCode: string): DeliveryJob | null {
  const jobs = (view?.jobs ?? []).filter((job) => job.workCode.toLowerCase() === workCode.toLowerCase());
  return jobs.reduce<DeliveryJob | null>(
    (latest, job) => (latest === null || job.createdAt >= latest.createdAt ? job : latest),
    null,
  );
}

export function totalBytes(job: DeliveryJob): number {
  return job.archives.reduce((sum, archive) => sum + archive.size, 0);
}

/** Bytes held for a job, from live progress while it runs. */
export function receivedBytes(job: DeliveryJob, progress: DeliveryProgress | null): number {
  return progress?.jobId === job.id ? progress.receivedBytes : job.receivedBytes;
}
