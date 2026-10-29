import {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type RefObject,
} from "react";

import { AlertTriangle, X } from "lucide-react";

import { useDocumentScrollLock } from "../../app/useDocumentScrollLock";
import { useBoundKeys } from "../../preferences/KeyBindingsProvider";
import { usePresentation } from "../../preferences/PresentationProvider";
import { readImageViewerPreferences, type ImageSetViewer } from "./imageViewerModel";
import { useReader, type ReaderPosition } from "./ReaderProvider";
import type { LibraryGateway } from "./types";

// The overlay is mounted above the router from startup; a viewer loads the
// first time a session needs it.
const Reader = lazy(() => import("./Reader").then((module) => ({ default: module.Reader })));
const ImageViewer = lazy(() => import("./ImageViewer").then((module) => ({ default: module.ImageViewer })));

export function ReaderOverlay({ gateway }: { gateway: LibraryGateway }) {
  const reader = useReader();
  const { locale, t } = usePresentation();
  const overlayRef = useRef<HTMLDivElement>(null);
  const [moved, setMoved] = useState<{ sessionId: string; viewer: ImageSetViewer } | null>(null);
  useDocumentScrollLock(Boolean(reader.session));
  const readerHandlers = useMemo(() => ({
    readerClose: () => void reader.close(),
  }), [reader.close]);
  useBoundKeys("reader", readerHandlers, {
    enabled: Boolean(reader.session),
    ignoreInteractive: false,
  });

  useEffect(() => {
    if (!reader.session) return;
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const frame = requestAnimationFrame(() => focusReader(overlayRef.current));
    return () => {
      cancelAnimationFrame(frame);
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  }, [reader.session]);

  const { handOver } = reader;
  const sessionId = reader.session?.id;
  const openIn = useCallback((viewer: ImageSetViewer, position: ReaderPosition) => {
    if (!sessionId) return;
    handOver(position);
    setMoved({ sessionId, viewer });
    requestAnimationFrame(() => focusReader(overlayRef.current));
  }, [handOver, sessionId]);

  if (!reader.session) {
    return reader.openError ? (
      <div className="reader-open-error" role="alert">
        <AlertTriangle aria-hidden="true" />
        <div>
          <strong>{t("reader.openFailed")}</strong>
          <span>{t("common.technicalDetail", { detail: reader.openError })}</span>
        </div>
        <button className="reader-icon-button is-plain" type="button" aria-label={t("notice.close")} onClick={reader.dismissOpenError}>
          <X aria-hidden="true" />
        </button>
      </div>
    ) : null;
  }

  const images = reader.session.action === "read_images";
  const readerLabel = images ? t("media.player.images") : t("media.player.document");
  const viewer = !images
    ? "reader"
    : moved?.sessionId === reader.session.id
      ? moved.viewer
      : readImageViewerPreferences(reader.session.installationId).openIn;

  return (
    <div
      className="reader-overlay"
      ref={overlayRef}
      role="dialog"
      aria-modal="true"
      aria-label={`${readerLabel}: ${reader.title(locale !== "ja-JP")}`}
      tabIndex={-1}
      onKeyDown={trapFocus}
    >
      <Suspense fallback={null}>
        {viewer === "viewer" ? (
          <ImageViewer key="viewer" gateway={gateway} onOpenIn={openIn} />
        ) : (
          <Reader key="reader" gateway={gateway} onOpenIn={images ? openIn : undefined} />
        )}
        <FocusLoadedReader overlay={overlayRef} />
      </Suspense>
    </div>
  );
}

// When the reader chunk arrives after the overlay took focus, move focus on
// to the reader as an already loaded reader would have.
function FocusLoadedReader({ overlay }: { overlay: RefObject<HTMLDivElement | null> }) {
  useEffect(() => {
    const container = overlay.current;
    if (!container || document.activeElement !== container) return;
    focusReader(container);
  }, [overlay]);
  return null;
}

// The reader itself takes focus rather than its first button, so the page
// keys work at once and Space turns the page instead of pressing Back.
function focusReader(overlay: HTMLElement | null) {
  if (!overlay) return;
  (overlay.querySelector<HTMLElement>(".reader") ?? overlay).focus();
}

const focusableSelector = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

function focusableElements(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>(focusableSelector)]
    .filter((element) => !element.hidden && element.tabIndex >= 0 && element.getAttribute("aria-hidden") !== "true");
}

function trapFocus(event: ReactKeyboardEvent<HTMLDivElement>) {
  if (event.key !== "Tab") return;
  const focusable = focusableElements(event.currentTarget);
  if (!focusable.length) {
    event.preventDefault();
    event.currentTarget.focus();
    return;
  }
  const first = focusable[0]!;
  const last = focusable[focusable.length - 1]!;
  if (event.shiftKey && (document.activeElement === first || !event.currentTarget.contains(document.activeElement))) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && (document.activeElement === last || !event.currentTarget.contains(document.activeElement))) {
    event.preventDefault();
    first.focus();
  }
}
