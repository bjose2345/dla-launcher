import { Copy, ExternalLink, FileArchive, ShieldAlert, X } from "lucide-react";
import { useEffect, useState } from "react";

import { usePresentation } from "../preferences/PresentationProvider";
import type { SupportGateway, SupportStatus } from "./types";

export function SupportRecoveryNotice({
  gateway,
  onOpenSupport,
}: {
  gateway: SupportGateway;
  onOpenSupport: () => void;
}) {
  const { t } = usePresentation();
  const [status, setStatus] = useState<SupportStatus | null>(null);
  const [action, setAction] = useState<"idle" | "copied" | "saving" | "saved" | "failed">("idle");

  useEffect(() => {
    let active = true;
    void gateway.readStatus().then((value) => {
      if (active) setStatus(value);
    }).catch(() => undefined);
    return () => {
      active = false;
    };
  }, [gateway]);

  if (!status?.previousShutdownUnclean) return null;

  const dismiss = async () => {
    try {
      await gateway.acknowledgeUncleanShutdown();
      setStatus(null);
    } catch {
      setAction("failed");
    }
  };
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(status.summary);
      setAction("copied");
    } catch {
      setAction("failed");
    }
  };
  const save = async () => {
    setAction("saving");
    try {
      const result = await gateway.saveBundle();
      setAction(result.outcome === "saved" ? "saved" : "idle");
    } catch {
      setAction("failed");
    }
  };

  return (
    <aside className="support-recovery" aria-labelledby="support-recovery-title">
      <ShieldAlert aria-hidden="true" />
      <div className="support-recovery-copy">
        <h2 id="support-recovery-title">{t("support.recoveryTitle")}</h2>
        <p>{t("support.recoveryHelp")}</p>
        <small>{t("support.publicWarning")}</small>
        {action === "saved" ? <span role="status">{t("support.saved")}</span> : null}
        {action === "failed" ? <span role="alert">{t("support.actionFailed")}</span> : null}
      </div>
      <div className="support-recovery-actions">
        <button type="button" onClick={() => void copy()}>
          <Copy aria-hidden="true" />{t(action === "copied" ? "support.copied" : "support.copySummary")}
        </button>
        <button type="button" disabled={action === "saving"} onClick={() => void save()}>
          <FileArchive aria-hidden="true" />{t(action === "saving" ? "support.saving" : "support.saveReport")}
        </button>
        <button type="button" onClick={() => void gateway.openIssue().catch(() => setAction("failed"))}>
          <ExternalLink aria-hidden="true" />{t("support.reportGitHub")}
        </button>
        <button type="button" onClick={onOpenSupport}>{t("support.openSupport")}</button>
      </div>
      <button className="support-recovery-dismiss" type="button" aria-label={t("support.dismiss")} onClick={() => void dismiss()}>
        <X aria-hidden="true" />
      </button>
    </aside>
  );
}
