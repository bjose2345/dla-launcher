import { useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";

import { useGatewaySubscription } from "../../app/useGatewaySubscription";

import { newestView } from "../../app/revisionedView";
import {
  scanInstallIsActive,
  scanInstallsKey,
  type ScanInstallGateway,
  type ScanInstallView,
} from "./types";

/** Stores a view from a command or event unless a newer one is already held. */
export function acceptScanInstallView(queryClient: QueryClient, next: ScanInstallView) {
  queryClient.setQueryData<ScanInstallView>(scanInstallsKey, (current) => newestView(current, next));
}

/** Archives the scanner is installing, kept current by the gateway's events. */
export function useScanInstalls(gateway: ScanInstallGateway | undefined) {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: scanInstallsKey,
    queryFn: async () => newestView(
      queryClient.getQueryData(scanInstallsKey),
      await gateway!.readInstalls(),
    ),
    enabled: gateway !== undefined,
  });
  useGatewaySubscription(
    gateway?.subscribeInstalls.bind(gateway),
    (view) => {
      const previous = queryClient.getQueryData<ScanInstallView>(scanInstallsKey);
      acceptScanInstallView(queryClient, view);
      const settled = view.installs.some((install) => (
        !scanInstallIsActive(install.status)
          && previous?.installs.find((before) => before.installationId === install.installationId)?.status
            !== install.status
      ));
      // A finished install changes what the Library and the scan show.
      if (settled) {
        void queryClient.invalidateQueries({ queryKey: ["library"] });
        void queryClient.invalidateQueries({ queryKey: ["scanner", "findings"] });
      }
    },
    [gateway],
  );
  return query;
}
