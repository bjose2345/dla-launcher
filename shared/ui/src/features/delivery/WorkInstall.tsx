import { CloudDownload, LoaderCircle } from "lucide-react";

import { usePresentation } from "../../preferences";
import { DeliveryJobControls } from "./DeliveryJobControls";
import { DeliverySignIn } from "./DeliverySignIn";
import { problemText } from "./deliveryText";
import { useDelivery } from "./useDelivery";
import { useStartInstall } from "./useStartInstall";
import { latestJob, type DeliveryGateway } from "./types";

/**
 * Install from DLA on a work's page: offered when the work has no local
 * copy, it then shows the download's progress, and once installed points to
 * the Library.
 */
export function WorkInstall({
  workCode,
  gateway,
  hasLocalCopy,
  onOpenInstallation,
  onOpenExternal,
}: {
  workCode: string;
  gateway: DeliveryGateway;
  hasLocalCopy: boolean;
  onOpenInstallation: (installationId: string) => void;
  onOpenExternal?: (url: string) => Promise<void>;
}) {
  const { t } = usePresentation();
  const delivery = useDelivery(gateway);
  const view = delivery.data;
  const install = useStartInstall(gateway, view);
  if (!view?.available) return null;
  const job = latestJob(view, workCode);
  const showJob = job !== null && job.status !== "cancelled";

  if (showJob && job.status === "installed") {
    return (
      <div className="delivery-installed" role="status">
        <p>{t("delivery.status.installed")}</p>
        {job.installationId && (
          <button className="delivery-link" type="button" onClick={() => onOpenInstallation(job.installationId!)}>
            {t("delivery.openLibrary")}
          </button>
        )}
      </div>
    );
  }
  if (showJob) {
    return (
      <div className="delivery-work-panel">
        <DeliveryJobControls gateway={gateway} job={job} progress={view.progress} />
        {job.status === "needs_review" && job.installationId && (
          <button className="delivery-link" type="button" onClick={() => onOpenInstallation(job.installationId!)}>
            {t("library.setUp")}
          </button>
        )}
      </div>
    );
  }
  if (hasLocalCopy) return null;
  return (
    <div className="delivery-work-panel">
      <button
        className="work-local-button work-local-button-delivery"
        type="button"
        disabled={install.pending}
        aria-busy={install.pending}
        onClick={() => install.start(workCode)}
      >
        <span className="work-local-button-delivery-label">
          {install.pending ? <LoaderCircle className="delivery-spin" aria-hidden="true" /> : <CloudDownload aria-hidden="true" />}
          {t("delivery.install")}
          <span aria-hidden="true">→</span>
        </span>
        <small>{t("delivery.installRequirement")}</small>
      </button>
      <p className="delivery-work-hint">{t("delivery.installHint")}</p>
      {install.failure && <p className="delivery-problem" role="alert">{problemText(t, install.failure.problem)}</p>}
      {install.signingIn && (
        <DeliverySignIn
          gateway={gateway}
          onSignedIn={install.signedIn}
          onClose={install.cancelSignIn}
          onOpenExternal={onOpenExternal}
        />
      )}
    </div>
  );
}
