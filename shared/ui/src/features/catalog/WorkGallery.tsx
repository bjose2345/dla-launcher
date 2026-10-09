import { LayoutGrid, LayoutPanelLeft } from "lucide-react";
import { useReducedMotion } from "motion/react";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react";
import { flushSync } from "react-dom";

import { usePresentation } from "../../preferences/PresentationProvider";
import { ArchiveImagePlaceholder } from "./ArchiveImagePlaceholder";
import {
  balanceGalleryColumns,
  bentoSpans,
  defaultGalleryRatio,
  galleryColumnCount,
  readGalleryLayout,
  writeGalleryLayout,
  type BentoSpan,
  type GalleryLayout,
} from "./galleryLayout";

const noMorph: ReadonlySet<number> = new Set();

type ViewTransitionDocument = Document & {
  startViewTransition?: (update: () => void) => { finished: Promise<void> };
};

type GalleryTileState = {
  sourceIndex: number;
  ratio: number;
  loaded: boolean;
  failed: boolean;
};

export function WorkGallery({
  images,
  layout = "grid",
  animate = true,
  morphing = noMorph,
  onOpen,
}: {
  images: string[][];
  layout?: GalleryLayout;
  /** False once the layout was switched: the tiles then morph instead of rising in again. */
  animate?: boolean;
  /** Tiles that carry a view-transition name while the layout morphs. */
  morphing?: ReadonlySet<number>;
  onOpen: (index: number) => void;
}) {
  const { t } = usePresentation();
  const rootRef = useRef<HTMLDivElement>(null);
  const imageKey = useMemo(() => images.map((chain) => chain.join("|")).join("||"), [images]);
  const [columnCount, setColumnCount] = useState<1 | 2 | 3>(() =>
    galleryColumnCount(typeof window === "undefined" ? 1024 : window.innerWidth),
  );
  const [states, setStates] = useState<GalleryTileState[]>(() => createTileStates(images));

  useEffect(() => setStates(createTileStates(images)), [imageKey]);

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const measure = () => setColumnCount(galleryColumnCount(root.getBoundingClientRect().width));
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    return () => observer.disconnect();
  }, [layout]);

  const columns = useMemo(
    () => balanceGalleryColumns(images.map((_, index) => states[index]?.ratio ?? defaultGalleryRatio), columnCount),
    [columnCount, images, states],
  );

  const spans = useMemo(() => bentoSpans(images.length), [images.length]);

  if (!images.length) return <p className="work-detail-empty">{t("detail.galleryEmpty")}</p>;

  const tile = (index: number) => {
    const chain = images[index] ?? [];
    const state = states[index] ?? createTileState(chain);
    return (
      <GalleryTile
        chain={chain}
        index={index}
        state={state}
        bento={layout === "bento" ? spans[index] : undefined}
        animate={animate}
        morphName={morphing.has(index) ? `work-gallery-tile-${index}` : undefined}
        onOpen={onOpen}
        onLoad={(ratio) => setStates((current) => updateTileState(current, index, {
          ratio,
          loaded: true,
          failed: false,
        }))}
        onError={() => setStates((current) => advanceTileSource(current, images, index))}
        openLabel={t("detail.galleryOpenImage", { number: index + 1 })}
        unavailableLabel={t("detail.galleryImageUnavailable")}
        key={`${index}:${chain.join("|")}`}
      />
    );
  };

  if (layout === "bento") {
    return (
      <div className="work-gallery-bento" ref={rootRef}>
        {images.map((_, index) => tile(index))}
      </div>
    );
  }
  return (
    <div className={`work-gallery-grid columns-${columnCount}`} ref={rootRef}>
      {columns.map((indices, columnIndex) => (
        <div className="work-gallery-column" key={`column:${columnIndex}`}>
          {indices.map((index) => tile(index))}
        </div>
      ))}
    </div>
  );
}

/**
 * The gallery's layout, remembered across sessions, and its switch. Switching
 * morphs each tile from its old box to its new one with a view transition.
 * Only tiles within a screen of the viewport are named, since each named
 * tile is its own snapshot; the rest ride the page cross-fade.
 */
