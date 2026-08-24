import { useQuery } from "@tanstack/react-query";
import { Check, Copy, ExternalLink, Info, LoaderCircle, Users } from "lucide-react";
import { useEffect, useState } from "react";

import { BrandMark } from "../app/BrandMark";
import { formatByteSize } from "../features/importer/CatalogImportPage";
import { DeveloperEffect } from "../preferences/DeveloperEffect";
import { developers } from "../preferences/developers";
import { usePresentation } from "../preferences/PresentationProvider";
import { SettingsSection } from "../preferences/SettingsSection";
import type { SystemGateway, SystemReport } from "../preferences/systemReport";
import type { WindowGateway } from "../preferences/windowSizing";

export function AboutPage({
  systemGateway,
  windowGateway,
  onOpenProject,
  version = "",
}: {
  systemGateway?: SystemGateway;
  windowGateway?: WindowGateway;
  onOpenProject?: () => void | Promise<void>;
  version?: string;
}) {
  const { t } = usePresentation();
  return (
    <main className="about-page">
      <header className="about-page-masthead">
        <span>{t("settings.eyebrow")}</span>
        <h1>{t("nav.about")}</h1>
      </header>
      <div className="about-page-body">
        <CreditsSection onOpenProject={onOpenProject} />
        <AboutBuildSection
          gateway={systemGateway}
          windowGateway={windowGateway}
          version={version}
        />
      </div>
    </main>
  );
}

function CreditsSection({
  onOpenProject,
}: {
  onOpenProject?: () => void | Promise<void>;
}) {
  const { t } = usePresentation();
  return (
    <SettingsSection
      icon={<Users aria-hidden="true" />}
      title={t("settings.credits")}
      description={t("settings.creditsHelp")}
      action={onOpenProject ? <OpenProjectButton onOpen={onOpenProject} /> : undefined}
    >
      <div className="settings-crew">
        {developers.map((developer, index) => (
          <div
            className={`settings-developer is-${developer.effect}${index % 2 === 1 ? " is-right" : ""}`}
            key={developer.id}
          >
            <DeveloperEffect effect={developer.effect} side={index % 2 === 1 ? "right" : "left"} />
            <span className="settings-portrait">
              <img src={developer.portrait} alt="" width={84} height={84} loading="lazy" />
            </span>
            <span className="settings-developer-copy">
              <small>{t("settings.developer")}</small>
              <strong>{developer.name}</strong>
              <span className="settings-developer-quote">
                {developer.quote}
                {developer.quoteEmoji ? (
                  <>
                    {" "}
                    <span className="settings-developer-emoji">{developer.quoteEmoji}</span>
                  </>
                ) : null}
              </span>
            </span>
          </div>
        ))}
      </div>
    </SettingsSection>
  );
}

function AboutBuildSection({
  gateway,
  windowGateway,
  version,
}: {
  gateway?: SystemGateway;
  windowGateway?: WindowGateway;
  version: string;
}) {
  const { locale, t } = usePresentation();
  const system = useQuery({
    queryKey: ["about", "system-report"],
    queryFn: () => gateway!.readSystemReport(),
    enabled: Boolean(gateway),
  });
  const metrics = useQuery({
    queryKey: ["settings", "window-metrics"],
    queryFn: () => windowGateway!.readWindowMetrics(),
    enabled: Boolean(windowGateway),
  });
  const unknown = t("settings.factUnknown");
  const report = system.data;
  const screen = metrics.data
    ? `${metrics.data.workAreaWidth} × ${metrics.data.workAreaHeight}`
    : unknown;
  const cores = report && report.cpuCores > 0
    ? t("settings.factCores", { count: report.cpuCores.toLocaleString(locale) })
    : "";
  const processor = report?.cpu
    ? cores ? `${report.cpu} · ${cores}` : report.cpu
    : cores || unknown;
  const facts: Array<[string, string]> = [
    [t("settings.factVersion"), version || unknown],
    [t("settings.factSystem"), buildSystemLabel(report, unknown)],
    [t("settings.factKernel"), report?.kernel || unknown],
    [t("settings.factProcessor"), processor],
    [t("settings.factMemory"), report && report.memoryBytes > 0 ? formatByteSize(report.memoryBytes, locale) : unknown],
    [t("settings.factScreen"), screen],
    [t("settings.factRenderer"), report?.webview || unknown],
  ];

  return (
    <SettingsSection
      className="settings-about-build"
      icon={<Info aria-hidden="true" />}
      title={t("settings.aboutBuild")}
      description={t("settings.aboutBuildDescription")}
      action={<CopyBuildInfoButton facts={facts} />}
    >
      <div className="settings-brand-line">
        <BrandMark />
        <div>
          <strong>DLA Launcher</strong>
          <span>{t("settings.appTagline")}</span>
        </div>
      </div>
      <dl className="settings-facts">
        {facts.map(([label, value]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
      {system.isPending && gateway ? (
        <p className="settings-note settings-loading">
          <LoaderCircle className="settings-spin" aria-hidden="true" />
          {t("settings.systemLoading")}
        </p>
      ) : null}
      {system.error ? (
        <p className="settings-error" role="alert">
          {t("common.requestFailed", { error: String(system.error) })}
        </p>
      ) : null}
    </SettingsSection>
  );
}

function OpenProjectButton({ onOpen }: { onOpen: () => void | Promise<void> }) {
  const { t } = usePresentation();
  const open = async () => {
    try {
      await onOpen();
    } catch {
      return;
    }
  };

  return (
    <button className="settings-button" type="button" onClick={() => void open()}>
      <ExternalLink aria-hidden="true" />
      {t("support.visitGitHub")}
    </button>
  );
}

function CopyBuildInfoButton({ facts }: { facts: Array<[string, string]> }) {
  const { t } = usePresentation();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(timer);
  }, [copied]);

  const copy = () => {
    const text = facts.map(([label, value]) => `${label}: ${value}`).join("\n");
    void navigator.clipboard?.writeText(text).then(() => setCopied(true)).catch(() => undefined);
  };

  return (
    <button className="settings-button" type="button" onClick={copy}>
      {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
      {t(copied ? "settings.copiedBuildInfo" : "settings.copyBuildInfo")}
    </button>
  );
}

function buildSystemLabel(report: SystemReport | undefined, fallback: string): string {
  if (!report) return fallback;
  const name = report.osVersion || report.os;
  return report.arch ? `${name} · ${report.arch}` : name || fallback;
}
