import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CloudDownload, FolderOpen, LogIn, LogOut } from "lucide-react";
import { useState } from "react";

import { displayPath } from "../../displayPath";
import { usePresentation } from "../../preferences";
import type { CatalogDetailGateway } from "../catalog/types";
import { DeliveryJobControls } from "./DeliveryJobControls";
import { DeliverySignIn } from "./DeliverySignIn";
import { problemText } from "./deliveryText";
import { acceptDeliveryView, useDelivery } from "./useDelivery";
import { deliveryFailure, type DeliveryFailure, type DeliveryGateway, type DeliveryView } from "./types";

/** Every Install from DLA job, with the account and install folder they use. */
export function DownloadsPage({
  gateway,
  catalogGateway,
  onOpenWork,
  onOpenInstallation,
  onOpenExternal,
}: {
  gateway: DeliveryGateway;
  catalogGateway: Pick<CatalogDetailGateway, "readWorks">;
  onOpenWork: (code: string) => void;
  onOpenInstallation: (installationId: string) => void;
  onOpenExternal?: (url: string) => Promise<void>;
}) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const delivery = useDelivery(gateway);
  const view = delivery.data;
  const [signingIn, setSigningIn] = useState(false);
  const [failure, setFailure] = useState<DeliveryFailure | null>(null);
  const jobs = [...(view?.jobs ?? [])].reverse();
  const codes = [...new Set(jobs.map((job) => job.workCode))];
  const titles = useQuery({
    queryKey: ["catalog", "works", codes],
    queryFn: () => catalogGateway.readWorks(codes),
    enabled: codes.length > 0,
  });
  const title = (code: string) => titles.data?.find((work) => work.code === code)?.title ?? code;

  const run = async (action: () => Promise<DeliveryView | null>) => {
    setFailure(null);
    try {
      const next = await action();
      if (next) acceptDeliveryView(queryClient, next);
    } catch (error) {
      setFailure(deliveryFailure(error));
    }
  };

  return (
    <main className="downloads-page">
      <section className="downloads-masthead">
        <div>
          <span className="downloads-eyebrow"><CloudDownload aria-hidden="true" />{t("delivery.eyebrow")}</span>
          <h1>{t("delivery.title")}</h1>
        </div>
        {view?.available && (
          <div className="downloads-settings">
            <div className="downloads-setting">
              <span>{view.account ? t("delivery.signedInAs", { account: view.account }) : t("delivery.signedOut")}</span>
              {view.account ? (
                <button className="delivery-button" type="button" onClick={() => void run(gateway.signOut)}>
                  <LogOut aria-hidden="true" />{t("delivery.signOut")}
                </button>
              ) : (
                <button className="delivery-button delivery-button-primary" type="button" onClick={() => setSigningIn(true)}>
                  <LogIn aria-hidden="true" />{t("delivery.signIn")}
                </button>
              )}
            </div>
            <div className="downloads-setting">
              <span title={view.installFolder ? displayPath(view.installFolder) : undefined}>
                {t("delivery.installFolder")}: <code>{view.installFolder ? displayPath(view.installFolder) : t("delivery.installFolderNone")}</code>
              </span>
              <button className="delivery-button" type="button" onClick={() => void run(gateway.chooseInstallFolder)}>
                <FolderOpen aria-hidden="true" />{view.installFolder ? t("delivery.changeFolder") : t("delivery.chooseFolder")}
              </button>
            </div>
          </div>
        )}
      </section>
      <div className="downloads-body">
        {failure && <p className="delivery-problem" role="alert">{problemText(t, failure.problem)}</p>}
        {view && !view.available && <p className="downloads-empty">{t("delivery.unavailableBuild")}</p>}
        {view?.available && jobs.length === 0 && <p className="downloads-empty">{t("delivery.empty")}</p>}
        <ul className="downloads-list">
          {jobs.map((job) => (
            <li key={job.id} className="downloads-item">
              <header>
                <button className="delivery-link" type="button" onClick={() => onOpenWork(job.workCode)}>
                  {title(job.workCode)}
                </button>
                <span className="downloads-code">
                  {job.workCode}{job.version ? ` · v${job.version}` : ""}{job.updateDate ? ` · ${job.updateDate}` : ""}
                </span>
              </header>
              <DeliveryJobControls gateway={gateway} job={job} progress={view?.progress ?? null} />
              {(job.status === "installed" || job.status === "needs_review") && job.installationId && (
                <button className="delivery-link" type="button" onClick={() => onOpenInstallation(job.installationId!)}>
                  {job.status === "installed" ? t("delivery.openLibrary") : t("library.setUp")}
                </button>
              )}
            </li>
          ))}
        </ul>
      </div>
      {signingIn && (
        <DeliverySignIn
          gateway={gateway}
          onSignedIn={(next) => {
            acceptDeliveryView(queryClient, next);
            setSigningIn(false);
          }}
          onClose={() => setSigningIn(false)}
          onOpenExternal={onOpenExternal}
        />
      )}
    </main>
  );
}
