import { useEffect, useRef, useState } from "react";

import { AnchoredPopover } from "../../app/AnchoredPopover";
import { usePresentation } from "../../preferences/PresentationProvider";
import {
  catalogMonthLabel,
  catalogMonthNavigation,
  catalogRailMonths,
  catalogYearHasMonths,
} from "./catalogMonth";
import type { CatalogMonthBucket } from "./types";

const YEAR_PAGE_SIZE = 24;

/**
 * The works page's month navigation: the shown year's 12 months with their
 * counts, a year picker, and the month-step arrows at the ends. A month is
 * pickable when it is inside the timeline's range and has works; a year when
 * any of its months is. Narrow windows scroll the months sideways, centred on
 * the one on screen.
 */
export function CatalogMonthRail({
  month,
  minMonth,
  maxMonth,
  months,
  onPick,
  onStep,
}: {
  /** The month on screen (YYYY-MM). */
  month: string;
  minMonth: string;
  maxMonth: string;
  months: CatalogMonthBucket[];
  onPick: (month: string) => void;
  onStep: (month: string) => void;
}) {
  const { locale, t } = usePresentation();
  const shownYear = Number(month.slice(0, 4)) || new Date().getFullYear();
  const minYear = Number(minMonth.slice(0, 4)) || shownYear;
  const maxYear = Number(maxMonth.slice(0, 4)) || shownYear;

  // Picking a year previews its months without moving; the preview lapses as
  // soon as the month on screen changes.
  const [preview, setPreview] = useState<{ from: string; year: number } | null>(null);
  const year = preview?.from === month ? preview.year : shownYear;
  const rail = catalogRailMonths(year, minMonth, maxMonth, months);
  const navigation = catalogMonthNavigation(month, months);
  const monthNames = Array.from({ length: 12 }, (_, index) =>
    new Date(Date.UTC(2000, index, 1)).toLocaleDateString(locale, { month: "short", timeZone: "UTC" }));

  const [yearsOpen, setYearsOpen] = useState(false);
  const [yearBase, setYearBase] = useState(() => yearPageBase(year, minYear));
  const yearButton = useRef<HTMLButtonElement>(null);
  const years = Array.from({ length: YEAR_PAGE_SIZE }, (_, index) => yearBase + index)
    .filter((value) => value <= maxYear);
  const pagedYears = maxYear - minYear + 1 > YEAR_PAGE_SIZE;

  const pickYear = (value: number) => {
    setPreview(value === shownYear ? null : { from: month, year: value });
    setYearsOpen(false);
    yearButton.current?.focus();
  };

  const scroller = useRef<HTMLDivElement>(null);
  const scrolledOnce = useRef(false);
  useEffect(() => {
    const element = scroller.current;
    const active = element?.querySelector<HTMLElement>('[aria-current="date"]');
    if (!element || !active) return;
    const reduceMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    element.scrollTo?.({
      left: active.offsetLeft - (element.clientWidth - active.offsetWidth) / 2,
      behavior: scrolledOnce.current && !reduceMotion ? "smooth" : "auto",
    });
    scrolledOnce.current = true;
  }, [month, year]);

  return (
    <div className="catalog-month-rail">
      <button
        ref={yearButton}
        className="catalog-month-rail-year"
        type="button"
        aria-haspopup="dialog"
        aria-expanded={yearsOpen}
        aria-label={`${t("catalog.year")} ${year}`}
        onClick={() => {
          if (!yearsOpen) setYearBase(yearPageBase(year, minYear));
          setYearsOpen((open) => !open);
        }}
      >
        {year}
        <svg className={yearsOpen ? "expanded" : ""} viewBox="0 0 24 24" aria-hidden="true">
          <polyline points="7 9 12 14 17 9" />
        </svg>
      </button>
      {yearsOpen && (
        <AnchoredPopover
          anchorRef={yearButton}
          className="catalog-month-picker"
          role="dialog"
          ariaLabel={t("catalog.year")}
          gap={8}
          maximumWidth={310}
          onClose={() => setYearsOpen(false)}
        >
          {pagedYears && (
            <div className="catalog-month-picker-header">
              <strong>{years[0]} – {years.at(-1)}</strong>
              <span>
                <button
                  type="button"
                  disabled={yearBase <= minYear}
                  aria-label={t("catalog.earlierYears")}
                  onClick={() => setYearBase((base) => Math.max(minYear, base - YEAR_PAGE_SIZE))}
                ><Chevron direction="previous" /></button>
                <button
                  type="button"
                  disabled={yearBase + YEAR_PAGE_SIZE > maxYear}
                  aria-label={t("catalog.laterYears")}
                  onClick={() => setYearBase((base) => base + YEAR_PAGE_SIZE)}
                ><Chevron direction="next" /></button>
              </span>
            </div>
          )}
          <div className="catalog-month-picker-grid">
            {years.map((value) => (
              <button
                className={value === year ? "active" : ""}
                type="button"
                disabled={!catalogYearHasMonths(value, minMonth, maxMonth, months)}
                onClick={() => pickYear(value)}
                key={value}
              >{value}</button>
            ))}
          </div>
        </AnchoredPopover>
      )}

      <MonthArrow
        direction="previous"
        disabled={!navigation.previous}
        onClick={() => navigation.previous && onStep(navigation.previous)}
      />
      <div className="catalog-month-rail-scroll" ref={scroller}>
        <div className="catalog-month-rail-months" role="group" aria-label={t("catalog.pickMonth")}>
          {rail.map((cell, index) => {
            const label = catalogMonthLabel(cell.month, locale);
            return (
              <button
                className={cell.month === month ? "active" : ""}
                type="button"
                disabled={!cell.enabled}
                aria-current={cell.month === month ? "date" : undefined}
                aria-label={cell.count === null ? label : `${label} · ${cell.count.toLocaleString(locale)}`}
                onClick={() => onPick(cell.month)}
                key={cell.month}
              >
                <strong>{monthNames[index]}</strong>
                <small>{cell.count === null ? "" : cell.count.toLocaleString(locale)}</small>
              </button>
            );
          })}
        </div>
      </div>
      <MonthArrow
        direction="next"
        disabled={!navigation.next}
        onClick={() => navigation.next && onStep(navigation.next)}
      />
    </div>
  );
}

function MonthArrow({
  direction,
  disabled,
  onClick,
}: {
  direction: "previous" | "next";
  disabled: boolean;
  onClick: () => void;
}) {
  const { t } = usePresentation();
  return (
    <button
      className="catalog-month-rail-arrow"
      type="button"
      disabled={disabled}
      aria-label={t(direction === "previous" ? "catalog.previousMonth" : "catalog.nextMonth")}
      onClick={onClick}
    ><Chevron direction={direction} /></button>
  );
}

function yearPageBase(year: number, minimum: number): number {
  return minimum + Math.floor(Math.max(0, year - minimum) / YEAR_PAGE_SIZE) * YEAR_PAGE_SIZE;
}

function Chevron({ direction }: { direction: "previous" | "next" }) {
  return <svg viewBox="0 0 24 24" aria-hidden="true"><polyline points={direction === "previous" ? "15 18 9 12 15 6" : "9 18 15 12 9 6"} /></svg>;
}
