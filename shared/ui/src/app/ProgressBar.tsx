import type { MessageKey } from "../i18n/catalogs";

export type ProgressBarState = "running" | "waiting" | "paused" | "failed" | "done";

type Translate = (key: MessageKey, values?: Record<string, string | number>) => string;

/**
 * How far a task has got, drawn alike everywhere in the launcher. While it
 * runs the bar fills with a moving shine; while it waits, or before the amount
 * is known, a sweep runs along it. A paused bar turns grey, a failed one red,
 * and a finished one stays full and still.
 */
export function ProgressBar({
  percent,
  state = "running",
  size = "card",
  label,
  className,
}: {
  /** 0 to 100, or null while the amount is not known yet. */
  percent: number | null;
  state?: ProgressBarState;
  size?: "card" | "slim";
  /** What the bar measures, for screen readers. Left out, the text beside it says so. */
  label?: string;
  className?: string;
}) {
  const value = percent === null ? null : Math.min(100, Math.max(0, percent));
  const classes = ["app-progress", `is-${size}`, `is-${state}`];
  if (value === null) classes.push("is-unknown");
  if (className) classes.push(className);
  return (
    <div
      className={classes.join(" ")}
      role={label ? "progressbar" : undefined}
      aria-hidden={label ? undefined : true}
      aria-label={label}
      aria-valuemin={label ? 0 : undefined}
      aria-valuemax={label ? 100 : undefined}
      aria-valuenow={label && value !== null ? Math.round(value) : undefined}
    >
      {value === null ? null : <i style={{ width: `${value}%` }} />}
    </div>
  );
}

/** "about 12 min left", worded alike for every task with an estimate. */
export function timeLeftText(t: Translate, seconds: number): string | null {
  if (!(seconds > 0)) return null;
  const minutes = Math.ceil(seconds / 60);
  if (minutes <= 1) return t("progress.timeLeft.soon");
  if (minutes < 60) return t("progress.timeLeft.minutes", { minutes });
  return t("progress.timeLeft.hours", { hours: Math.floor(minutes / 60), minutes: minutes % 60 });
}
