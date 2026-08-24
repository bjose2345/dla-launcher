import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import {
  assignBinding,
  clearBinding,
  eventCombo,
  isInteractiveTarget,
  matchBinding,
  readKeyBindingOverrides,
  resetBinding,
  resolveBindings,
  writeKeyBindingOverrides,
  type BindingScope,
  type KeyBindingOverrides,
  type ResolvedBindings,
} from "./keyBindings";

interface KeyBindingsContextValue {
  bindings: ResolvedBindings;
  overrides: KeyBindingOverrides;
  assign: (actionId: string, slot: number, combo: string) => void;
  clear: (actionId: string, slot: number) => void;
  reset: (actionId: string) => void;
  resetAll: () => void;
}

const KeyBindingsContext = createContext<KeyBindingsContextValue | null>(null);

export function KeyBindingsProvider({ children }: { children: ReactNode }) {
  const [overrides, setOverrides] = useState<KeyBindingOverrides>(readKeyBindingOverrides);
  const bindings = useMemo(() => resolveBindings(overrides), [overrides]);

  useEffect(() => {
    writeKeyBindingOverrides(overrides);
  }, [overrides]);

  const value = useMemo<KeyBindingsContextValue>(() => ({
    bindings,
    overrides,
    assign: (actionId, slot, combo) => (
      setOverrides((current) => assignBinding(current, resolveBindings(current), actionId, slot, combo))
    ),
    clear: (actionId, slot) => (
      setOverrides((current) => clearBinding(current, resolveBindings(current), actionId, slot))
    ),
    reset: (actionId) => setOverrides((current) => resetBinding(current, actionId)),
    resetAll: () => setOverrides({}),
  }), [bindings, overrides]);

  return <KeyBindingsContext.Provider value={value}>{children}</KeyBindingsContext.Provider>;
}

export function useKeyBindings(): KeyBindingsContextValue {
  const context = useContext(KeyBindingsContext);
  if (!context) throw new Error("useKeyBindings must be used within KeyBindingsProvider");
  return context;
}

export function useBoundKeys(
  scope: BindingScope,
  handlers: Readonly<Record<string, ((event: KeyboardEvent) => void) | undefined>>,
  options: { enabled?: boolean; ignoreInteractive?: boolean } = {},
): void {
  const { bindings } = useKeyBindings();
  const { enabled = true, ignoreInteractive = true } = options;

  useEffect(() => {
    if (!enabled) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (ignoreInteractive && isInteractiveTarget(event.target)) return;
      const combo = eventCombo(event);
      if (!combo) return;
      const actionId = matchBinding(bindings, scope, combo);
      const handler = actionId ? handlers[actionId] : undefined;
      if (!handler) return;
      event.preventDefault();
      handler(event);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [bindings, enabled, handlers, ignoreInteractive, scope]);
}
