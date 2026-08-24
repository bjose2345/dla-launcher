import { ArchiveGlyph } from "../features/catalog/ArchiveGlyph";

export function BrandMark() {
  return (
    <span className="brand-mark" aria-hidden="true">
      <ArchiveGlyph outlineClassName="brand-mark-outline" bookmarkClassName="brand-mark-bookmark" />
    </span>
  );
}
