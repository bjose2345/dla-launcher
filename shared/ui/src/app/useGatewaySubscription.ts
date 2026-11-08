import { useEffect, useRef, type DependencyList } from "react";

export type GatewaySubscribe<T> = (listener: (value: T) => void) => Promise<() => void>;

/**
 * Listens through a gateway subscription while the component is mounted and
 * `subscribe` is given. Subscribing is asynchronous, so a subscription that
 * completes after unmount is released at once, and values that arrive after
 * unmount are dropped. The latest `listener` sees every value without
 * subscribing again; `deps` say when to subscribe again, as for an effect.
 */
export function useGatewaySubscription<T>(
  subscribe: GatewaySubscribe<T> | null | undefined,
  listener: (value: T) => void,
  deps: DependencyList,
): void {
  const subscribeRef = useRef(subscribe);
  const listenerRef = useRef(listener);
  subscribeRef.current = subscribe;
  listenerRef.current = listener;
  const enabled = Boolean(subscribe);

  useEffect(() => {
    const start = subscribeRef.current;
    if (!start) return undefined;
    let unsubscribe: (() => void) | undefined;
    let disposed = false;
    void start((value) => {
      if (!disposed) listenerRef.current(value);
    }).then((release) => {
      if (disposed) release();
      else unsubscribe = release;
    });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [enabled, ...deps]);
}