export function useGalleryLayout(containerRef: RefObject<HTMLElement | null>) {
  const reduceMotion = useReducedMotion() ?? false;
  const [layout, setLayout] = useState<GalleryLayout>(readGalleryLayout);
  const [switched, setSwitched] = useState(false);
  const [morphing, setMorphing] = useState<ReadonlySet<number>>(noMorph);

  const change = useCallback((next: GalleryLayout) => {
    if (next === layout) return;
    writeGalleryLayout(next);
    const doc = document as ViewTransitionDocument;
    const container = containerRef.current;
    const visible = container && container.closest("details")?.open !== false;
    if (reduceMotion || !visible || typeof doc.startViewTransition !== "function") {
      setSwitched(true);
      setLayout(next);
      return;
    }
    const reach = window.innerHeight;
    const near = new Set(Array.from(container.querySelectorAll<HTMLElement>("[data-gallery-index]"))
      .filter((element) => {
        const box = element.getBoundingClientRect();
        return box.bottom > -reach && box.top < window.innerHeight + reach;
      })
      .map((element) => Number(element.dataset.galleryIndex)));
    flushSync(() => {
      setSwitched(true);
      setMorphing(near);
    });
    doc.startViewTransition(() => flushSync(() => setLayout(next)))
      .finished
      .finally(() => setMorphing(noMorph));
  }, [containerRef, layout, reduceMotion]);

  return { layout, animate: !switched, morphing, change };
}

/** Bento or Grid, beside the gallery's heading. It sits inside the section's
 *  summary, so its clicks never fold the section. */
export function GalleryLayoutToggle({
  layout,
  onChange,
}: {
  layout: GalleryLayout;
  onChange: (layout: GalleryLayout) => void;
}) {
  const { t } = usePresentation();
  const option = (value: GalleryLayout, label: string, icon: ReactNode) => (
    <button
      type="button"
      aria-pressed={layout === value}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        onChange(value);
      }}
    >
      {icon}
      <span>{label}</span>
    </button>
  );
  return (
    <div className="work-gallery-layout" role="group" aria-label={t("detail.galleryLayout")}>
      {option("bento", t("detail.galleryBento"), <LayoutPanelLeft aria-hidden="true" />)}
      {option("grid", t("detail.galleryGrid"), <LayoutGrid aria-hidden="true" />)}
    </div>
  );
}

function GalleryTile({
  chain,
  index,
  state,
  bento,
  animate,
  morphName,
  onOpen,
  onLoad,
  onError,
  openLabel,
  unavailableLabel,
}: {
  chain: string[];
  index: number;
  state: GalleryTileState;
  bento?: BentoSpan;
  animate: boolean;
  morphName?: string;
  onOpen: (index: number) => void;
  onLoad: (ratio: number) => void;
  onError: () => void;
  openLabel: string;
  unavailableLabel: string;
}) {
  const source = chain[state.sourceIndex];
  const ready = state.loaded || state.failed;

  return (
    <button
      type="button"
      className={`work-gallery-tile cover-hover-trigger cover-hover-frame${ready ? " is-loaded" : ""}${animate ? "" : " is-settled"}`}
      data-gallery-index={index}
      data-bento={bento}
      style={{
        animationDelay: `${(index % 5) * 0.06}s`,
        viewTransitionName: morphName,
      } as CSSProperties}
      onClick={() => onOpen(index)}
      aria-label={openLabel}
    >
      <span className="work-gallery-frame">
        <span className="work-gallery-image cover-hover-media">
          {state.failed || !source ? (
            <ArchiveImagePlaceholder className="work-gallery-placeholder" label={unavailableLabel} />
          ) : (
            <img
              src={source}
              alt=""
              loading={index < 4 ? "eager" : "lazy"}
              decoding="async"
              onLoad={(event) => {
                const image = event.currentTarget;
                onLoad(image.naturalWidth && image.naturalHeight
                  ? image.naturalWidth / image.naturalHeight
                  : defaultGalleryRatio);
              }}
              onError={onError}
            />
          )}
        </span>
      </span>
    </button>
  );
}

function createTileStates(images: string[][]): GalleryTileState[] {
  return images.map(createTileState);
}

function createTileState(chain: string[]): GalleryTileState {
  return {
    sourceIndex: 0,
    ratio: defaultGalleryRatio,
    loaded: false,
    failed: chain.length === 0,
  };
}

function updateTileState(
  states: GalleryTileState[],
  index: number,
  updates: Partial<GalleryTileState>,
): GalleryTileState[] {
  return states.map((state, stateIndex) => stateIndex === index ? { ...state, ...updates } : state);
}

function advanceTileSource(states: GalleryTileState[], images: string[][], index: number): GalleryTileState[] {
  const current = states[index];
  const chain = images[index] ?? [];
  if (!current) return states;
  const nextSource = current.sourceIndex + 1;
  return updateTileState(states, index, nextSource < chain.length
    ? { sourceIndex: nextSource, ratio: defaultGalleryRatio, loaded: false, failed: false }
    : { ratio: defaultGalleryRatio, loaded: false, failed: true });
}
