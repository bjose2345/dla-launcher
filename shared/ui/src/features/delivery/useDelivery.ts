import { useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { newestView } from "../../app/revisionedView";
import type { DeliveryGateway, DeliveryView } from "./types";

export const DELIVERY_QUERY_KEY = ["delivery"] as const;

/** Stores a view from a command or event unless a newer one is already held. */
export function acceptDeliveryView(queryClient: QueryClient, next: DeliveryView) {
  queryClient.setQueryData<DeliveryView>(DELIVERY_QUERY_KEY, (current) => newestView(current, next));
}

/** The downloads view, kept current by the gateway's change and progress events. */
export function useDelivery(gateway: DeliveryGateway | undefined) {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: DELIVERY_QUERY_KEY,
    queryFn: async () => newestView(queryClient.getQueryData(DELIVERY_QUERY_KEY), await gateway!.read()),
    enabled: gateway !== undefined,
  });
  useEffect(() => {
    if (!gateway) return undefined;
    return gateway.subscribe({
      onChange: (view) => {
        const previous = queryClient.getQueryData<DeliveryView>(DELIVERY_QUERY_KEY);
        acceptDeliveryView(queryClient, view);
        const finished = view.jobs.some((job) => (
          (job.status === "installed" || job.status === "needs_review")
            && previous?.jobs.find((before) => before.id === job.id)?.status !== job.status
        ));
        // A finished install changes what the work and the Library show.
        if (finished) void queryClient.invalidateQueries({ queryKey: ["library"] });
      },
      onProgress: (progress) => {
        queryClient.setQueryData<DeliveryView>(DELIVERY_QUERY_KEY, (current) => (
          current ? { ...current, progress } : current
        ));
      },
    });
  }, [gateway, queryClient]);
  return query;
}
