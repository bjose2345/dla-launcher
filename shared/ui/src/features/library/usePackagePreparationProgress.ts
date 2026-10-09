import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import {
  mergePackagePreparationProgress,
  packagePreparationIsTerminal,
  type LibraryGateway,
  type PackagePreparationProgress,
} from "./types";

export const packagePreparationProgressKey = ["library", "package-preparation", "progress"] as const;

export type PackagePreparationProgressGateway = Pick<
  LibraryGateway,
  "readPackagePreparationProgress" | "subscribePackagePreparationProgress"
>;

/** The newest unpacking progress of any work, from events and a light poll. */
export function usePackagePreparationProgress(gateway: PackagePreparationProgressGateway | undefined) {
  const queryClient = useQueryClient();
  const progress = useQuery({
    queryKey: packagePreparationProgressKey,
    queryFn: async () => mergePackagePreparationProgress(
      queryClient.getQueryData<PackagePreparationProgress | null>(packagePreparationProgressKey),
      await gateway!.readPackagePreparationProgress(),
    ),
    enabled: gateway !== undefined,
    refetchInterval: (query) => {
      const value = query.state.data;
      return value && !packagePreparationIsTerminal(value.stage) ? 650 : false;
    },
  });

  useEffect(() => {
    if (!gateway) return undefined;
    let unsubscribe: (() => void) | undefined;
    let disposed = false;
    void gateway.subscribePackagePreparationProgress((value) => {
      queryClient.setQueryData<PackagePreparationProgress | null>(
        packagePreparationProgressKey,
        (current) => mergePackagePreparationProgress(current, value),
      );
    }).then((listener) => {
      if (disposed) listener();
      else unsubscribe = listener;
    });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [gateway, queryClient]);

  return progress;
}

export function packagePreparationPercent(progress: PackagePreparationProgress): number | null {
  if (progress.stage === "completed") return 100;
  const { processedBytes, totalBytes, processedFiles, totalFiles } = progress.counters;
  const bytes = totalBytes > 0 ? processedBytes / totalBytes : 0;
  const files = totalFiles > 0 ? processedFiles / totalFiles : 0;
  const fraction = Math.max(bytes, files);
  return fraction > 0 ? Math.min(100, Math.round(fraction * 100)) : null;
}
