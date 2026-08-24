import { useEffect, useMemo, useState } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { ArchiveImagePlaceholder } from "./ArchiveImagePlaceholder";
import { visualImageUrls } from "./workImages";

interface WorkCardImageProps {
  code: string;
  title: string;
  mainImageUrls: string[];
  thumbnailUrls: string[];
}

export function WorkCardImage({
  code,
  title,
  mainImageUrls,
  thumbnailUrls,
}: WorkCardImageProps) {
  const { t } = usePresentation();
  const candidates = useMemo(
    () => visualImageUrls({ mainImageUrls, thumbnailUrls }),
    [mainImageUrls, thumbnailUrls],
  );
  const [candidate, setCandidate] = useState(0);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    setCandidate(0);
    setLoaded(false);
  }, [code, candidates]);

  const source = candidates[candidate];
  if (!source) {
    return <ArchiveImagePlaceholder className="work-card-image-missing" label={t("image.noCover", { title })} />;
  }

  return (
    <span className={`work-card-image cover-hover-media ${loaded ? "loaded" : "loading"}`}>
      <img
        src={source}
        alt={title}
        width={900}
        height={600}
        loading="lazy"
        decoding="async"
        fetchPriority="low"
        sizes="(min-width: 1280px) 20vw, (min-width: 768px) 28vw, 45vw"
        referrerPolicy="no-referrer"
        onLoad={() => setLoaded(true)}
        onError={() => {
          setLoaded(false);
          setCandidate((current) => current + 1);
        }}
      />
    </span>
  );
}
