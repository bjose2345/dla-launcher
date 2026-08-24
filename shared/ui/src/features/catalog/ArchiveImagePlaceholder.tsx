import { ArchiveGlyph } from "./ArchiveGlyph";

interface ArchiveImagePlaceholderProps {
  label: string;
  className?: string;
}

export function ArchiveImagePlaceholder({ label, className = "" }: ArchiveImagePlaceholderProps) {
  return (
    <span
      className={`archive-image-placeholder ${className}`.trim()}
      role={label ? "img" : undefined}
      aria-label={label || undefined}
      aria-hidden={label ? undefined : "true"}
    >
      <ArchiveGlyph bookmarkClassName="archive-image-placeholder-bookmark" />
    </span>
  );
}
