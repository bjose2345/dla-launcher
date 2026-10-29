import { AlertTriangle, RefreshCw } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import type { PdfDocument } from "./pdfDocument";
import type { PageBox } from "./readerModel";

/** A page of an image set; until its own size is known it is letterboxed in a typical page shape. */
export function ImagePage({
  source,
  label,
  box,
  onNaturalSize,
}: {
  source: string;
  label: string;
  box: PageBox;
  onNaturalSize: (size: PageBox) => void;
}) {
  const [attempt, setAttempt] = useState(0);
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [source]);
  if (failed) return <PageError box={box} onRetry={() => { setFailed(false); setAttempt((value) => value + 1); }} />;
  return (
    <img
      className="reader-page"
      key={attempt}
      src={source}
      alt={label}
      draggable={false}
      decoding="async"
      style={{ width: box.width, height: box.height }}
      onLoad={(event) => onNaturalSize({
        width: event.currentTarget.naturalWidth,
        height: event.currentTarget.naturalHeight,
      })}
      onError={() => setFailed(true)}
    />
  );
}

/** A PDF page drawn on a canvas the document keeps for a while. */
export function PdfPage({
  pdf,
  index,
  label,
  box,
}: {
  pdf: PdfDocument;
  index: number;
  label: string;
  box: PageBox;
}) {
  const holderRef = useRef<HTMLDivElement>(null);
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<"drawing" | "ready" | "failed">("drawing");

  useLayoutEffect(() => {
    let current = true;
    setState("drawing");
    pdf.render(index, box.width).then(
      (canvas) => {
        const holder = holderRef.current;
        if (!current || !holder) return;
        canvas.className = "reader-page-canvas";
        holder.replaceChildren(canvas);
        setState("ready");
      },
      () => current && setState("failed"),
    );
    return () => {
      current = false;
    };
  }, [attempt, box.width, index, pdf]);

  if (state === "failed") return <PageError box={box} onRetry={() => setAttempt((value) => value + 1)} />;
  return (
    <div
      className={`reader-page reader-pdf-page${state === "drawing" ? " is-drawing" : ""}`}
      ref={holderRef}
      role="img"
      aria-label={label}
      style={{ width: box.width, height: box.height }}
    />
  );
}

function PageError({ box, onRetry }: { box: PageBox; onRetry: () => void }) {
  const { t } = usePresentation();
  return (
    <div className="reader-page reader-page-error" role="alert" style={{ width: box.width, height: box.height }}>
      <AlertTriangle aria-hidden="true" />
      <strong>{t("reader.pageUnavailable")}</strong>
      <button className="reader-chip-button" type="button" onClick={onRetry}>
        <RefreshCw aria-hidden="true" />{t("media.reader.retryPage")}
      </button>
    </div>
  );
}

export function PdfThumbnail({ pdf, index }: { pdf: PdfDocument; index: number }) {
  const holderRef = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    let current = true;
    // Thumbnails wait their turn behind the page being read.
    const timer = window.setTimeout(() => {
      pdf.render(index, THUMBNAIL_WIDTH).then((canvas) => {
        if (current) holderRef.current?.replaceChildren(canvas);
      }, () => undefined);
    }, 120);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [index, pdf]);
  return <span className="reader-thumbnail-art" ref={holderRef} />;
}

export const THUMBNAIL_WIDTH = 46;

/**
 * Every page in one column. Pages near the screen are drawn; the page that
 * fills most of the screen is reported as the current one.
 */
export function StripView({
  count,
  current,
  pageBoxFor,
  renderPage,
  onCurrent,
}: {
  count: number;
  current: number;
  pageBoxFor: (index: number) => PageBox;
  renderPage: (index: number, box: PageBox) => React.ReactNode;
  onCurrent: (index: number) => void;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const [near, setNear] = useState<ReadonlySet<number>>(() => new Set([current]));
  const reportedRef = useRef(current);
  const scrollingToRef = useRef<number | null>(null);

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const nearby = new IntersectionObserver((entries) => {
      setNear((previous) => {
        const next = new Set(previous);
        for (const entry of entries) {
          const index = Number((entry.target as HTMLElement).dataset.page);
          if (entry.isIntersecting) next.add(index);
          else next.delete(index);
        }
        return next;
      });
    }, { root, rootMargin: "150% 0px" });
    const reading = new IntersectionObserver((entries) => {
      const best = entries
        .filter((entry) => entry.isIntersecting)
        .sort((left, right) => right.intersectionRatio - left.intersectionRatio)[0];
      if (!best) return;
      const index = Number((best.target as HTMLElement).dataset.page);
      if (scrollingToRef.current !== null) {
        if (index === scrollingToRef.current) scrollingToRef.current = null;
        return;
      }
      if (index !== reportedRef.current) {
        reportedRef.current = index;
        onCurrent(index);
      }
    }, { root, threshold: [0.25, 0.5, 0.75] });
    root.querySelectorAll<HTMLElement>("[data-page]").forEach((page) => {
      nearby.observe(page);
      reading.observe(page);
    });
    return () => {
      nearby.disconnect();
      reading.disconnect();
    };
  }, [count, onCurrent]);

  useEffect(() => {
    if (reportedRef.current === current) return;
    reportedRef.current = current;
    scrollingToRef.current = current;
    rootRef.current?.querySelector<HTMLElement>(`[data-page='${current}']`)?.scrollIntoView({ block: "start" });
  }, [current]);

  return (
    <div className="reader-strip" ref={rootRef}>
      {Array.from({ length: count }, (_, index) => {
        const box = pageBoxFor(index);
        const style: CSSProperties = { width: box.width, height: box.height };
        return (
          <div className="reader-strip-page" data-page={index} key={index} style={style}>
            {near.has(index) ? renderPage(index, box) : null}
          </div>
        );
      })}
    </div>
  );
}
