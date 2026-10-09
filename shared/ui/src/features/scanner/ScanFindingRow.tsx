import {
  AlertTriangle,
  Archive,
  Check,
  CheckCircle2,
  File,
  Folder,
  LoaderCircle,
} from "lucide-react";
import { useState } from "react";

import { ActionMenu } from "../../app/ActionMenu";
import { displayPath } from "../../displayPath";
import { formatByteSize } from "../../formatByteSize";
import type { MessageKey } from "../../i18n/catalogs";
import { usePresentation } from "../../preferences/PresentationProvider";
import { CatalogWorkPicker } from "../catalog/CatalogWorkPicker";
import type { CatalogDetailGateway, CatalogWork } from "../catalog/types";
import { WorkCardImage } from "../catalog/WorkCardImage";
import type { ManageTab } from "../library/LibraryManageDrawer";
import type { PackagePreparationProgress } from "../library/types";
import { packagePreparationPercent } from "../library/usePackagePreparationProgress";
import type { SearchGateway } from "../search/types";
import { findingConfidence, findingName } from "./scanFindings";
import type {
  AddScanFinding,
  ScanFinding,
  ScanFindingRefusal,
  ScanInstall,
} from "./types";

export type RowOutcome =
  | { kind: "added"; installationId: string; workCode: string | null }
  | { kind: "installing"; installationId: string; workCode: string | null }
  | { kind: "refused"; refusal: ScanFindingRefusal };

type RowStatus =
  | "open"
  | "added"
  | "queued"
  | "installing"
  | "installed"
  | "needs_review"
  | "failed"
  | "cancelled"
  | "refused"
  | "in_library"
  | "ignored";

