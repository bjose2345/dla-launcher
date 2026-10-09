import { useQuery } from "@tanstack/react-query";
import { PackageOpen } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { Notice } from "../../app/Notice";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { CatalogDetailGateway } from "../catalog/types";
import {
  packagePreparationPercent,
  usePackagePreparationProgress,
  type PackagePreparationProgressGateway,
} from "../library/usePackagePreparationProgress";
import { workTitle } from "./ScanFindingRow";
import type { ScanInstall, ScanInstallGateway } from "./types";
import { useScanInstalls } from "./useScanInstalls";

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

  const [finished, setFinished] = useState<ScanInstall | null>(null);
  const statuses = useRef<Map<string, ScanInstall["status"]> | null>(null);
  useEffect(() => {
    if (!installs.data) return;
    const previous = statuses.current;
    statuses.current = new Map(installs.data.installs.map((install) => [install.installationId, install.status]));
    if (!previous) return;
    const settled = installs.data.installs.find((install) => (
      ["installed", "needs_review", "failed"].includes(install.status)
        && previous.get(install.installationId) !== install.status
    ));
    if (settled) setFinished(settled);
  }, [installs.data]);

  const codes = [...new Set([running?.workCode, finished?.workCode].filter((code): code is string => !!code))];
  const works = useQuery({
    queryKey: ["catalog-works", codes],
    queryFn: () => catalogGateway.readWorks(codes),
    enabled: codes.length > 0,
    staleTime: 5 * 60_000,
  });
  const title = (install: ScanInstall) => {
    const work = works.data?.find((item) => item.code === install.workCode);
    if (work) return workTitle(work, locale !== "ja-JP");
    const name = install.name.split(/[\\/]/).filter(Boolean).at(-1) ?? install.name;
    return name.replace(/(\.part\d+)?\.(zip|rar|exe)$/i, "");
  };
  const current = running && progress.data?.installationId === running.installationId ? progress.data : null;
  const percent = current ? packagePreparationPercent(current) : null;

  return (
    <>
      {running ? (
        <button
          className="scan-activity"
          type="button"
          onClick={onOpenScanner}
          aria-label={t("scanner.activity.label", { title: title(running) })}
        >
          <PackageOpen className="scan-activity-icon" aria-hidden="true" />
          <span className="scan-activity-text">
            <strong>{title(running)}</strong>
            <span>
              {percent === null
                ? t("scanner.status.installing")
                : t("scanner.status.installingPercent", { percent })}
            </span>
            {waiting > 0 ? <span>{t("scanner.activity.waiting", { count: waiting.toLocaleString(locale) })}</span> : null}
          </span>
          <progress className="scan-activity-progress" max={100} value={percent ?? undefined} />
        </button>
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
            ? [{ label: t("scanner.activity.openScanner"), onClick: () => { onOpenScanner(); setFinished(null); } }]
            : [{
              label: t("scanner.notice.openLibrary"),
              primary: true,
              onClick: () => {
                onShowInLibrary(finished.installationId);
                setFinished(null);
              },
            }]}
          onClose={() => setFinished(null)}
        />
      ) : null}
    </>
  );
}
