import { useEffect, useMemo, useRef, type CSSProperties } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { catalogDayLabel, catalogRailDays } from "./catalogMonth";
import type { CatalogDayBucket } from "./types";

/**
 * The works page's day rail under the month rail: every day of the month
 * with its works on the current timeline and filters, the cell's fill showing
 * how busy the day is. A click locks the listing to that day; a second click
 * clears it. Days without works are ghosts, and a line under the cells marks
 * the days on screen. Narrow windows scroll the days sideways, centred on the
 * locked day, else the first marked one.
 */
export function CatalogDayRail({
  buckets,
  lockedDay,
  markedDays,
  onToggleDay,
}: {
  buckets: CatalogDayBucket[];
  lockedDay: string;
  markedDays: ReadonlySet<string>;
  onToggleDay: (day: string) => void;
}) {
  const { locale, t } = usePresentation();
  const days = useMemo(() => catalogRailDays(buckets), [buckets]);
  const scroller = useRef<HTMLDivElement>(null);
  const scrolledOnce = useRef(false);
  const focusDay = lockedDay || days.find((day) => markedDays.has(day.day))?.day || "";

  useEffect(() => {
    const element = scroller.current;
    const target = focusDay ? element?.querySelector<HTMLElement>(`[data-day="${focusDay}"]`) : null;
    if (!element || !target) return;
    const reduceMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    element.scrollTo?.({
      left: target.offsetLeft - (element.clientWidth - target.offsetWidth) / 2,
      behavior: scrolledOnce.current && !reduceMotion ? "smooth" : "auto",
    });
    scrolledOnce.current = true;
  }, [focusDay]);

  if (days.length === 0) return null;
  return (
    <div className="catalog-day-rail-scroll" ref={scroller}>
      <div
        className="catalog-day-rail"
        role="group"
        aria-label={t("catalog.worksPerDay")}
        style={{ "--catalog-rail-days": days.length } as CSSProperties}
      >
        {days.map((day) => {
          const locked = day.day === lockedDay;
          const label = catalogDayLabel(day.day, locale);
          return (
            <div className="catalog-day-rail-cell" data-day={day.day} key={day.day}>
              <button
                className={locked ? "active" : ""}
                type="button"
                disabled={day.count === null}
                aria-pressed={day.count === null ? undefined : locked}
                aria-label={day.count === null
                  ? label
                  : t("catalog.dayCountAria", { day: label, count: day.count.toLocaleString(locale) })}
                onClick={() => onToggleDay(day.day)}
              >
                {day.fill > 0 && <i aria-hidden="true" style={{ height: `${Math.round(day.fill * 100)}%` }} />}
                <strong>{day.date}</strong>
                <small>{day.count === null ? "" : day.count.toLocaleString(locale)}</small>
              </button>
              <span className={markedDays.has(day.day) ? "marked" : ""} aria-hidden="true" />
            </div>
          );
        })}
      </div>
    </div>
  );
}
