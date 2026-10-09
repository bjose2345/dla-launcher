import { useQuery } from "@tanstack/react-query";
import { CloudDownload } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { Notice } from "../../app/Notice";
import { usePresentation } from "../../preferences";
import type { CatalogDetailGateway } from "../catalog/types";
import { jobProgressFraction, jobStatusText, problemText, remainingText } from "./deliveryText";
import { useDelivery } from "./useDelivery";
import { useTransferRate } from "./useTransferRate";
import { receivedBytes, totalBytes, type DeliveryGateway, type DeliveryJob } from "./types";

interface Toast {
  job: DeliveryJob;
}

/**
 * The running Install from DLA in the sidebar, while the rest of the app
 * stays usable, plus a notice when a job finishes or stops.
 */
export function DeliveryActivity({
  gateway,
  catalogGateway,
  onOpenDownloads,
  onOpenInstallation,
}: {
  gateway: DeliveryGateway;
  catalogGateway: Pick<CatalogDetailGateway, "readWorks">;
  onOpenDownloads: () => void;
  onOpenInstallation: (installationId: string) => void;
}) {
  const { t, locale } = usePresentation();
  const delivery = useDelivery(gateway);
  const view = delivery.data;
  const jobs = view?.jobs ?? [];
  const running = jobs.find((job) => job.status === "downloading" || job.status === "installing") ?? null;
  const waiting = jobs.filter((job) => job.status === "queued").length;
  const progress = view?.progress && running && view.progress.jobId === running.id ? view.progress : null;
  const rate = useTransferRate(progress);

  const [toast, setToast] = useState<Toast | null>(null);
  const statuses = useRef<Map<string, DeliveryJob["status"]> | null>(null);
  useEffect(() => {
    if (!view) return;
    const previous = statuses.current;
    statuses.current = new Map(view.jobs.map((job) => [job.id, job.status]));
    // Jobs present when the app opened are not announced; a job created since
    // is, even when it finished before the view first showed it.
    if (!previous) return;
    const finished = view.jobs.find((job) => (
      ["installed", "needs_review", "failed"].includes(job.status) && previous.get(job.id) !== job.status
    ));
    if (finished) setToast({ job: finished });
  }, [view]);
  const codes = [...new Set([running?.workCode, toast?.job.workCode].filter((code): code is string => !!code))];
  const titles = useQuery({
    queryKey: ["catalog", "works", codes],
    queryFn: () => catalogGateway.readWorks(codes),
    enabled: codes.length > 0,
  });
  const title = (code: string) => titles.data?.find((work) => work.code === code)?.title ?? code;

  if (!view?.available) return null;

  let remaining: string | null = null;
  if (running && progress?.phase === "receiving") {
    remaining = remainingText(t, totalBytes(running) - receivedBytes(running, progress), rate);
  } else if (running && progress?.phase === "unpacking") {
    remaining = remainingText(t, progress.totalBytes - progress.receivedBytes, rate);
  }

  return (
    <>
      {running && (
        <button
          className="delivery-activity"
          type="button"
          onClick={onOpenDownloads}
          aria-label={t("delivery.activity.label", { title: title(running.workCode) })}
        >
          <CloudDownload className="delivery-activity-icon" aria-hidden="true" />
          <span className="delivery-activity-text">
            <strong>{title(running.workCode)}</strong>
            <span>{jobStatusText(t, locale, running, progress)}</span>
            {remaining && <span>{remaining}</span>}
            {waiting > 0 && <span>{t("delivery.activity.waiting", { count: waiting })}</span>}
          </span>
          <progress
            className="delivery-activity-progress"
            max={1}
            value={running.status === "installing" && progress?.phase !== "unpacking"
              ? undefined
              : jobProgressFraction(running, progress)}
          />
        </button>
      )}
      {toast && (
        <Notice
          tone={toast.job.status === "failed" ? "problem" : "success"}
          title={toast.job.status === "installed"
            ? t("delivery.toast.installed", { title: title(toast.job.workCode) })
            : toast.job.status === "needs_review"
              ? t("delivery.toast.review", { title: title(toast.job.workCode) })
              : t("delivery.toast.failed", {
                title: title(toast.job.workCode),
                reason: problemText(t, toast.job.problem ?? "unavailable"),
              })}
          actions={toast.job.installationId && toast.job.status !== "failed"
            ? [{
              label: toast.job.status === "installed" ? t("delivery.openLibrary") : t("library.setUp"),
              primary: true,
              onClick: () => {
                onOpenInstallation(toast.job.installationId!);
                setToast(null);
              },
            }]
            : [{
              label: t("delivery.openDownloads"),
              onClick: () => {
                onOpenDownloads();
                setToast(null);
              },
            }]}
          onClose={() => setToast(null)}
        />
      )}
    </>
  );
}