export function ScanFindingRow({
  finding,
  rootPath,
  works,
  outcome,
  install,
  progress,
  busy,
  catalogGateway,
  searchGateway,
  onAdd,
  onIgnore,
  onUndo,
  onManage,
  onShowInLibrary,
}: {
  finding: ScanFinding;
  rootPath: string;
  works: ReadonlyMap<string, CatalogWork>;
  outcome: RowOutcome | undefined;
  install: ScanInstall | undefined;
  progress: PackagePreparationProgress | null;
  busy: boolean;
  catalogGateway: Pick<CatalogDetailGateway, "readWorks">;
  searchGateway?: Pick<SearchGateway, "search">;
  onAdd: (item: AddScanFinding) => void;
  onIgnore: (ignored: boolean) => void;
  onUndo: () => void;
  onManage: (installationId: string, tab: ManageTab) => void;
  onShowInLibrary: (installationId: string) => void;
}) {
  const { locale, t } = usePresentation();
  const preferEnglish = locale !== "ja-JP";
  const [picking, setPicking] = useState(false);
  const [picked, setPicked] = useState<CatalogWork | null>(null);

  const status = rowStatus(finding, outcome, install);
  const workCode = (outcome && outcome.kind !== "refused" ? outcome.workCode : null)
    ?? (outcome ? null : finding.inLibrary?.workCode)
    ?? finding.workCode;
  const work = workCode ? works.get(workCode) : undefined;
  const name = findingName(finding, rootPath);
  const title = work ? workTitle(work, preferEnglish) : name;
  const circle = work?.circles[0];
  const confidence = findingConfidence(finding);
  const installationId = outcome && outcome.kind !== "refused"
    ? outcome.installationId
    : finding.inLibrary?.installationId;
  const verb = t(finding.kind === "archive" ? "scanner.install" : "scanner.add");
  const choosing = status === "open"
    && (finding.state === "choose" || finding.state === "unknown" || picking);

  return (
    <article className={`scan-finding is-${finding.state} is-${status.replace("_", "-")}`}>
      <span className="scan-finding-art" aria-hidden="true">
        {work ? (
          <WorkCardImage
            code={work.code}
            title={title}
            mainImageUrls={work.mainImageUrls}
            thumbnailUrls={work.thumbnailUrls}
          />
        ) : (
          <span className="scan-finding-glyph">
            {finding.kind === "archive" ? <Archive /> : finding.kind === "file" ? <File /> : <Folder />}
          </span>
        )}
      </span>

      <div className="scan-finding-copy">
        <div className="scan-finding-title">
          <strong>{title}</strong>
          {circle ? <span>{preferEnglish && circle.nameEnglish.trim() ? circle.nameEnglish : circle.name}</span> : null}
          {finding.state === "unknown" && !work ? <em>{t("scanner.unknownWork")}</em> : null}
        </div>
        <div className="scan-finding-facts">
          <span className="scan-finding-chip">{kindLabel(finding, locale, t)}</span>
          {finding.state === "ready" && confidence ? (
            <span className="scan-finding-chip is-match">
              {t(confidence === "exact" ? "scanner.match.exact" : "scanner.match.strong")}
            </span>
          ) : null}
          {workCode ? <code>{workCode}</code> : null}
        </div>
        <code className="scan-finding-path" title={finding.relativePath || displayPath(rootPath)}>
          {finding.relativePath || displayPath(rootPath)}
        </code>
      </div>

      <div className="scan-finding-actions">
        {status === "open" && finding.state === "ready" ? (
          <>
            <button
              className="scan-button is-primary"
              type="button"
              disabled={busy}
              onClick={() => onAdd({ key: finding.key })}
            >
              {verb}
            </button>
            <ActionMenu
              label={t("scanner.more", { name: title })}
              items={[
                { label: t("scanner.pickAnother"), onSelect: () => setPicking(true) },
                { label: t("scanner.ignore"), onSelect: () => onIgnore(true) },
              ]}
            />
          </>
        ) : null}

        {status === "open" && finding.state !== "ready" ? (
          <button className="scan-text-button" type="button" disabled={busy} onClick={() => onIgnore(true)}>
            {t("scanner.ignore")}
          </button>
        ) : null}

        {status === "added" || status === "installed" || status === "in_library" ? (
          <span className="scan-finding-status is-done">
            <CheckCircle2 aria-hidden="true" />
            {t(status === "installed"
              ? "scanner.status.installed"
              : status === "in_library" && !finding.inLibrary?.sameFiles
                ? "scanner.status.anotherCopy"
                : "scanner.status.added")}
          </span>
        ) : null}

        {status === "needs_review" ? (
          <span className="scan-finding-status is-attention">
            <AlertTriangle aria-hidden="true" />
            {t("scanner.status.needsReview")}
          </span>
        ) : null}

        {status === "queued" || status === "installing" ? (
          <InstallProgress status={status} progress={progress} />
        ) : null}

        {status === "failed" || status === "refused" || status === "cancelled" ? (
          <span className={`scan-finding-status ${status === "cancelled" ? "" : "is-problem"}`.trim()}>
            {status === "cancelled" ? null : <AlertTriangle aria-hidden="true" />}
            {status === "failed"
              ? t("scanner.status.failed")
              : status === "cancelled"
                ? t("scanner.status.cancelled")
                : t(refusalKey(outcome?.kind === "refused" ? outcome.refusal : "failed"))}
          </span>
        ) : null}

        {status === "ignored" ? (
          <>
            <span className="scan-finding-status">{t("scanner.status.ignored")}</span>
            <button className="scan-text-button" type="button" disabled={busy} onClick={() => onIgnore(false)}>
              {t("scanner.showAgain")}
            </button>
          </>
        ) : null}

        {["added", "installed", "needs_review", "queued", "installing"].includes(status) && outcome ? (
          <button className="scan-text-button" type="button" disabled={busy} onClick={onUndo}>
            {t(status === "queued" || status === "installing" ? "scanner.cancelInstall" : "scanner.undo")}
          </button>
        ) : null}

        {installationId && ["added", "installed", "needs_review", "in_library"].includes(status) ? (
          <ActionMenu
            label={t("scanner.more", { name: title })}
            items={[
              { label: t("workMenu.edit"), onSelect: () => onManage(installationId, "general") },
              { label: t("workMenu.show"), onSelect: () => onShowInLibrary(installationId) },
              { label: t("workMenu.remove"), onSelect: () => onManage(installationId, "files"), danger: true },
            ]}
          />
        ) : null}

        {status === "failed" || status === "refused" || status === "cancelled" ? (
          <button className="scan-text-button" type="button" disabled={busy} onClick={() => onAdd({ key: finding.key })}>
            {t("scanner.tryAgain")}
          </button>
        ) : null}
      </div>

      {status === "failed" && install?.detail ? (
        <p className="scan-finding-note is-problem">{t("common.technicalDetail", { detail: install.detail })}</p>
      ) : null}

      {status === "open" && (finding.state === "mixed" || finding.state === "incomplete") ? (
        <p className="scan-finding-note">
          {t(finding.state === "mixed" ? "scanner.mixedHelp" : "scanner.incompleteHelp")}
        </p>
      ) : null}

      {choosing ? (
        <div className="scan-finding-choose">
          <p>{t(finding.state === "choose" ? "scanner.chooseHelp" : finding.state === "unknown" ? "scanner.unknownHelp" : "scanner.pickAnotherHelp")}</p>
          {finding.state === "choose" ? (
            <div className="scan-finding-candidates" role="radiogroup" aria-label={t("scanner.chooseHelp")}>
              {finding.candidates.map((candidate) => {
                const candidateWork = works.get(candidate.workCode);
                const selected = picked?.code === candidate.workCode;
                const candidateTitle = candidateWork ? workTitle(candidateWork, preferEnglish) : candidate.workCode;
                return (
                  <button
                    className={`scan-candidate${selected ? " is-selected" : ""}`}
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    disabled={!candidateWork}
                    key={candidate.workCode}
                    onClick={() => candidateWork && setPicked(candidateWork)}
                  >
                    <span className="scan-candidate-cover" aria-hidden="true">
                      {candidateWork ? (
                        <WorkCardImage
                          code={candidateWork.code}
                          title={candidateTitle}
                          mainImageUrls={candidateWork.mainImageUrls}
                          thumbnailUrls={candidateWork.thumbnailUrls}
                        />
                      ) : null}
                    </span>
                    <span className="scan-candidate-copy">
                      <strong>{candidateTitle}</strong>
                      <small>{candidateCircle(candidateWork, preferEnglish)}{candidate.workCode}</small>
                    </span>
                    {selected ? <Check className="scan-candidate-check" aria-hidden="true" /> : null}
                  </button>
                );
              })}
            </div>
          ) : null}
          {picked && !finding.candidates.some((candidate) => candidate.workCode === picked.code) ? (
            <div className="scan-finding-candidates">
              <span className="scan-candidate is-selected">
                <span className="scan-candidate-copy">
                  <strong>{workTitle(picked, preferEnglish)}</strong>
                  <small>{candidateCircle(picked, preferEnglish)}{picked.code}</small>
                </span>
                <Check className="scan-candidate-check" aria-hidden="true" />
              </span>
            </div>
          ) : null}
          <details className="scan-finding-search" open={finding.state !== "choose"}>
            <summary>{t("scanner.searchCatalog")}</summary>
            <CatalogWorkPicker
              catalogGateway={catalogGateway}
              searchGateway={searchGateway}
              onPick={setPicked}
            />
          </details>
          <div className="scan-finding-choose-actions">
            <button
              className="scan-button is-primary"
              type="button"
              disabled={busy || !picked}
              onClick={() => picked && onAdd({ key: finding.key, workCode: picked.code })}
            >
              {verb}
            </button>
            {finding.state === "ready" ? (
              <button
                className="scan-text-button"
                type="button"
                onClick={() => {
                  setPicking(false);
                  setPicked(null);
                }}
              >
                {t("common.cancel")}
              </button>
            ) : (
              <button
                className="scan-button"
                type="button"
                disabled={busy}
                onClick={() => onAdd({ key: finding.key, unknown: true })}
              >
                {t("scanner.addAsUnknown")}
              </button>
            )}
          </div>
        </div>
      ) : null}
    </article>
  );
}

