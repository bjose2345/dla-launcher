import { useQuery } from "@tanstack/react-query";
import { createContext, useContext, useMemo, type ReactNode } from "react";

import type { LibraryGateway, OwnWorkDetails } from "./types";

export const OWN_WORK_DETAILS_KEY = ["library", "own-work-details"] as const;

export type OwnWorkGateway = Pick<LibraryGateway, "listOwnWorkDetails" | "ownWorkArtworkUrl">;

interface OwnWorkValue {
  details: (installationId: string) => OwnWorkDetails | undefined;
  coverUrl: (installationId: string) => string | null;
  /** Opens the work page of a work outside the catalog. */
  openPage?: (installationId: string) => void;
}

const OwnWorkContext = createContext<OwnWorkValue>({
  details: () => undefined,
  coverUrl: () => null,
});

/**
 * The details the user gave works the catalog doesn't know, for every Library
 * view to show where a catalog work shows the catalog's.
 */
export function OwnWorkDetailsProvider({
  gateway,
  onOpenPage,
  children,
}: {
  gateway: OwnWorkGateway;
  onOpenPage?: (installationId: string) => void;
  children: ReactNode;
}) {
  const query = useQuery({ queryKey: OWN_WORK_DETAILS_KEY, queryFn: () => gateway.listOwnWorkDetails() });
  const value = useMemo<OwnWorkValue>(() => {
    const byInstallation = new Map((query.data ?? []).map((details) => [details.installationId, details]));
    return {
      details: (installationId) => byInstallation.get(installationId),
      coverUrl: (installationId) => {
        const cover = byInstallation.get(installationId)?.cover;
        return cover ? gateway.ownWorkArtworkUrl(installationId, cover) : null;
      },
      openPage: onOpenPage,
    };
  }, [gateway, onOpenPage, query.data]);
  return <OwnWorkContext.Provider value={value}>{children}</OwnWorkContext.Provider>;
}

export function useOwnWork(): OwnWorkValue {
  return useContext(OwnWorkContext);
}
