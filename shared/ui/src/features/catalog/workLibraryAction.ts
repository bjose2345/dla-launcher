import type {
  Installation,
  LaunchActionKind,
  LibraryGateway,
  PreparedPackageInstallation,
} from "../library/types";

export interface WorkInstallationSnapshot {
  installation: Installation;
  prepared: PreparedPackageInstallation | null;
}

export type WorkLibraryAction =
  | { kind: "scan" }
  | { kind: "install"; installationId: string }
  | { kind: "review"; installationId: string }
  | { kind: "play"; installationId: string; action: LaunchActionKind }
  | { kind: "installed"; installationId: string };

export async function readWorkLibraryAction(
  gateway: LibraryGateway,
  workCode: string,
): Promise<WorkLibraryAction> {
  const installations = await gateway.listInstallationsForWork(workCode);
  const snapshots = await Promise.all(
    installations.map(async (installation) => ({
      installation,
      prepared: await gateway.readPreparedPackage(installation.id),
    })),
  );
  return resolveWorkLibraryAction(snapshots);
}

export function resolveWorkLibraryAction(
  snapshots: WorkInstallationSnapshot[],
): WorkLibraryAction {
  const prepared = snapshots.find((snapshot) => snapshot.prepared !== null);
  if (prepared) {
    if (!installationWasReviewed(prepared)) {
      return { kind: "review", installationId: prepared.installation.id };
    }
    const action = installationAction(prepared);
    return action
      ? { kind: "play", installationId: prepared.installation.id, action }
      : { kind: "installed", installationId: prepared.installation.id };
  }

  const direct = snapshots.find((snapshot) => (
    snapshot.installation.detection.packageInspection === null
      && snapshot.installation.status === "ready"
      && snapshot.installation.overrides.reviewedAt !== null
      && installationHasAction(snapshot)
  ));
  if (direct) {
    return {
      kind: "play",
      installationId: direct.installation.id,
      action: installationAction(direct)!,
    };
  }

  const packageInstallation = snapshots.find((snapshot) => (
    snapshot.installation.detection.packageInspection?.safety === "safe"
  ));
  if (packageInstallation) {
    return { kind: "install", installationId: packageInstallation.installation.id };
  }

  const review = snapshots[0];
  return review
    ? { kind: "review", installationId: review.installation.id }
    : { kind: "scan" };
}

function installationHasAction(snapshot: WorkInstallationSnapshot): boolean {
  return installationAction(snapshot) !== null;
}

function installationAction(snapshot: WorkInstallationSnapshot): LaunchActionKind | null {
  if (snapshot.prepared) {
    return snapshot.prepared.preferredAction?.action ?? null;
  }
  return snapshot.installation.overrides.preferredAction?.action ?? null;
}

function installationWasReviewed(snapshot: WorkInstallationSnapshot): boolean {
  return snapshot.installation.status === "ready"
    && snapshot.installation.overrides.reviewedAt !== null;
}
