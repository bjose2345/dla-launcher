import { useEffect, useRef, type CSSProperties } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import type { GallerySize, ImageFolder } from "./imageViewerModel";
import { mediaItemName } from "./mediaSession";
import type { MediaSessionItem } from "./types";

const TILE_WIDTHS: Record<GallerySize, string> = {
  small: "140px",
  medium: "200px",
  large: "290px",
};

/** Every image of the set as a thumbnail, by folder, with the last one viewed marked. */
export function ImageGallery({
  items,
  folders,
  scope,
  current,
  size,
  folderName,
  thumbnailUrl,
  onScope,
  onOpen,
}: {
  items: readonly MediaSessionItem[];
  folders: readonly ImageFolder[];
  scope: string | null;
  current: number;
  size: GallerySize;
  folderName: (folder: ImageFolder | undefined) => string;
  thumbnailUrl: (ordinal: number) => string;
  onScope: (key: string | null) => void;
  onOpen: (index: number) => void;
}) {
  const { t } = usePresentation();
  const currentRef = useRef<HTMLButtonElement>(null);
  const shown = scope === null ? folders : folders.filter((folder) => folder.key === scope);
  const currentItem = items[current];
  const currentFolder = folders.find((folder) => folder.indexes.includes(current));
  const grid = { "--image-tile-width": TILE_WIDTHS[size] } as CSSProperties;

  useEffect(() => {
    const tile = currentRef.current;
    tile?.scrollIntoView?.({ block: "center" });
    tile?.focus({ preventScroll: true });
  }, []);

  return (
    <div className="image-gallery">
      <nav className="image-gallery-side" aria-label={t("viewer.folders")}>
        {folders.length > 1 ? (
          <>
            <span className="image-gallery-side-title">{t("viewer.folders")}</span>
            <FolderChoices folders={folders} scope={scope} count={items.length} folderName={folderName} onChoose={onScope} />
          </>
        ) : null}
        {currentItem ? (
          <div className="image-gallery-last">
            <span className="image-gallery-side-title">{t("viewer.lastViewed")}</span>
            <img src={thumbnailUrl(currentItem.ordinal)} alt="" decoding="async" draggable={false} />
            <span>
              {t("viewer.imageNumber", { number: current + 1 })}
              {folders.length > 1 ? ` · ${folderName(currentFolder)}` : ""}
            </span>
            <button className="reader-chip-button is-primary" type="button" onClick={() => onOpen(current)}>
              {t("viewer.continue")}
            </button>
          </div>
        ) : null}
      </nav>
      <div className="image-gallery-scroll">
        {shown.map((folder) => (
          <section className="image-gallery-section" key={folder.key} aria-label={folderName(folder)}>
            {folders.length > 1 ? (
              <h2>
                {folderName(folder)}
                <small>
                  {folder.indexes.length === 1
                    ? t("viewer.imageCountOne")
                    : t("viewer.imageCount", { count: folder.indexes.length })}
                </small>
              </h2>
            ) : null}
            <div className="image-gallery-grid" style={grid}>
              {folder.indexes.map((index, place) => {
                const item = items[index]!;
                const number = scope === null ? index + 1 : place + 1;
                return (
                  <button
                    className={`image-gallery-tile${index === current ? " is-current" : ""}`}
                    type="button"
                    key={item.ordinal}
                    ref={index === current ? currentRef : undefined}
                    aria-label={t("viewer.imageLabel", { number, name: mediaItemName(item) })}
                    aria-current={index === current ? "true" : undefined}
                    onClick={() => onOpen(index)}
                  >
                    <img
                      src={thumbnailUrl(item.ordinal)}
                      alt=""
                      loading="lazy"
                      decoding="async"
                      draggable={false}
                      onError={(event) => { event.currentTarget.hidden = true; }}
                    />
                    <small>{number}</small>
                    {index === current ? <span className="image-gallery-badge">{t("viewer.lastViewed")}</span> : null}
                  </button>
                );
              })}
            </div>
          </section>
        ))}
      </div>
    </div>
  );
}

export function FolderChoices({
  folders,
  scope,
  count,
  folderName,
  onChoose,
}: {
  folders: readonly ImageFolder[];
  scope: string | null;
  count: number;
  folderName: (folder: ImageFolder) => string;
  onChoose: (key: string | null) => void;
}) {
  const { t } = usePresentation();
  const choices: Array<{ key: string | null; name: string; count: number }> = [
    { key: null, name: t("viewer.allImages"), count },
    ...folders.map((folder) => ({ key: folder.key, name: folderName(folder), count: folder.indexes.length })),
  ];
  return (
    <div className="image-folder-choices">
      {choices.map((choice) => (
        <button
          className={`image-folder-choice${scope === choice.key ? " is-active" : ""}`}
          type="button"
          key={choice.key ?? ""}
          aria-pressed={scope === choice.key}
          onClick={() => onChoose(choice.key)}
        >
          <span title={choice.name}>{choice.name}</span>
          <small>{choice.count}</small>
        </button>
      ))}
    </div>
  );
}
