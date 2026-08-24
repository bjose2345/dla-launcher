import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import {
  applyPresentationPreferences,
  readPresentationPreferences,
  translate,
  writePresentationPreferences,
  type LocaleId,
  type MessageKey,
  type ThemeId,
} from "./preferences";

interface PresentationContextValue {
  locale: LocaleId;
  theme: ThemeId;
  showPlayTime: boolean;
  includeUnreviewed: boolean;
  sidebarCollapsed: boolean;
  setLocale: (locale: LocaleId) => void;
  setTheme: (theme: ThemeId) => void;
  setShowPlayTime: (showPlayTime: boolean) => void;
  setIncludeUnreviewed: (includeUnreviewed: boolean) => void;
  setSidebarCollapsed: (sidebarCollapsed: boolean) => void;
  t: (key: MessageKey, values?: Record<string, string | number>) => string;
}

const PresentationContext = createContext<PresentationContextValue | null>(null);

export function PresentationProvider({ children }: { children: ReactNode }) {
  const [preferences, setPreferences] = useState(readPresentationPreferences);

  useEffect(() => {
    applyPresentationPreferences(preferences);
    writePresentationPreferences(preferences);
  }, [preferences]);

  const value = useMemo<PresentationContextValue>(() => ({
    ...preferences,
    setLocale: (locale) => setPreferences((current) => ({ ...current, locale })),
    setTheme: (theme) => setPreferences((current) => ({ ...current, theme })),
    setShowPlayTime: (showPlayTime) => setPreferences((current) => ({ ...current, showPlayTime })),
    setIncludeUnreviewed: (includeUnreviewed) => (
      setPreferences((current) => ({ ...current, includeUnreviewed }))
    ),
    setSidebarCollapsed: (sidebarCollapsed) => (
      setPreferences((current) => ({ ...current, sidebarCollapsed }))
    ),
    t: (key, values) => translate(preferences.locale, key, values),
  }), [preferences]);

  return <PresentationContext.Provider value={value}>{children}</PresentationContext.Provider>;
}

export function usePresentation(): PresentationContextValue {
  const context = useContext(PresentationContext);
  if (!context) throw new Error("usePresentation must be used within PresentationProvider");
  return context;
}
