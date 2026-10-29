import type { CatalogWork } from "../catalog";
import type { ScanFinding } from "../scanner/types";

export type LibraryAddSource = "folders" | "archives" | "scan";
export type LibraryAddPhase = "scanning" | "review" | "adding" | "done" | "cancelled";
/** How a work will reach the install folder: `copy` is a move from another drive. */
export type LibraryAddPlacement = "move" | "copy" | "in_place" | "unpack";
export type LibraryAddJobState = "queued" | "running" | "done" | "failed" | "cancelled";

export interface LibraryAddSourceView {
  sourceId: string;
  /** The picked folder or file's own name. */
  displayName: string;
  kind: "folder" | "archive" | "scan";
  status: "queued" | "scanning" | "done" | "failed";
  error: string | null;
  sessionId: string | null;
}

export interface LibraryAddFinding {
  sourceId: string;
  finding: ScanFinding;
  placement: LibraryAddPlacement;
}

export interface LibraryAddJob {
  sourceId: string;
  key: string;
  kind: "move" | "copy" | "unpack" | "add";
  state: LibraryAddJobState;
  installationId: string | null;
  error: string | null;
  /** Something to know about a job that still succeeded, such as an original left behind. */
  warning: string | null;
  doneBytes: number | null;
  totalBytes: number | null;
}

/** One "+ Add": the picked folders or archives, the works found in them, and their arrival. */
export interface LibraryAddView {
  batchId: string;
  revision: number;
  phase: LibraryAddPhase;
  installFolder: string | null;
  deleteArchives: boolean;
  sources: LibraryAddSourceView[];
  findings: LibraryAddFinding[];
  jobs: LibraryAddJob[];
}

export interface LibraryAddItem {
  sourceId: string;
  key: string;
  workCode: string | null;
  /** Adds the work as the user's own, outside the catalog. */
  unknown: boolean;
}

export interface ConfirmLibraryAddRequest {
  batchId: string;
  items: LibraryAddItem[];
  deleteArchives: boolean;
}

export interface LibraryAddGateway {
  /** Opens the system picker; null when it is cancelled. */
  startLibraryAdd(source: LibraryAddSource): Promise<LibraryAddView | null>;
  readLibraryAdd(): Promise<LibraryAddView | null>;
  confirmLibraryAdd(request: ConfirmLibraryAddRequest): Promise<LibraryAddView>;
  cancelLibraryAdd(batchId: string): Promise<LibraryAddView | null>;
  dismissLibraryAdd(batchId: string): Promise<void>;
  subscribeLibraryAdd(listener: (view: LibraryAddView) => void): Promise<() => void>;
}

/**
 * What a found work is, for the review:
 * - `catalog`: matched to the catalog.
 * - `newer`: the catalog has a newer version than the folder's date says.
 * - `choose`: several catalog works fit.
 * - `own`: no catalog work fits; it is added as the user's own work.
 * - `library`: already in the Library.
 * - `incomplete`: an archive set with a part missing.
 * - `mixed`: files of several works in one folder.
 */
export type LibraryAddStatus = "catalog" | "newer" | "choose" | "own" | "library" | "incomplete" | "mixed";

/** What the user decided for a work the catalog can't name on its own. */
export type LibraryAddChoice = { kind: "work"; workCode: string } | { kind: "own" };

export interface FolderRelease {
  date: string;
  version: string | null;
}

const RELEASE_PATTERN = /\((\d{4})-(\d{2})-(\d{2})(?:-(\d{1,3}))?\)/;

/** The release date and version in a name such as `RJ01234567 (2024-05-01-01)`. */
export function folderRelease(name: string): FolderRelease | null {
  const match = RELEASE_PATTERN.exec(name);
  if (!match) return null;
  const [, year, month, day, version] = match;
  return { date: `${year}-${month}-${day}`, version: version ?? null };
}

export function addFindingKey(item: { sourceId: string; finding: { key: string } } | { sourceId: string; key: string }): string {
  return `${item.sourceId}:${"finding" in item ? item.finding.key : item.key}`;
}

/** The work's own name: its folder or archive, or the picked folder when that is the work. */
export function addFindingName(item: LibraryAddFinding, sources: readonly LibraryAddSourceView[]): string {
  const path = item.finding.relativePath || sources.find((source) => source.sourceId === item.sourceId)?.displayName || "";
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

export function addStatus(finding: ScanFinding, name: string, work: CatalogWork | undefined): LibraryAddStatus {
  if (finding.inLibrary?.sameFiles) return "library";
  switch (finding.state) {
    case "incomplete": return "incomplete";
    case "mixed": return "mixed";
    case "choose": return "choose";
    case "unknown": return "own";
    case "ready": {
      const release = folderRelease(name);
      const latest = work?.latestUpdate?.date;
      return release && latest && latest > release.date ? "newer" : "catalog";
    }
  }
}

/** Whether a work can be added, and whether it is ticked before the user does anything. */
export function addable(status: LibraryAddStatus): boolean {
  return status !== "library" && status !== "incomplete" && status !== "mixed";
}

/** The request for one ticked work, or null while a choice it needs is missing. */
export function addItem(
  item: LibraryAddFinding,
  status: LibraryAddStatus,
  choice: LibraryAddChoice | undefined,
): LibraryAddItem | null {
  const base = { sourceId: item.sourceId, key: item.finding.key };
  if (choice?.kind === "own") return { ...base, workCode: null, unknown: true };
  if (choice?.kind === "work") return { ...base, workCode: choice.workCode, unknown: false };
  if (status === "own") return { ...base, workCode: null, unknown: true };
  if (status === "catalog" || status === "newer") return { ...base, workCode: null, unknown: false };
  return null;
}

export function jobFor(view: LibraryAddView, item: LibraryAddFinding): LibraryAddJob | undefined {
  return view.jobs.find((job) => job.sourceId === item.sourceId && job.key === item.finding.key);
}

export function jobPercent(job: LibraryAddJob): number | null {
  if (job.doneBytes === null || !job.totalBytes) return null;
  return Math.max(0, Math.min(100, Math.round((job.doneBytes / job.totalBytes) * 100)));
}

export function libraryAddSettled(view: LibraryAddView): boolean {
  return view.phase === "done" || view.phase === "cancelled";
}
