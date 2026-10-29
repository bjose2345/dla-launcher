import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useQueryClient } from "@tanstack/react-query";

import type { CatalogDetailGateway, CatalogWork } from "../catalog";
import { libraryDisplayCode, libraryDisplayCreator, libraryDisplayTitle } from "./libraryHome";
import { effectiveIdentity } from "./libraryPresentation";
import { orderedSessionItems } from "./mediaSession";
import { isMangaWork } from "./readerModel";
import type { Installation, LibraryGateway, MediaSession, MediaSessionItem, OwnWorkDetails } from "./types";

export type ReaderGateway = Pick<
  LibraryGateway,
  | "openMediaSession"
  | "closeMediaSession"
  | "updateMediaProgress"
  | "readInstallation"
  | "readOwnWorkDetails"
  | "mediaAssetUrl"
  | "mediaThumbnailUrl"
>;
export type ReaderCatalog = Pick<CatalogDetailGateway, "readWorks">;

// Page turns come quickly; the place is written once they settle.
const SAVE_DELAY_MS = 600;

export function isReaderAction(action: MediaSession["action"]): boolean {
  return action === "read_images" || action === "open_document";
}

/**
 * A place in a reading: the open file, and the page counted from 0 (an
 * image's place in the set, or the page of a PDF). It is stored as a media
 * position of `page + 1` out of the page count, so the Library's progress
 * bars read it like any other progress.
 */
export interface ReaderPosition {
  ordinal: number;
  page: number;
}

interface ReaderValue {
  session: MediaSession | null;
  items: MediaSessionItem[];
  workCode: string | null;
  /** The title and circle the Library shows for this work. */
  title: (preferEnglish: boolean) => string;
  creator: (preferEnglish: boolean) => string;
  manga: boolean;
  /** Where the reading opened. */
  start: ReaderPosition;
  /** The reading picked up a saved place, so the viewer says where it continues. */
  resumed: boolean;
  finished: boolean;
  saveError: string;
  openError: string;
  closing: boolean;
  open: (installationId: string) => Promise<MediaSession | null>;
  move: (position: ReaderPosition, pageCount: number) => void;
  finish: (position: ReaderPosition, pageCount: number) => void;
  close: () => Promise<void>;
  /** Moves the open reading to another viewer at the given place. */
  handOver: (position: ReaderPosition) => void;
  dismissOpenError: () => void;
}

interface PendingSave {
  session: MediaSession;
  position: ReaderPosition;
  pageCount: number;
  finished: boolean;
}

const ReaderContext = createContext<ReaderValue | null>(null);

