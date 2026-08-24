import { useQuery } from "@tanstack/react-query";

import {
  diagnosticCheckMessageKey,
  diagnosticDetailMessageKey,
  platformMessageKey,
} from "../../i18n/domainLabels";
import { summarizeChecks } from "./report";
import type { DiagnosticsGateway } from "./types";
import { usePresentation } from "../../preferences/PresentationProvider";

interface DiagnosticsPageProps {
  gateway: DiagnosticsGateway;
  bridgeDescription: string;
  platformNote: string;
}

export function DiagnosticsPage({
  gateway,
  bridgeDescription,
  platformNote,
}: DiagnosticsPageProps) {
  const { locale, t } = usePresentation();
  const probe = useQuery({
    queryKey: ["diagnostics", "sqlite-capability"],
    queryFn: () => gateway.runSQLiteProbe(),
  });
  const report = probe.data;
  const summary = summarizeChecks(report?.checks ?? []);

  return (
    <main className="page-shell diagnostics-shell">
      <section className="hero">
        <div>
          <p className="eyebrow">{t("diagnostics.foundationGate")}</p>
          <h2>{t("diagnostics.sqliteViability")}</h2>
          <p className="lede">
            {t("diagnostics.foundationDescription", { bridge: bridgeDescription })}
          </p>
        </div>
        <button
          className="run-button"
          type="button"
          disabled={probe.isFetching}
          onClick={() => void probe.refetch()}
        >
          {t(probe.isFetching ? "diagnostics.running" : "diagnostics.runAgain")}
        </button>
      </section>

      {probe.isPending && <p className="notice">{t("diagnostics.runningFirst")}</p>}

      {probe.isError && (
        <section className="notice error" role="alert">
          <strong>{t("diagnostics.bindingFailed")}</strong>
          <span>{t("common.technicalDetail", { detail: errorMessage(probe.error) })}</span>
        </section>
      )}

      {report && (
        <>
          <section className={`gate-summary ${report.passed ? "passed" : "failed"}`}>
            <div>
              <p className="eyebrow">{t("diagnostics.gateResult")}</p>
              <h2>{t(report.passed ? "diagnostics.passed" : "diagnostics.needsInvestigation")}</h2>
            </div>
            <div className="score">
              <strong>{summary.passed}</strong>
              <span>{t("diagnostics.checksPassed", { total: summary.total })}</span>
            </div>
          </section>

          <section className="metadata" aria-label={t("diagnostics.environment")}>
            <Metadata label={t("diagnostics.platform")} value={t(platformMessageKey(report.platform))} />
            <Metadata label="SQLite" value={report.sqliteVersion || t("diagnostics.unavailable")} />
            <Metadata label={t("diagnostics.journal")} value={report.journalMode || t("diagnostics.unavailable")} />
            <Metadata label={t("diagnostics.completed")} value={formatTimestamp(report.completedAt, locale)} />
            <Metadata className="database-path" label={t("diagnostics.probeDatabase")} value={report.databasePath} />
          </section>

          <section className="checks" aria-labelledby="checks-title">
            <div className="section-heading">
              <div>
                <p className="eyebrow">{t("diagnostics.nativePath")}</p>
                <h2 id="checks-title">{t("diagnostics.capabilityChecks")}</h2>
              </div>
              {summary.failed > 0 && <span className="failure-count">{t("diagnostics.failed", { count: summary.failed })}</span>}
            </div>

            <ul>
              {report.checks.map((check) => {
                const detailKey = diagnosticDetailMessageKey(check.detail);
                return (
                <li key={check.key}>
                  <span className={`check-icon ${check.passed ? "passed" : "failed"}`}>
                    {check.passed ? "✓" : "×"}
                  </span>
                  <span className="check-copy">
                    <strong>{t(diagnosticCheckMessageKey(check.key))}</strong>
                    <small>{detailKey
                      ? t(detailKey)
                      : t("common.technicalDetail", { detail: check.detail })}</small>
                  </span>
                </li>
                );
              })}
            </ul>
          </section>
        </>
      )}

      <footer>
        {platformNote}
      </footer>
    </main>
  );
}

interface MetadataProps {
  className?: string;
  label: string;
  value: string;
}

function Metadata({ className = "", label, value }: MetadataProps) {
  return (
    <div className={className}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function formatTimestamp(timestamp: string, locale: string): string {
  const date = new Date(timestamp);
  return Number.isNaN(date.valueOf()) ? timestamp : date.toLocaleString(locale);
}
