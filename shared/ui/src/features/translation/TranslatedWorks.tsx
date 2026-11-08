import { useQuery } from "@tanstack/react-query";
import { Languages } from "lucide-react";
import { createContext, useContext, type ReactNode } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { translatedWorksKey } from "./archiveKeys";
import type { TranslationGateway } from "./types";

const TranslatedWorks = createContext<Readonly<Record<string, string>>>({});

/** Every work the Archive has a translation of, for the library to mark. */
export function TranslatedWorksProvider({
  gateway,
  children,
}: {
  gateway?: TranslationGateway;
  children: ReactNode;
}) {
  const works = useQuery({
    queryKey: translatedWorksKey,
    queryFn: () => gateway!.translatedWorks(),
    enabled: Boolean(gateway),
    // The list changes when the Archive publishes; once an hour is plenty.
    staleTime: 60 * 60_000,
    retry: false,
  });
  return <TranslatedWorks.Provider value={works.data ?? {}}>{children}</TranslatedWorks.Provider>;
}

/** The day the Archive's translation of a work was made, if it has one. */
export function useArchiveTranslation(code: string | null | undefined): string | null {
  const works = useContext(TranslatedWorks);
  return code ? works[code] ?? null : null;
}

/** When the Archive made a translation (`2026-11-03T10:36:17Z`), as a day. */
export function madeDate(made: string, locale: string): string {
  // The UTC time the pack was made; a day alone is midnight here.
  const day = made.includes("T") ? made : `${made.slice(0, 10)}T00:00:00`;
  return new Date(day).toLocaleDateString(locale, { day: "numeric", month: "short", year: "numeric" });
}

/** Marks a work the Archive has a translation of. */
export function ArchiveTranslationBadge({ code, className }: { code: string | null | undefined; className: string }) {
  const { locale, t } = usePresentation();
  const made = useArchiveTranslation(code);
  if (!made) return null;
  const title = t("translation.archive.badgeTitle", { date: madeDate(made, locale) });
  return (
    <span className={className} title={title} aria-label={title}>
      <Languages aria-hidden="true" />
      {t("translation.archive.badge")}
    </span>
  );
}
