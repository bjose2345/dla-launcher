import { ArrowLeft, CheckCircle2, ChevronLeft, ChevronRight, LoaderCircle } from "lucide-react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { mediaItemName } from "./mediaSession";
import type { LibraryGateway, MediaSession, MediaSessionItem } from "./types";

export function DocumentReader({
  gateway,
  session,
  installationName,
  items,
  currentOrdinal,
  completed,
  saveError,
  closing,
  onChoose,
  onComplete,
  onBack,
}: {
  gateway: Pick<LibraryGateway, "mediaAssetUrl">;
  session: MediaSession;
  installationName: string;
  items: MediaSessionItem[];
  currentOrdinal: number;
  completed: boolean;
  saveError: string;
  closing: boolean;
  onChoose: (ordinal: number) => void;
  onComplete: () => void;
  onBack: () => void;
}) {
  const { t } = usePresentation();
  const index = Math.max(0, items.findIndex((item) => item.ordinal === currentOrdinal));
  const current = items[index];
  const previous = items[index - 1];
  const next = items[index + 1];

  return (
    <main className="document-reader">
      <header className="document-reader-top">
        <button className="document-reader-back" type="button" disabled={closing} onClick={onBack}>
          {closing ? <LoaderCircle className="library-spin" aria-hidden="true" /> : <ArrowLeft aria-hidden="true" />}
          {t("library.back")}
        </button>
        <div className="document-reader-title">
          <strong>{installationName}</strong>
          <small>{current ? mediaItemName(current) : ""}</small>
        </div>
        <div className="document-reader-actions">
          {items.length > 1 ? (
            <span className="document-reader-count">
              {t("media.trackPosition", { position: index + 1, total: items.length })}
            </span>
          ) : null}
          <button
            type="button"
            disabled={!previous}
            aria-label={t("media.previous")}
            onClick={() => previous && onChoose(previous.ordinal)}
          >
            <ChevronLeft aria-hidden="true" />
          </button>
          <button
            type="button"
            disabled={!next}
            aria-label={t("media.next")}
            onClick={() => next && onChoose(next.ordinal)}
          >
            <ChevronRight aria-hidden="true" />
          </button>
          <button
            className="document-reader-finish"
            type="button"
            disabled={completed}
            onClick={onComplete}
          >
            <CheckCircle2 aria-hidden="true" />
            {t(completed ? "media.completed" : "media.markFinished")}
          </button>
        </div>
      </header>

      {saveError ? (
        <p className="document-reader-error" role="alert">
          {t("common.requestFailed", { error: saveError })}
        </p>
      ) : null}

      {current ? (
        <iframe
          className="document-reader-surface"
          key={current.ordinal}
          src={gateway.mediaAssetUrl(session.id, current.ordinal)}
          title={mediaItemName(current)}
        />
      ) : (
        <p className="document-reader-empty">{t("media.noItems")}</p>
      )}
    </main>
  );
}
