import type { ScanFinding, ScanMatchConfidence } from "./types";

export type FindingSection = "ready" | "choose" | "unknown" | "blocked" | "library" | "ignored";

export const findingSections: FindingSection[] = [
  "ready",
  "choose",
  "unknown",
  "blocked",
  "library",
  "ignored",
];

/**
 * Where a finding is listed. A finding the user just acted on stays where it
 * was, so its row can confirm the result and offer Undo.
 */
export function findingSection(finding: ScanFinding, stays: boolean): FindingSection {
  if (finding.ignored && !stays) return "ignored";
  if (finding.inLibrary && !stays) return "library";
  switch (finding.state) {
    case "ready": return "ready";
    case "choose": return "choose";
    case "unknown": return "unknown";
    case "mixed":
    case "incomplete": return "blocked";
  }
}

export function groupFindings(
  findings: ScanFinding[],
  staying: ReadonlySet<string>,
): Record<FindingSection, ScanFinding[]> {
  const groups = Object.fromEntries(
    findingSections.map((section) => [section, [] as ScanFinding[]]),
  ) as Record<FindingSection, ScanFinding[]>;
  for (const finding of findings) {
    groups[findingSection(finding, staying.has(finding.key))].push(finding);
  }
  return groups;
}

/** Every catalog work a list of findings names, for one titles request. */
export function findingWorkCodes(findings: ScanFinding[]): string[] {
  const codes = new Set<string>();
  for (const finding of findings) {
    if (finding.workCode) codes.add(finding.workCode);
    if (finding.inLibrary?.workCode) codes.add(finding.inLibrary.workCode);
    for (const candidate of finding.candidates) codes.add(candidate.workCode);
  }
  return [...codes].sort();
}

export function findingConfidence(finding: ScanFinding): ScanMatchConfidence | null {
  return finding.candidates.find((candidate) => candidate.workCode === finding.workCode)?.confidence
    ?? null;
}

/** The finding's own name: its folder or file name, or the scanned folder's. */
export function findingName(finding: ScanFinding, rootPath: string): string {
  const path = finding.relativePath || rootPath;
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

const workCodePattern = /^[A-Z]{2}\d{5,10}$/i;

export function looksLikeWorkCode(text: string): boolean {
  return workCodePattern.test(text.trim());
}
