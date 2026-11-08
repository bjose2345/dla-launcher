import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";

import { AnimatedDots } from "../../app/AnimatedDots";
import { Notice } from "../../app/Notice";
import { useGatewaySubscription } from "../../app/useGatewaySubscription";
import { usePresentation } from "../../preferences/PresentationProvider";
import { formatDuration } from "./LaunchHistory";
import { installationTitle } from "./libraryPresentation";
import { launchActivityIsActive, type LaunchActivity, type LibraryGateway } from "./types";
import { pluralText } from "../../i18n/pluralText";

export type ActiveLaunchGateway = Pick<
  LibraryGateway,
  "listRecentLaunches" | "subscribeLaunchActivity" | "launchInstallation" | "readInstallation"
>;

/**
 * How often to ask for the launches again: without launch events, often while
 * one runs and now and then otherwise; with events, only a slow safety net
 * while one runs.
 */
export function launchPollInterval(activities: LaunchActivity[] | undefined, eventsAvailable: boolean): number | false {
  const active = activities?.some((activity) => launchActivityIsActive(activity.status)) ?? false;
  if (!eventsAvailable) return active ? 750 : 15_000;
  return active ? 5_000 : false;
}

/** A game that ends by itself this soon after it started most likely could not run. */
const QUICK_EXIT_MS = 15_000;

export function activeLaunch(activities: LaunchActivity[]): LaunchActivity | null {
  return activities.find((activity) => launchActivityIsActive(activity.status)) ?? null;
}

/**
 * The launches that just ended by themselves right after they started.
 * `handled` holds the launches already looked at; the finished ones among
 * `activities` are added to it, so each is reported once.
 */
export function quickExits(activities: LaunchActivity[], handled: Set<string>): LaunchActivity[] {
  const found: LaunchActivity[] = [];
  for (const activity of activities) {
    if (handled.has(activity.id) || launchActivityIsActive(activity.status)) continue;
    handled.add(activity.id);
    // A crash counts too: the game started, then failed on its own. One that
    // never started has no duration and gets the launch error instead.
    if (
      (activity.status === "exited" || activity.status === "failed")
      && activity.stopRequestedAt === null
      && activity.durationMs !== null
      && activity.durationMs < QUICK_EXIT_MS
    ) {
      found.push(activity);
    }
  }
  return found;
}

export function ActiveLaunchPill({
  gateway,
  onOpenLibrary,
}: {
  gateway: ActiveLaunchGateway;
  onOpenLibrary: () => void;
}) {
  const { locale, t } = usePresentation();
  const queryClient = useQueryClient();
  const launches = useQuery({
    queryKey: ["library", "launches", "recent"],
    queryFn: () => gateway.listRecentLaunches(50),
    refetchInterval: (query) => launchPollInterval(query.state.data, Boolean(gateway.subscribeLaunchActivity)),
  });
  const invalidate = () => Promise.all([
    queryClient.invalidateQueries({ queryKey: ["library", "launches"] }),
    queryClient.invalidateQueries({ queryKey: ["library", "shelves"] }),
  ]);
  useGatewaySubscription(
    gateway.subscribeLaunchActivity?.bind(gateway),
    (activity) => {
      void queryClient.invalidateQueries({ queryKey: ["library", "launches"] });
      // Shelves count a launch once it has ended.
      if (!launchActivityIsActive(activity.status)) {
        void queryClient.invalidateQueries({ queryKey: ["library", "shelves"] });
      }
    },
    [gateway],
  );
  const relaunch = useMutation({
    mutationFn: (installationId: string) => gateway.launchInstallation(installationId),
    onSettled: invalidate,
  });
  const active = activeLaunch(launches.data ?? []);
  const elapsed = useElapsed(active?.startedAt ?? null);

  // Launches that had finished before the launcher opened are not news.
  const handled = useRef<Set<string> | null>(null);
  const [closed, setClosed] = useState<{ activity: LaunchActivity; title: string } | null>(null);
  useEffect(() => {
    const activities = launches.data;
    if (!activities) return;
    if (!handled.current) {
      handled.current = new Set(
        activities.filter((activity) => !launchActivityIsActive(activity.status)).map((activity) => activity.id),
      );
      return;
    }
    const [exited] = quickExits(activities, handled.current);
    if (!exited) return;
    void gateway.readInstallation(exited.installationId)
      .then(installationTitle, () => exited.targetPath ?? "")
      .then((title) => setClosed({ activity: exited, title }));
  }, [gateway, launches.data]);

  const seconds = Math.max(1, Math.round((closed?.activity.durationMs ?? 0) / 1_000));
  const notice = closed ? (
    <Notice
      tone="problem"
      title={pluralText(t, locale, seconds, "library.closedSoonOne", "library.closedSoon", { title: closed.title, seconds })}
      detail={t("library.closedSoonHelp")}
      actions={[{
        label: t("library.launchAgain"),
        primary: true,
        onClick: () => {
          relaunch.mutate(closed.activity.installationId);
          setClosed(null);
        },
      }]}
      onClose={() => setClosed(null)}
    />
  ) : relaunch.error ? (
    <Notice
      tone="problem"
      title={t("common.requestFailed", { error: String(relaunch.error) })}
      onClose={() => relaunch.reset()}
    />
  ) : null;

  if (!active) return notice;

  return (
    <>
      <button className="app-launch-running" type="button" onClick={onOpenLibrary}>
        <strong>{t("library.launchStatus.running")}</strong>
        <AnimatedDots />
        {elapsed === null ? null : <small>{formatDuration(elapsed, t)}</small>}
      </button>
      {notice}
    </>
  );
}

function useElapsed(startedAt: string | null): number | null {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!startedAt) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [startedAt]);

  if (!startedAt) return null;
  const started = Date.parse(startedAt);
  return Number.isNaN(started) ? null : Math.max(0, now - started);
}
