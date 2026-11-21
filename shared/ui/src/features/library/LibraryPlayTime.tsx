import { Clock3, Repeat } from "lucide-react";

import { pluralText } from "../../i18n/pluralText";
import { usePresentation } from "../../preferences/PresentationProvider";
import { lastPlayedDay, playTimeParts } from "./libraryHome";
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

/** A work's play time, when it was last played and how often, beside its Play button. */
export function LibraryPlayed({ totals }: { totals: LibraryLaunchTotals }) {
  const { locale, t } = usePresentation();
  const day = lastPlayedDay(totals.lastStartedAt, new Date());
  const last = day === "today"
    ? t("library.playTime.lastToday")
    : day === "yesterday"
      ? t("library.playTime.lastYesterday")
      : t("library.playTime.lastOn", {
        date: new Date(totals.lastStartedAt).toLocaleDateString(locale, {
          day: "numeric",
          month: "short",
          year: "numeric",
        }),
      });
  return (
    <div className="library-feature-played">
      <strong>{t("library.playTime.played", { time: playTimeLabel(totals.totalDurationMs, t) })}</strong>
      <span>
        {last}
        {" · "}
        {pluralText(t, locale, totals.launchCount, "library.playTime.timesOne", "library.playTime.times")}
      </span>
    </div>
  );
}
