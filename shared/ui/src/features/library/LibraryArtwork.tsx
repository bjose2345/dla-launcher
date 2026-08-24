import {
  AppWindow,
  BookOpen,
  Film,
  Headphones,
  Images,
  Library,
} from "lucide-react";

import { WorkCardImage } from "../catalog/WorkCardImage";
import type { CatalogWork } from "../catalog";
import type { LibraryContentKind } from "./libraryHome";

export function LibraryArtwork({
  kind,
  title,
  work,
}: {
  kind: LibraryContentKind;
  title: string;
  work?: CatalogWork;
}) {
  if (work) {
    return (
      <span className="library-artwork" data-library-kind={kind}>
        <WorkCardImage
          code={work.code}
          title={title}
          mainImageUrls={work.mainImageUrls}
          thumbnailUrls={work.thumbnailUrls}
        />
      </span>
    );
  }

  const Icon = kind === "audio"
    ? Headphones
    : kind === "images"
      ? Images
      : kind === "video"
        ? Film
        : kind === "documents"
          ? BookOpen
          : kind === "apps"
            ? AppWindow
            : Library;

  return (
    <span className="library-artwork library-artwork-fallback" data-library-kind={kind}>
      <Icon aria-hidden="true" />
      <strong>{title.slice(0, 2).toLocaleUpperCase()}</strong>
    </span>
  );
}
