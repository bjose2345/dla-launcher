import { Trophy } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import type { CatalogRating } from "./types";
import { ratingCountValue } from "./ratingAnimation";

export function WorkRatingPanel({ rating }: { rating: CatalogRating | null }) {
  const { locale, t } = usePresentation();
  const numberFormat = new Intl.NumberFormat(locale);
  const score = rating ? Math.max(0, Math.min(100, Math.round((rating.score / 5) * 100))) : null;
  const animatedScore = useAnimatedScore(score);

  return (
    <>
      <span className="work-panel-tab work-hero-enter work-hero-enter-3">
        {t("detail.rating")} <small>{t("detail.ratingSecondary")}</small>
      </span>
      <section className="work-rating-panel work-hero-enter work-hero-enter-3">
        <div className="work-rating-summary">
          <RatingEmblem score={score} />
          <div>
            <span className="work-rating-label">{t("detail.dlsiteRating")}</span>
            <strong className="work-rating-score">{animatedScore === null ? "—" : `${animatedScore}%`}</strong>
            {rating && (rating.ratingCount !== null || rating.totalSales !== null) && (
              <p>
                {rating.ratingCount !== null && t("detail.ratingCount", { count: numberFormat.format(rating.ratingCount) })}
                {rating.ratingCount !== null && rating.totalSales !== null && <span> · </span>}
                {rating.totalSales !== null && t("detail.purchasedCount", { count: numberFormat.format(rating.totalSales) })}
              </p>
            )}
          </div>
        </div>
        {rating && rating.rankings.length > 0 && (
          <div className="work-ranking-list">
            {rating.rankings.map((ranking) => (
              <span className="work-rank-chip" key={ranking.range}>
                <Trophy aria-hidden="true" />
                <span>{ranking.range}</span>
                <strong>#{numberFormat.format(ranking.rank)}</strong>
              </span>
            ))}
          </div>
        )}
      </section>
    </>
  );
}

function useAnimatedScore(score: number | null): number | null {
  const [value, setValue] = useState<number | null>(() => score === null ? null : 0);

  useEffect(() => {
    if (score === null) {
      setValue(null);
      return;
    }
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setValue(score);
      return;
    }

    setValue(0);
    const started = performance.now();
    let frameId = 0;
    const frame = (time: number) => {
      const next = ratingCountValue(score, time - started);
      setValue(Math.round(next));
      if (next < score) frameId = window.requestAnimationFrame(frame);
    };
    frameId = window.requestAnimationFrame(frame);
    return () => window.cancelAnimationFrame(frameId);
  }, [score]);

  return value;
}

function RatingEmblem({ score }: { score: number | null }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;

    const grade = score === null
      ? "—"
      : score >= 90
        ? "S"
        : score >= 80
          ? "A"
          : score >= 70
            ? "B"
            : score >= 60
              ? "C"
              : "D";
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const primary = getComputedStyle(document.documentElement).getPropertyValue("--color-primary").trim() || "#f43f5e";
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const size = 112 * dpr;
    const unit = size / 112;
    const center = 56 * unit;
    canvas.width = size;
    canvas.height = size;

    const hex = (radius: number) => {
      context.beginPath();
      for (let index = 0; index < 6; index += 1) {
        const angle = (Math.PI / 3) * index - Math.PI / 2;
        const x = center + radius * Math.cos(angle);
        const y = center + radius * Math.sin(angle);
        if (index === 0) context.moveTo(x, y);
        else context.lineTo(x, y);
      }
      context.closePath();
    };

    const draw = (time: number) => {
      context.clearRect(0, 0, size, size);
      context.save();
      context.translate(center, center);
      context.rotate((time / 14000) * Math.PI * 2);
      for (let index = 0; index < 24; index += 1) {
        context.rotate(Math.PI / 12);
        context.fillStyle = index % 6 === 0 ? primary : "rgba(236,233,228,0.22)";
        context.globalAlpha = index % 6 === 0 ? 0.8 : 1;
        context.fillRect(46 * unit, -unit, (index % 6 === 0 ? 7 : 4) * unit, 2 * unit);
      }
      context.restore();
      context.globalAlpha = 1;
      const gradient = context.createLinearGradient(center - 40 * unit, center - 40 * unit, center + 40 * unit, center + 40 * unit);
      if (score === null) {
        gradient.addColorStop(0, "#3a3a44");
        gradient.addColorStop(0.5, "#565664");
        gradient.addColorStop(1, "#33333c");
      } else {
        gradient.addColorStop(0, "#8c1d31");
        gradient.addColorStop(0.5, primary);
        gradient.addColorStop(1, "#7c1d3a");
      }
      hex(38 * unit);
      context.fillStyle = gradient;
      context.fill();
      hex(38 * unit);
      context.strokeStyle = "rgba(255,255,255,0.35)";
      context.lineWidth = 1.5 * unit;
      context.stroke();
      hex(31 * unit);
      context.strokeStyle = "rgba(0,0,0,0.25)";
      context.lineWidth = 2 * unit;
      context.stroke();
      context.fillStyle = "#fff";
      context.font = `800 ${34 * unit}px system-ui, sans-serif`;
      context.textAlign = "center";
      context.textBaseline = "middle";
      context.shadowColor = "rgba(255,255,255,0.6)";
      context.shadowBlur = 10 * unit;
      context.fillText(grade, center, center + 2 * unit);
      context.shadowBlur = 0;
      const angle = ((time / 2600) % 1) * Math.PI * 2;
      const sheen = context.createLinearGradient(
        center - Math.cos(angle) * 40 * unit,
        center - Math.sin(angle) * 40 * unit,
        center + Math.cos(angle) * 40 * unit,
        center + Math.sin(angle) * 40 * unit,
      );
      sheen.addColorStop(0.42, "rgba(255,255,255,0)");
      sheen.addColorStop(0.5, "rgba(255,255,255,0.28)");
      sheen.addColorStop(0.58, "rgba(255,255,255,0)");
      hex(38 * unit);
      context.fillStyle = sheen;
      context.fill();
    };

    if (reduced) {
      draw(650);
      return;
    }

    let frameId = 0;
    const frame = (time: number) => {
      draw(time);
      frameId = window.requestAnimationFrame(frame);
    };
    const start = () => {
      if (!frameId) frameId = window.requestAnimationFrame(frame);
    };
    const stop = () => {
      if (frameId) window.cancelAnimationFrame(frameId);
      frameId = 0;
    };
    const visibility = () => {
      if (document.hidden) stop();
      else start();
    };
    document.addEventListener("visibilitychange", visibility);
    start();
    return () => {
      document.removeEventListener("visibilitychange", visibility);
      stop();
    };
  }, [score]);

  return <canvas className="work-rating-emblem" ref={canvasRef} width="112" height="112" aria-hidden="true" />;
}
