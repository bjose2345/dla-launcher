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

import { installationTitle } from "./libraryPresentation";
import { orderedSessionItems } from "./mediaSession";
import type { LibraryGateway, MediaSession, MediaSessionItem } from "./types";

export type ImageReaderGateway = Pick<
  LibraryGateway,
  "openMediaSession" | "closeMediaSession" | "updateMediaProgress" | "readInstallation" | "mediaAssetUrl"
>;

export function isReaderAction(action: MediaSession["action"]): boolean {
  return action === "read_images" || action === "open_document";
}

interface ImageReaderValue {
  session: MediaSession | null;
  items: MediaSessionItem[];
  installationName: string;
  currentOrdinal: number;
  completed: boolean;
  saveError: string;
  closing: boolean;
  open: (installationId: string) => Promise<MediaSession | null>;
  choose: (ordinal: number) => void;
  complete: () => void;
  close: () => Promise<void>;
}

const ImageReaderContext = createContext<ImageReaderValue | null>(null);

export function ImageReaderProvider({
  gateway,
  children,
}: {
  gateway: ImageReaderGateway;
  children: ReactNode;
}) {
  const queryClient = useQueryClient();
  const [session, setSession] = useState<MediaSession | null>(null);
  const [installationName, setInstallationName] = useState("");
  const [currentOrdinal, setCurrentOrdinal] = useState(0);
  const [completed, setCompleted] = useState(false);
  const [saveError, setSaveError] = useState("");
  const [closing, setClosing] = useState(false);
  const writeRef = useRef<Promise<void>>(Promise.resolve());

  const items = useMemo(
    () => (session ? orderedSessionItems(session, false) : []),
    [session],
  );

  const persist = useCallback((
    current: MediaSession,
    ordinal: number,
    finished: boolean,
    status: "active" | "paused" | "completed",
  ) => {
    writeRef.current = writeRef.current.then(async () => {
      try {
        await gateway.updateMediaProgress({
          sessionId: current.id,
          itemOrdinal: ordinal,
          positionMs: 0,
          durationMs: null,
          completed: finished,
          status,
        });
        setSaveError("");
      } catch (cause) {
        setSaveError(cause instanceof Error ? cause.message : String(cause));
      }
    });
    return writeRef.current;
  }, [gateway]);

  const open = useCallback(async (installationId: string) => {
    try {
      setSaveError("");
      const opened = await gateway.openMediaSession(installationId);
      if (!isReaderAction(opened.action)) return null;
      const installation = await gateway.readInstallation(installationId).catch(() => null);
      setSession(opened);
      setInstallationName(installation ? installationTitle(installation) : installationId);
      setCurrentOrdinal(opened.progress.itemOrdinal);
      setCompleted(opened.progress.completed);
      setClosing(false);
      return opened;
    } catch (cause) {
      setSaveError(cause instanceof Error ? cause.message : String(cause));
      return null;
    }
  }, [gateway]);

  const choose = useCallback((ordinal: number) => {
    if (!session || ordinal === currentOrdinal) return;
    setCurrentOrdinal(ordinal);
    setCompleted(false);
    void persist(session, ordinal, false, "active");
  }, [currentOrdinal, persist, session]);

  const complete = useCallback(() => {
    if (!session) return;
    setCompleted(true);
    void persist(session, currentOrdinal, true, "completed");
  }, [currentOrdinal, persist, session]);

  const close = useCallback(async () => {
    const current = session;
    if (!current || closing) return;
    setClosing(true);
    try {
      if (!completed) await persist(current, currentOrdinal, false, "paused");
      await gateway.closeMediaSession(current.id);
      setSession(null);
      setCurrentOrdinal(0);
      setCompleted(false);
      void queryClient.invalidateQueries({ queryKey: ["library", "shelves"] });
    } catch (cause) {
      setSaveError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setClosing(false);
    }
  }, [closing, completed, currentOrdinal, gateway, persist, queryClient, session]);

  const value = useMemo<ImageReaderValue>(() => ({
    session,
    items,
    installationName,
    currentOrdinal,
    completed,
    saveError,
    closing,
    open,
    choose,
    complete,
    close,
  }), [
    choose, close, closing, complete, completed, currentOrdinal, installationName,
    items, open, saveError, session,
  ]);

  return <ImageReaderContext.Provider value={value}>{children}</ImageReaderContext.Provider>;
}

export function useImageReader(): ImageReaderValue {
  const context = useContext(ImageReaderContext);
  if (!context) throw new Error("useImageReader must be used within ImageReaderProvider");
  return context;
}
