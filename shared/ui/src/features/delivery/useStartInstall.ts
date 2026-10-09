import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { acceptDeliveryView } from "./useDelivery";
import { deliveryFailure, type DeliveryFailure, type DeliveryGateway, type DeliveryView } from "./types";

/**
 * Install from DLA as one action: sign in when needed (the caller shows the
 * dialog while `signingIn` is set), choose the install folder the first
 * time, then queue the work.
 */
export function useStartInstall(gateway: DeliveryGateway, view: DeliveryView | undefined) {
  const queryClient = useQueryClient();
  const [signingIn, setSigningIn] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<DeliveryFailure | null>(null);

  const accept = (next: DeliveryView) => acceptDeliveryView(queryClient, next);

  const queue = async (workCode: string, current: DeliveryView) => {
    setPending(true);
    setFailure(null);
    try {
      let ready = current;
      if (!ready.installFolder) {
        const chosen = await gateway.chooseInstallFolder();
        if (!chosen) return;
        accept(chosen);
        ready = chosen;
      }
      accept(await gateway.install(workCode));
    } catch (error) {
      setFailure(deliveryFailure(error));
    } finally {
      setPending(false);
    }
  };

  const start = (workCode: string) => {
    if (!view) return;
    if (!view.account) {
      setSigningIn(workCode);
      return;
    }
    void queue(workCode, view);
  };

  const signedIn = (next: DeliveryView) => {
    accept(next);
    const workCode = signingIn;
    setSigningIn(null);
    if (workCode) void queue(workCode, next);
  };

  return {
    start,
    pending,
    failure,
    signingIn: signingIn !== null,
    signedIn,
    cancelSignIn: () => setSigningIn(null),
  };
}
