import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { LoaderCircle, Square } from "lucide-react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { formatDuration } from "./LaunchHistory";
import { launchActivityIsActive, type LaunchActivity, type LibraryGateway } from "./types";

export type ActiveLaunchGateway = Pick<LibraryGateway, "listRecentLaunches" | "stopLaunch">;

export function activeLaunch(activities: LaunchActivity[]): LaunchActivity | null {
  return activities.find((activity) => launchActivityIsActive(activity.status)) ?? null;
}

export function ActiveLaunchPill({
  gateway,
  onOpenLibrary,
}: {
  gateway: ActiveLaunchGateway;
  onOpenLibrary: () => void;
}) {
  const { t } = usePresentation();
  const queryClient = useQueryClient();
  const launches = useQuery({
    queryKey: ["library", "launches", "recent"],
    queryFn: () => gateway.listRecentLaunches(50),
    refetchInterval: (query) => (
      query.state.data?.some((activity) => launchActivityIsActive(activity.status)) ? 750 : 15_000
    ),
  });
  const stop = useMutation({
    mutationFn: (activityId: string) => gateway.stopLaunch(activityId),
    onSettled: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["library", "launches"] }),
        queryClient.invalidateQueries({ queryKey: ["library", "shelves"] }),
      ]);
    },
  });
  const active = activeLaunch(launches.data ?? []);
  const elapsed = useElapsed(active?.startedAt ?? null);

  if (!active) return null;

  return (
    <div className="app-launch-pill">
      <button className="app-launch-open" type="button" onClick={onOpenLibrary}>
        <span className="app-launch-dot" aria-hidden="true" />
        <span className="app-launch-copy">
          <strong>{t("library.launchStatus.running")}</strong>
          {elapsed === null ? null : <small>{formatDuration(elapsed, t)}</small>}
        </span>
      </button>
      {active.status === "stopping" ? null : (
        <button
          className="app-launch-stop"
          type="button"
          title={t("library.stopLaunch")}
          aria-label={t("library.stopLaunch")}
          disabled={stop.isPending}
          onClick={() => stop.mutate(active.id)}
        >
          {stop.isPending
            ? <LoaderCircle className="library-spin" aria-hidden="true" />
            : <Square aria-hidden="true" />}
        </button>
      )}
    </div>
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
