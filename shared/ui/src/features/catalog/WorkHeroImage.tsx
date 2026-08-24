import { useState } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { ArchiveImagePlaceholder } from "./ArchiveImagePlaceholder";

interface WorkHeroImageProps {
  title: string;
  urls: string[];
  onOpen: () => void;
}

export function WorkHeroImage({ title, urls, onOpen }: WorkHeroImageProps) {
  const { t } = usePresentation();
  const [index, setIndex] = useState(0);
  const source = urls[index];

  if (!source) {
    return <ArchiveImagePlaceholder className="work-hero-image-missing" label={t("image.noCover", { title })} />;
  }

  return (
    <button
      className="work-hero-art"
      type="button"
      aria-label={t("image.openGallery", { title })}
      onClick={onOpen}
    >
      <div className="work-hero-backdrop" style={{ backgroundImage: `url(${source})` }} />
      <img
        src={source}
        alt={t("image.cover", { title })}
        onError={() => setIndex((current) => current + 1)}
      />
      <span className="work-hero-scanline" aria-hidden="true" />
      <span className="work-hero-overlay" aria-hidden="true" />
    </button>
  );
}
