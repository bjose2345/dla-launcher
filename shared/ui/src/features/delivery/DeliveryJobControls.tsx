import { Pause, Play, RotateCcw, Trash2, X } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { usePresentation } from "../../preferences";
import { jobProgressFraction, jobStatusText, problemText } from "./deliveryText";
import { acceptDeliveryView } from "./useDelivery";
import {
  deliveryFailure,
  type DeliveryFailure,
  type DeliveryGateway,
  type DeliveryJob,
  type DeliveryProgress,
  type DeliveryView,
} from "./types";

/** A job's status line, progress bar and the actions its state allows. */
export function DeliveryJobControls({
  gateway,
  job,
  progress,
}: {
  gateway: DeliveryGateway;
  job: DeliveryJob;
  progress: DeliveryProgress | null;
}) {
  const { t, locale } = usePresentation();
  const queryClient = useQueryClient();
  const [failure, setFailure] = useState<DeliveryFailure | null>(null);
  const [pending, setPending] = useState(false);
  const run = async (action: (jobId: string) => Promise<DeliveryView>) => {
    setPending(true);
    setFailure(null);
    try {
      acceptDeliveryView(queryClient, await action(job.id));
    } catch (error) {
      setFailure(deliveryFailure(error));
    } finally {
      setPending(false);
    }
  };
  const fraction = jobProgressFraction(job, progress);
  const running = job.status === "queued" || job.status === "downloading";
  const stopped = job.status === "paused" || job.status === "failed";
  const finished = job.status === "installed" || job.status === "needs_review" || job.status === "cancelled";
  return (
    <div className={`delivery-job delivery-job-${job.status}`}>
      <p className="delivery-job-status" role="status">{jobStatusText(t, locale, job, progress)}</p>
      {(running || job.status === "installing" || job.status === "paused") && (
        <progress
          className="delivery-job-progress"
          max={1}
          value={job.status === "installing" ? undefined : fraction}
          aria-label={t("delivery.progressLabel", { code: job.workCode })}
        />
      )}
      {job.status === "failed" && job.detail && (job.problem === "install" || job.problem === "unavailable") && (
        <p className="delivery-job-detail">{t("common.technicalDetail", { detail: job.detail })}</p>
      )}
      {job.status === "needs_review" && job.detail && (
        <p className="delivery-job-detail">{t("common.technicalDetail", { detail: job.detail })}</p>
      )}
      <div className="delivery-job-actions">
        {running && (
          <button className="delivery-button" type="button" disabled={pending} onClick={() => void run(gateway.pause)}>
            <Pause aria-hidden="true" />{t("delivery.pause")}
          </button>
        )}
        {stopped && (
          <button className="delivery-button" type="button" disabled={pending} onClick={() => void run(gateway.resume)}>
            {job.status === "failed" ? <RotateCcw aria-hidden="true" /> : <Play aria-hidden="true" />}
            {job.status === "failed" ? t("delivery.retry") : t("delivery.resume")}
          </button>
        )}
        {(running || stopped) && (
          <button className="delivery-button delivery-button-danger" type="button" disabled={pending} onClick={() => void run(gateway.cancel)}>
            <X aria-hidden="true" />{t("delivery.cancel")}
          </button>
        )}
        {finished && (
          <button className="delivery-button" type="button" disabled={pending} onClick={() => void run(gateway.remove)}>
            <Trash2 aria-hidden="true" />{t("delivery.remove")}
          </button>
        )}
      </div>
      {failure && <p className="delivery-problem" role="alert">{problemText(t, failure.problem)}</p>}
    </div>
  );
}
