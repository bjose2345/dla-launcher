import { formatByteSize } from "../../formatByteSize";
import type { MessageKey } from "../../i18n/catalogs";
import type { DeliveryJob, DeliveryProblem, DeliveryProgress } from "./types";
import { receivedBytes, totalBytes } from "./types";

type Translate = (key: MessageKey, values?: Record<string, string | number>) => string;

export function problemText(t: Translate, problem: DeliveryProblem): string {
  return t(`delivery.problem.${problem}` as MessageKey);
}

/** The DLA website's patron plans in the launcher's interface language. */
export function patronPlansUrl(locale: string): string {
  return `https://dlsitearchive.com/${locale}/patron/plans`;
}

/** One line describing where a job stands. */
export function jobStatusText(
  t: Translate,
  locale: string,
  job: DeliveryJob,
  progress: DeliveryProgress | null,
): string {
  const total = totalBytes(job);
  const received = Math.min(receivedBytes(job, progress), total);
  const sizes = {
    received: formatByteSize(received, locale),
    total: formatByteSize(total, locale),
    percent: total > 0 ? Math.floor((received / total) * 100) : 0,
  };
  const live = progress?.jobId === job.id ? progress.phase : null;
  switch (job.status) {
    case "downloading":
      if (live === "locating") return t("delivery.phase.locating");
      if (live === "checking") return t("delivery.phase.checking", sizes);
      if (live === "verifying") return t("delivery.phase.verifying");
      return t("delivery.status.downloading", sizes);
    case "installing":
      if (live === "unpacking" && progress && progress.totalBytes > 0) {
        return t("delivery.phase.unpacking", {
          percent: Math.floor((Math.min(progress.receivedBytes, progress.totalBytes) / progress.totalBytes) * 100),
        });
      }
      return t("delivery.status.installing");
    case "failed":
      return t("delivery.status.failed", { reason: problemText(t, job.problem ?? "unavailable") });
    case "paused":
      return t("delivery.status.paused", sizes);
    default:
      return t(`delivery.status.${job.status}` as MessageKey, sizes);
  }
}

export function jobProgressFraction(job: DeliveryJob, progress: DeliveryProgress | null): number {
  const total = totalBytes(job);
  if (job.status === "installing" && progress?.jobId === job.id && progress.phase === "unpacking" && progress.totalBytes > 0) {
    return Math.min(progress.receivedBytes / progress.totalBytes, 1);
  }
  if (job.status === "installing" || job.status === "installed" || job.status === "needs_review") return 1;
  return total > 0 ? Math.min(receivedBytes(job, progress) / total, 1) : 0;
}

/** Remaining time, phrased for people, from a measured rate in bytes per second. */
export function remainingText(t: Translate, remainingBytes: number, bytesPerSecond: number): string | null {
  if (!(bytesPerSecond > 0) || remainingBytes <= 0) return null;
  const minutes = Math.ceil(remainingBytes / bytesPerSecond / 60);
  if (minutes <= 1) return t("delivery.eta.soon");
  if (minutes < 60) return t("delivery.eta.minutes", { minutes });
  return t("delivery.eta.hours", { hours: Math.floor(minutes / 60), minutes: minutes % 60 });
}