function InstallProgress({
  status,
  progress,
}: {
  status: "queued" | "installing";
  progress: PackagePreparationProgress | null;
}) {
  const { t } = usePresentation();
  const percent = status === "installing" && progress ? packagePreparationPercent(progress) : null;
  return (
    <span className="scan-finding-progress">
      <span>
        <LoaderCircle className="scan-spin" aria-hidden="true" />
        {status === "queued"
          ? t("scanner.status.queued")
          : percent === null
            ? t("scanner.status.installing")
            : t("scanner.status.installingPercent", { percent })}
      </span>
      <progress max={100} value={percent ?? undefined} aria-label={t("scanner.status.installing")} />
    </span>
  );
}

function rowStatus(
  finding: ScanFinding,
  outcome: RowOutcome | undefined,
  install: ScanInstall | undefined,
): RowStatus {
  if (outcome?.kind === "added") return "added";
  if (outcome?.kind === "refused") return "refused";
  if (outcome?.kind === "installing") return install?.status ?? "queued";
  if (finding.inLibrary) return "in_library";
  if (finding.ignored) return "ignored";
  return "open";
}

function kindLabel(
  finding: ScanFinding,
  locale: string,
  t: ReturnType<typeof usePresentation>["t"],
): string {
  const size = formatByteSize(finding.sizeBytes, locale);
  if (finding.kind === "archive") {
    return finding.volumeCount > 1
      ? t("scanner.kind.archiveParts", { count: finding.volumeCount.toLocaleString(locale), size })
      : t("scanner.kind.archive", { size });
  }
  if (finding.kind === "file") return t("scanner.kind.file", { size });
  return finding.fileCount === 1
    ? t("scanner.kind.folderOne", { size })
    : t("scanner.kind.folder", { count: finding.fileCount.toLocaleString(locale), size });
}

function refusalKey(refusal: ScanFindingRefusal): MessageKey {
  switch (refusal) {
    case "choose_work": return "scanner.refused.chooseWork";
    case "not_found": return "scanner.refused.notFound";
    case "mixed": return "scanner.mixedHelp";
    case "incomplete": return "scanner.incompleteHelp";
    case "failed": return "scanner.refused.failed";
  }
}

export function workTitle(work: CatalogWork, preferEnglish: boolean): string {
  return preferEnglish && work.titleEnglish.trim() ? work.titleEnglish : work.title;
}

function candidateCircle(work: CatalogWork | undefined, preferEnglish: boolean): string {
  const circle = work?.circles[0];
  if (!circle) return "";
  return `${preferEnglish && circle.nameEnglish.trim() ? circle.nameEnglish : circle.name} · `;
}
