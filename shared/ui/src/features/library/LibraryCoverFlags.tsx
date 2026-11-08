import { usePresentation } from "../../preferences/PresentationProvider";
import { ArchiveTranslationBadge } from "../translation/TranslatedWorks";
import { libraryContentKindLabel, type LibraryContentKind } from "./libraryHome";

/**
 * The flags in a cover's top-left corner, side by side as the catalog's are:
 * what the work is, then EN when the Archive has a translation of it.
 */
export function LibraryCoverFlags({ kind, code }: { kind: LibraryContentKind; code: string | null }) {
  const { t } = usePresentation();
  return (
    <span className="library-cover-flags">
      <span className="library-flag">{libraryContentKindLabel(kind, t)}</span>
      <ArchiveTranslationBadge code={code} className="library-flag is-translation" />
    </span>
  );
}
