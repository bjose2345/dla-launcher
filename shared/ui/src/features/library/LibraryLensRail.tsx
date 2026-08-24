import {
  AppWindow,
  BookOpen,
  Film,
  Headphones,
  Images,
  Library as LibraryIcon,
  type LucideIcon,
} from "lucide-react";

import { usePresentation } from "../../preferences/PresentationProvider";
import type { MessageKey } from "../../preferences/preferences";
import { libraryLenses, type LibraryLens } from "./libraryHome";

const lensIcons: Record<LibraryLens, LucideIcon> = {
  all: LibraryIcon,
  audio: Headphones,
  apps: AppWindow,
  images: Images,
  video: Film,
  documents: BookOpen,
};

const lensLabels: Record<LibraryLens, MessageKey> = {
  all: "library.home.filterAll",
  audio: "library.home.filterAudio",
  apps: "library.home.filterApps",
  images: "library.home.filterImages",
  video: "library.home.filterVideo",
  documents: "library.home.filterDocuments",
};

export function LibraryLensRail({
  lens,
  counts,
  needsReview,
  onSelect,
}: {
  lens: LibraryLens;
  counts: Map<LibraryLens, number>;
  needsReview: Set<LibraryLens>;
  onSelect: (lens: LibraryLens) => void;
}) {
  const { t } = usePresentation();
  return (
    <nav className="library-lens-rail" aria-label={t("library.home.filterLabel")}>
      {libraryLenses.map((candidate) => {
        const count = counts.get(candidate) ?? 0;
        if (candidate !== "all" && count === 0) return null;
        const Icon = lensIcons[candidate];
        return (
          <button
            className={lens === candidate ? "library-lens active" : "library-lens"}
            data-library-kind={candidate}
            type="button"
            aria-pressed={lens === candidate}
            key={candidate}
            onClick={() => onSelect(candidate)}
          >
            <Icon aria-hidden="true" />
            <span className="library-lens-name">{t(lensLabels[candidate])}</span>
            <span className="library-lens-count">{count}</span>
            {needsReview.has(candidate) ? (
              <span className="library-lens-flag" title={t("library.lens.needsReview")} />
            ) : null}
          </button>
        );
      })}
    </nav>
  );
}
