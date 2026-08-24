import { Clock3, Repeat } from "lucide-react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { playTimeParts } from "./libraryHome";
import type { LibraryLaunchTotals } from "./types";

export function playTimeLabel(
  totalDurationMs: number,
  t: ReturnType<typeof usePresentation>["t"],
): string {
  const { hours, minutes } = playTimeParts(totalDurationMs);
  return hours > 0
    ? t("library.playTime.hours", { hours, minutes })
    : t("library.playTime.minutes", { minutes });
}

export function LibraryPlayTime({ totals }: { totals: LibraryLaunchTotals }) {
  const { t } = usePresentation();
  return (
    <span className="library-play-time">
      <span title={t("library.playTime.total")}>
        <Clock3 aria-hidden="true" />{playTimeLabel(totals.totalDurationMs, t)}
      </span>
      <span title={t("library.playTime.launches")}>
        <Repeat aria-hidden="true" />{totals.launchCount}
      </span>
    </span>
  );
}
