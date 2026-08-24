import { useQuery } from "@tanstack/react-query";
import { Copy, ExternalLink, FileArchive, LifeBuoy, LoaderCircle } from "lucide-react";
import { useState } from "react";

import { formatByteSize } from "../features/importer/CatalogImportPage";
import { usePresentation } from "../preferences/PresentationProvider";
import type { SupportGateway } from "./types";

type SupportFeedback = "idle" | "copied" | "saved" | "cancelled" | "failed";

export function SupportPage({ gateway }: { gateway: SupportGateway }) {
  const { locale, t } = usePresentation();
  const support = useQuery({
    queryKey: ["support", "status"],
    queryFn: () => gateway.readStatus(),
  });
  const [feedback, setFeedback] = useState<SupportFeedback>("idle");
  const [saving, setSaving] = useState(false);
  const status = support.data;

  const copy = async () => {
    if (!status) return;
    try {
      await navigator.clipboard.writeText(status.summary);
      setFeedback("copied");
    } catch {
      setFeedback("failed");
    }
  };
  const save = async () => {
    setSaving(true);
    setFeedback("idle");
    try {
      const result = await gateway.saveBundle();
      setFeedback(result.outcome === "saved" ? "saved" : "cancelled");
    } catch {
      setFeedback("failed");
    } finally {
      setSaving(false);
    }
  };
  const report = async () => {
    try {
      await gateway.openIssue();
    } catch {
      setFeedback("failed");
    }
  };

  return (
    <main className="support-page">
      <header className="support-page-masthead">
        <span>{t("nav.support")}</span>
        <h1>{t("support.pageTitle")}</h1>
        <p>{t("support.pageDescription")}</p>
      </header>

      <section className="support-report" aria-labelledby="support-report-title">
        <header className="support-report-head">
          <span className="support-report-mark"><LifeBuoy aria-hidden="true" /></span>
          <div>
            <h2 id="support-report-title">{t("support.title")}</h2>
            <p>{t("support.description")}</p>
          </div>
        </header>

        {status ? (
          <div className="support-report-facts">
            <span>
              <small>{t("support.estimatedSize")}</small>
              <b>{formatByteSize(status.estimatedBundleBytes, locale)}</b>
            </span>
            <span>
              <small>{t("support.logsRetained")}</small>
              <b>{status.retainedLogFiles.toLocaleString(locale)}</b>
            </span>
            <span>
              <small>{t("support.faultsRetained")}</small>
              <b>{status.retainedFaultFiles.toLocaleString(locale)}</b>
            </span>
          </div>
        ) : null}

        <p className="support-report-note">{t("support.privacyHelp")}</p>
        <p className="support-report-warning">{t("support.publicWarning")}</p>
        {status?.lastFault ? (
          <p className="support-report-note">
            {t("support.lastFault", {
              date: new Date(status.lastFault.occurredAt).toLocaleString(locale),
            })}
          </p>
        ) : status ? <p className="support-report-note">{t("support.noFault")}</p> : null}

        <div className="support-report-actions">
          <button type="button" disabled={!status} onClick={() => void copy()}>
            <Copy aria-hidden="true" />
            {t(feedback === "copied" ? "support.copied" : "support.copySummary")}
          </button>
          <button className="is-primary" type="button" disabled={saving} onClick={() => void save()}>
            <FileArchive aria-hidden="true" />
            {t(saving ? "support.saving" : "support.saveReport")}
          </button>
          <button type="button" onClick={() => void report()}>
            <ExternalLink aria-hidden="true" />
            {t("support.reportGitHub")}
          </button>
        </div>

        {support.isPending ? (
          <p className="support-report-note support-report-loading">
            <LoaderCircle aria-hidden="true" />
            {t("support.loading")}
          </p>
        ) : null}
        {support.error || feedback === "failed" ? (
          <p className="support-report-error" role="alert">{t("support.actionFailed")}</p>
        ) : null}
        {feedback === "saved" ? (
          <p className="support-report-success" role="status">{t("support.saved")}</p>
        ) : null}
        {feedback === "cancelled" ? (
          <p className="support-report-note" role="status">{t("support.cancelled")}</p>
        ) : null}
      </section>
    </main>
  );
}
