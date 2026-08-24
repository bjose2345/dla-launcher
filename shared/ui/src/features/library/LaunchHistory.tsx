import {
  CheckCircle2,
  CircleSlash2,
  LoaderCircle,
  OctagonX,
  Square,
} from "lucide-react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { launchAdapterMessageKey } from "../../i18n/domainLabels";
import type { LaunchActivity, LaunchActivityStatus } from "./types";
import { launchActivityIsActive } from "./types";

export function LaunchActivityList({
  activities,
  compact = false,
  showInstallation = false,
  stoppingActivityId = null,
  onStop,
}: {
  activities: LaunchActivity[];
  compact?: boolean;
  showInstallation?: boolean;
  stoppingActivityId?: string | null;
  onStop?: (activityId: string) => void;
}) {
  const { locale, t } = usePresentation();

  if (activities.length === 0) {
    return <p className="launch-history-empty">{t("library.launchHistoryEmpty")}</p>;
  }

  return (
    <ol className={`launch-history-list${compact ? " launch-history-list-compact" : ""}`}>
      {activities.map((activity) => {
        const canStop = onStop
          && launchActivityIsActive(activity.status)
          && activity.status !== "stopping";
        return (
          <li className={`launch-history-entry launch-history-${activity.status}`} key={activity.id}>
            <span className="launch-history-icon" aria-hidden="true">
              <LaunchStatusIcon status={activity.status} />
            </span>
            <div className="launch-history-copy">
              <div>
                <strong>{launchStatusLabel(activity.status, t)}</strong>
                {showInstallation ? <code>{activity.installationId}</code> : null}
              </div>
              <span>
                {formatLaunchTime(activity.attemptedAt, locale)}
                {activity.adapter ? ` · ${t(launchAdapterMessageKey(activity.adapter))}` : ""}
                {activity.processId !== null ? ` · PID ${activity.processId}` : ""}
              </span>
              <span>
                {activity.durationMs !== null
                  ? t("library.launchDuration", { duration: formatDuration(activity.durationMs, t) })
                  : launchActivityIsActive(activity.status) && activity.startedAt
                    ? t("library.launchRunningFor", {
                      duration: formatDuration(Math.max(0, Date.now() - Date.parse(activity.startedAt)), t),
                    })
                    : null}
                {activity.exitCode !== null ? ` · ${t("library.launchExitCode", { code: activity.exitCode })}` : ""}
              </span>
              {activity.error
                ? <em>{t("common.technicalDetail", { detail: activity.error })}</em>
                : null}
            </div>
            {canStop ? (
              <button
                className="launch-history-stop"
                type="button"
                disabled={stoppingActivityId === activity.id}
                onClick={() => onStop(activity.id)}
              >
                {stoppingActivityId === activity.id
                  ? <LoaderCircle className="library-spin" aria-hidden="true" />
                  : <Square aria-hidden="true" />}
                {t("library.stopLaunch")}
              </button>
            ) : null}
          </li>
        );
      })}
    </ol>
  );
}

export function launchStatusLabel(
  status: LaunchActivityStatus,
  t: ReturnType<typeof usePresentation>["t"],
): string {
  switch (status) {
    case "starting": return t("library.launchStatus.starting");
    case "running": return t("library.launchStatus.running");
    case "stopping": return t("library.launchStatus.stopping");
    case "exited": return t("library.launchStatus.exited");
    case "failed": return t("library.launchStatus.failed");
    case "stopped": return t("library.launchStatus.stopped");
    case "interrupted": return t("library.launchStatus.interrupted");
  }
}

export function formatDuration(
  durationMs: number,
  t: ReturnType<typeof usePresentation>["t"],
): string {
  const seconds = Math.max(0, Math.floor(durationMs / 1000));
  if (seconds < 60) return t("unit.secondShort", { value: seconds });
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  if (minutes < 60) {
    return `${t("unit.minuteShort", { value: minutes })} ${t("unit.secondShort", { value: remainder })}`;
  }
  const hours = Math.floor(minutes / 60);
  return `${t("unit.hourShort", { value: hours })} ${t("unit.minuteShort", { value: minutes % 60 })}`;
}

function LaunchStatusIcon({ status }: { status: LaunchActivityStatus }) {
  if (status === "starting" || status === "running" || status === "stopping") {
    return <LoaderCircle className="library-spin" />;
  }
  if (status === "exited") return <CheckCircle2 />;
  if (status === "failed") return <OctagonX />;
  if (status === "stopped") return <Square />;
  return <CircleSlash2 />;
}

function formatLaunchTime(value: string, locale: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return value;
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}
