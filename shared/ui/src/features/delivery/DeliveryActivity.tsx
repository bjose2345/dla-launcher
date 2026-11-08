import { useQuery } from "@tanstack/react-query";
import { CloudDownload } from "lucide-react";

import { Notice } from "../../app/Notice";
import { SidebarActivity } from "../../app/SidebarActivity";
import { useNewlySettled } from "../../app/useNewlySettled";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { CatalogDetailGateway } from "../catalog/types";
import { catalogKeys } from "../catalog/catalogQueryKeys";
import { jobProgressFraction, jobStatusText, problemText, remainingText } from "./deliveryText";
import { useDelivery } from "./useDelivery";
import { useTransferRate } from "./useTransferRate";
import { receivedBytes, totalBytes, type DeliveryGateway, type DeliveryJob, type DeliveryStatus } from "./types";

const SETTLED: readonly DeliveryStatus[] = ["installed", "needs_review", "failed"];

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
  const [finished, dismiss] = useNewlySettled(view?.jobs, (job: DeliveryJob) => job.id, (job) => job.status, SETTLED);
  const codes = [...new Set([running?.workCode, finished?.workCode].filter((code): code is string => !!code))];
  const titles = useQuery({
    queryKey: catalogKeys.works(codes),
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
        <SidebarActivity
          icon={CloudDownload}
          title={title(running.workCode)}
          label={t("delivery.activity.label", { title: title(running.workCode) })}
          percent={running.status === "installing" && progress?.phase !== "unpacking"
            ? null
            : jobProgressFraction(running, progress) * 100}
          onOpen={onOpenDownloads}
        >
          <span>{jobStatusText(t, locale, running, progress)}</span>
          {remaining && <span>{remaining}</span>}
          {waiting > 0 && <span>{t("delivery.activity.waiting", { count: waiting })}</span>}
        </SidebarActivity>
      )}
      {finished && (
        <Notice
          tone={finished.status === "failed" ? "problem" : "success"}
          title={finished.status === "installed"
            ? t("delivery.toast.installed", { title: title(finished.workCode) })
            : finished.status === "needs_review"
              ? t("delivery.toast.review", { title: title(finished.workCode) })
              : t("delivery.toast.failed", {
                title: title(finished.workCode),
                reason: problemText(t, finished.problem ?? "unavailable"),
              })}
          actions={finished.installationId && finished.status !== "failed"
            ? [{
              label: finished.status === "installed" ? t("delivery.openLibrary") : t("library.setUp"),
              primary: true,
              onClick: () => {
                onOpenInstallation(finished.installationId!);
                dismiss();
              },
            }]
            : [{
              label: t("delivery.openDownloads"),
              onClick: () => {
                onOpenDownloads();
                dismiss();
              },
            }]}
          onClose={dismiss}
        />
      )}
    </>
  );
}
