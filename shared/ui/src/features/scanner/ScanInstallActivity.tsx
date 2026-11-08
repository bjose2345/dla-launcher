import { useQuery } from "@tanstack/react-query";
import { PackageOpen } from "lucide-react";

import { basename } from "../../basename";
import { Notice } from "../../app/Notice";
import { SidebarActivity } from "../../app/SidebarActivity";
import { useNewlySettled } from "../../app/useNewlySettled";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { CatalogDetailGateway } from "../catalog/types";
import { catalogKeys } from "../catalog/catalogQueryKeys";
import { workTitle } from "../catalog/localizedNames";
import {
  packagePreparationPercent,
  usePackagePreparationProgress,
  type PackagePreparationProgressGateway,
} from "../library/usePackagePreparationProgress";
import type { ScanInstall, ScanInstallGateway } from "./types";
import { useScanInstalls } from "./useScanInstalls";

const SETTLED: readonly ScanInstall["status"][] = ["installed", "needs_review", "failed"];

/**
 * An archive the scanner is installing, in the sidebar while the rest of the
 * app stays usable, and a notice when it is done.
 */
export function ScanInstallActivity({
  gateway,
  progressGateway,
  catalogGateway,
  onOpenScanner,
  onShowInLibrary,
}: {
  gateway: ScanInstallGateway;
  progressGateway: PackagePreparationProgressGateway;
  catalogGateway: Pick<CatalogDetailGateway, "readWorks">;
  onOpenScanner: () => void;
  onShowInLibrary: (installationId: string) => void;
}) {
  const { locale, t } = usePresentation();
  const installs = useScanInstalls(gateway);
  const progress = usePackagePreparationProgress(progressGateway);
  const list = installs.data?.installs ?? [];
  const running = list.find((install) => install.status === "installing") ?? null;
  const waiting = list.filter((install) => install.status === "queued").length;
  const [finished, dismiss] = useNewlySettled(
    installs.data?.installs,
    (install: ScanInstall) => install.installationId,
    (install) => install.status,
    SETTLED,
  );

  const codes = [...new Set([running?.workCode, finished?.workCode].filter((code): code is string => !!code))];
  const works = useQuery({
    queryKey: catalogKeys.works(codes),
    queryFn: () => catalogGateway.readWorks(codes),
    enabled: codes.length > 0,
    staleTime: 5 * 60_000,
  });
  const title = (install: ScanInstall) => {
    const work = works.data?.find((item) => item.code === install.workCode);
    if (work) return workTitle(work, locale !== "ja-JP");
    const name = basename(install.name);
    return name.replace(/(\.part\d+)?\.(zip|rar|exe)$/i, "");
  };
  const current = running && progress.data?.installationId === running.installationId ? progress.data : null;
  const percent = current ? packagePreparationPercent(current) : null;

  return (
    <>
      {running ? (
        <SidebarActivity
          icon={PackageOpen}
          title={title(running)}
          label={t("scanner.activity.label", { title: title(running) })}
          percent={percent}
          onOpen={onOpenScanner}
        >
          <span>
            {percent === null
              ? t("scanner.status.installing")
              : t("scanner.status.installingPercent", { percent })}
          </span>
          {waiting > 0 ? <span>{t("scanner.activity.waiting", { count: waiting.toLocaleString(locale) })}</span> : null}
        </SidebarActivity>
      ) : null}
      {finished ? (
        <Notice
          tone={finished.status === "failed" ? "problem" : "success"}
          title={t(finished.status === "installed"
            ? "scanner.notice.installed"
            : finished.status === "needs_review"
              ? "scanner.notice.installedReview"
              : "scanner.notice.installFailed", { title: title(finished) })}
          detail={finished.status === "failed" && finished.detail
            ? t("common.technicalDetail", { detail: finished.detail })
            : undefined}
          actions={finished.status === "failed"
            ? [{ label: t("scanner.activity.openScanner"), onClick: () => { onOpenScanner(); dismiss(); } }]
            : [{
              label: t("scanner.notice.openLibrary"),
              primary: true,
              onClick: () => {
                onShowInLibrary(finished.installationId);
                dismiss();
              },
            }]}
          onClose={dismiss}
        />
      ) : null}
    </>
  );
}
