import { useEffect, useMemo, useRef, type KeyboardEvent as ReactKeyboardEvent } from "react";

import { useDocumentScrollLock } from "../../app/useDocumentScrollLock";
import { useBoundKeys } from "../../preferences/KeyBindingsProvider";
import { usePresentation } from "../../preferences/PresentationProvider";
import { DocumentReader } from "./DocumentReader";
import { ImageReader } from "./ImageReader";
import { useImageReader } from "./ImageReaderProvider";
import type { LibraryGateway } from "./types";

export function ReaderOverlay({ gateway }: { gateway: LibraryGateway }) {
  const reader = useImageReader();
  const { t } = usePresentation();
  const overlayRef = useRef<HTMLDivElement>(null);
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
    const frame = requestAnimationFrame(() => {
      const overlay = overlayRef.current;
      if (!overlay) return;
      (focusableElements(overlay)[0] ?? overlay).focus();
    });
    return () => {
      cancelAnimationFrame(frame);
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  }, [reader.session]);

  if (!reader.session) return null;

  const content = reader.session.action === "open_document"
    ? (
        <DocumentReader
          gateway={gateway}
          session={reader.session}
          installationName={reader.installationName}
          items={reader.items}
          currentOrdinal={reader.currentOrdinal}
          completed={reader.completed}
          saveError={reader.saveError}
          closing={reader.closing}
          onChoose={reader.choose}
          onComplete={reader.complete}
          onBack={() => void reader.close()}
        />
      )
    : (
        <ImageReader
          gateway={gateway}
          session={reader.session}
          installationName={reader.installationName}
          items={reader.items}
          currentOrdinal={reader.currentOrdinal}
          completed={reader.completed}
          saveError={reader.saveError}
          closing={reader.closing}
          onChoose={reader.choose}
          onComplete={reader.complete}
          onBack={() => void reader.close()}
        />
      );
  const readerLabel = reader.session.action === "open_document"
    ? t("media.player.document")
    : t("media.player.images");

  return (
    <div
      className="reader-overlay"
      ref={overlayRef}
      role="dialog"
      aria-modal="true"
      aria-label={`${readerLabel}: ${reader.installationName}`}
      tabIndex={-1}
      onKeyDown={trapFocus}
    >
      {content}
    </div>
  );
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
    .filter((element) => !element.hidden && element.getAttribute("aria-hidden") !== "true");
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