export function ReaderProvider({
  gateway,
  catalog,
  children,
}: {
  gateway: ReaderGateway;
  catalog?: ReaderCatalog;
  children: ReactNode;
}) {
  const queryClient = useQueryClient();
  const [session, setSession] = useState<MediaSession | null>(null);
  const [installation, setInstallation] = useState<Installation | null>(null);
  const [work, setWork] = useState<CatalogWork | undefined>(undefined);
  const [own, setOwn] = useState<OwnWorkDetails | undefined>(undefined);
  const [workCode, setWorkCode] = useState<string | null>(null);
  const [manga, setManga] = useState(false);
  const [start, setStart] = useState<ReaderPosition>({ ordinal: 0, page: 0 });
  const [resumed, setResumed] = useState(false);
  const [finished, setFinished] = useState(false);
  const [saveError, setSaveError] = useState("");
  const [openError, setOpenError] = useState("");
  const [closing, setClosing] = useState(false);
  const writeRef = useRef<Promise<void>>(Promise.resolve());
  const pendingRef = useRef<PendingSave | null>(null);
  const timerRef = useRef<number | null>(null);

  const items = useMemo(
    () => (session ? orderedSessionItems(session, false) : []),
    [session],
  );

  const write = useCallback((save: PendingSave, status: "active" | "paused") => {
    writeRef.current = writeRef.current.then(async () => {
      try {
        // A finished reading stays completed; turning back still saves the place.
        await gateway.updateMediaProgress({
          sessionId: save.session.id,
          itemOrdinal: save.position.ordinal,
          positionMs: save.position.page + 1,
          durationMs: save.pageCount,
          completed: save.finished,
          status: save.finished ? "completed" : status,
        });
        setSaveError("");
      } catch (cause) {
        setSaveError(errorText(cause));
      }
    });
    return writeRef.current;
  }, [gateway]);

  const flush = useCallback((status: "active" | "paused") => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    const pending = pendingRef.current;
    pendingRef.current = null;
    return pending ? write(pending, status) : writeRef.current;
  }, [write]);

  const schedule = useCallback((save: PendingSave) => {
    pendingRef.current = save;
    if (timerRef.current !== null) window.clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(() => void flush("active"), SAVE_DELAY_MS);
  }, [flush]);

  const open = useCallback(async (installationId: string) => {
    try {
      setSaveError("");
      setOpenError("");
      const opened = await gateway.openMediaSession(installationId);
      if (!isReaderAction(opened.action)) return null;
      const installation = await gateway.readInstallation(installationId).catch(() => null);
      const own = await gateway.readOwnWorkDetails(installationId).catch(() => null) ?? undefined;
      const catalogCode = installation ? effectiveIdentity(installation) : null;
      const work = catalogCode && catalog
        ? (await catalog.readWorks([catalogCode]).catch(() => []))[0]
        : undefined;
      const code = installation ? libraryDisplayCode(installation, own) : null;
      const ordered = orderedSessionItems(opened, false);
      const { itemOrdinal, positionMs, durationMs } = opened.progress;
      const page = opened.action === "read_images"
        ? Math.max(0, ordered.findIndex((item) => item.ordinal === itemOrdinal))
        : durationMs ? Math.max(0, positionMs - 1) : 0;
      setSession(opened);
      setInstallation(installation);
      setWork(work);
      setOwn(own);
      setWorkCode(code);
      setManga(work ? isMangaWork(work.categories) : own?.kind === "manga");
      setStart({ ordinal: itemOrdinal, page });
      setResumed(true);
      setFinished(opened.progress.completed);
      setClosing(false);
      return opened;
    } catch (cause) {
      setOpenError(errorText(cause));
      return null;
    }
  }, [catalog, gateway]);

  const move = useCallback((position: ReaderPosition, pageCount: number) => {
    if (!session) return;
    schedule({ session, position, pageCount, finished });
  }, [finished, schedule, session]);

  const finish = useCallback((position: ReaderPosition, pageCount: number) => {
    if (!session) return;
    setFinished(true);
    pendingRef.current = { session, position, pageCount, finished: true };
    void flush("active");
  }, [flush, session]);

  const close = useCallback(async () => {
    const current = session;
    if (!current || closing) return;
    setClosing(true);
    try {
      await flush("paused");
      await gateway.closeMediaSession(current.id);
      setSession(null);
      setFinished(false);
      void queryClient.invalidateQueries({ queryKey: ["library", "shelves"] });
    } catch (cause) {
      setSaveError(errorText(cause));
    } finally {
      setClosing(false);
    }
  }, [closing, flush, gateway, queryClient, session]);

  const handOver = useCallback((position: ReaderPosition) => {
    setStart(position);
    setResumed(false);
  }, []);
  const dismissOpenError = useCallback(() => setOpenError(""), []);
  const title = useCallback((preferEnglish: boolean) => (
    installation ? libraryDisplayTitle(installation, work, preferEnglish) : session?.installationId ?? ""
  ), [installation, session?.installationId, work]);
  const creator = useCallback((preferEnglish: boolean) => (
    libraryDisplayCreator(work, preferEnglish, own)
  ), [own, work]);

  const value = useMemo<ReaderValue>(() => ({
    session,
    items,
    workCode,
    title,
    creator,
    manga,
    start,
    resumed,
    finished,
    saveError,
    openError,
    closing,
    open,
    move,
    finish,
    close,
    handOver,
    dismissOpenError,
  }), [
    close, closing, creator, dismissOpenError, finish, finished, handOver, items, manga, move,
    open, openError, resumed, saveError, session, start, title, workCode,
  ]);

  return <ReaderContext.Provider value={value}>{children}</ReaderContext.Provider>;
}

export function useReader(): ReaderValue {
  const context = useContext(ReaderContext);
  if (!context) throw new Error("useReader must be used within ReaderProvider");
  return context;
}

function errorText(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
