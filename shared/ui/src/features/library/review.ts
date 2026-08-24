import type {
  ContentItemOverride,
  Installation,
  InstallationReviewRequest,
  LaunchCandidate,
  ManualCatalogIdentity,
  ManualLaunchSelection,
  MediaType,
} from "./types";

export type IdentityMode = "detected" | "catalog_work" | "unidentified";

export interface ContentReviewValue {
  mediaType: MediaType | null;
  ignored: boolean;
  order: string;
}

export interface InstallationReviewDraft {
  identityMode: IdentityMode;
  identityWorkCode: string;
  customTitle: string;
  preferredSelectionKey: string;
  content: Record<string, ContentReviewValue>;
}

export function buildInstallationReviewDraft(installation: Installation): InstallationReviewDraft {
  const identity = installation.overrides.catalogIdentity;
  const identityMode: IdentityMode = identity?.kind ?? "detected";
  const content = Object.fromEntries(
    installation.overrides.contentItems.map((item) => [
      item.relativePath,
      {
        mediaType: item.mediaType,
        ignored: item.ignored,
        order: item.order === null ? "" : String(item.order),
      },
    ]),
  );
  return {
    identityMode,
    identityWorkCode: identity?.kind === "catalog_work" ? identity.workCode : "",
    customTitle: installation.overrides.customTitle ?? "",
    preferredSelectionKey: installation.overrides.preferredAction
      ? launchSelectionKey(installation.overrides.preferredAction)
      : "",
    content,
  };
}

export function installationReviewRequest(
  installation: Installation,
  draft: InstallationReviewDraft,
): InstallationReviewRequest {
  const selections = installationLaunchSelections(installation);
  const preferredAction = selections.find(
    (selection) => launchSelectionKey(selection) === draft.preferredSelectionKey,
  ) ?? null;
  const catalogIdentity: ManualCatalogIdentity | null = draft.identityMode === "catalog_work"
    ? { kind: "catalog_work", workCode: draft.identityWorkCode.trim() }
    : draft.identityMode === "unidentified"
      ? { kind: "unidentified" }
      : null;
  const contentItems = Object.entries(draft.content)
    .map(([relativePath, value]): ContentItemOverride | null => {
      const parsedOrder = value.order.trim() === "" ? null : Number(value.order);
      const order = parsedOrder !== null && Number.isInteger(parsedOrder) && parsedOrder >= 0
        ? parsedOrder
        : null;
      if (value.mediaType === null && !value.ignored && order === null) return null;
      return { relativePath, mediaType: value.mediaType, ignored: value.ignored, order };
    })
    .filter((item): item is ContentItemOverride => item !== null);
  return {
    installationId: installation.id,
    catalogIdentity,
    customTitle: draft.customTitle.trim() || null,
    preferredAction,
    contentItems,
  };
}

export function installationLaunchSelections(installation: Installation): ManualLaunchSelection[] {
  const candidates = installation.detection.launchCandidates.map(candidateSelection);
  const current = installation.overrides.preferredAction;
  if (current && !candidates.some((candidate) => launchSelectionKey(candidate) === launchSelectionKey(current))) {
    candidates.push(current);
  }
  return candidates;
}

export function launchSelectionKey(selection: ManualLaunchSelection): string {
  const target = selection.target.kind === "installation_root"
    ? "root"
    : `path:${selection.target.path}`;
  return `${selection.action}|${target}`;
}

function candidateSelection(candidate: LaunchCandidate): ManualLaunchSelection {
  return { action: candidate.action, target: candidate.target };
}
