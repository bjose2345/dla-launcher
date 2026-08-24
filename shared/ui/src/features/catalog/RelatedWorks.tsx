import {
  BookOpen,
  Box,
  ChevronRight,
  Crown,
  Film,
  Gift,
  Headphones,
  Image,
  Languages,
  Layers,
  Link2,
  Music,
  Sparkles,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useState } from "react";

import { usePresentation } from "../../preferences/PresentationProvider";
import { ArchiveImagePlaceholder } from "./ArchiveImagePlaceholder";
import type { CatalogRelatedWork } from "./types";

type RelationKind = { icon: LucideIcon; family: string };
type RelationShelf = RelationKind & { label: string; items: CatalogRelatedWork[] };

const relationKinds: Record<string, RelationKind> = {
  bonus: { icon: Gift, family: "bonus" },
  dlc: { icon: Layers, family: "dlc" },
  audio_ver: { icon: Headphones, family: "audio" },
  audio_manga_ver: { icon: Headphones, family: "audio" },
  audio_video_ver: { icon: Headphones, family: "audio" },
  soundtrack: { icon: Music, family: "audio" },
  video_ver: { icon: Film, family: "video" },
  video_cg_ver: { icon: Film, family: "video" },
  cg_ver: { icon: Image, family: "cg" },
  manga_ver: { icon: Image, family: "cg" },
  "3d_ver": { icon: Box, family: "3d" },
  vr_ver: { icon: Box, family: "vr" },
  translation: { icon: Languages, family: "text" },
  ai_translation: { icon: Languages, family: "text" },
  side_ver: { icon: BookOpen, family: "text" },
  patch: { icon: Wrench, family: "patch" },
  live2d_patch: { icon: Wrench, family: "patch" },
  demo: { icon: Sparkles, family: "demo" },
  remake: { icon: Sparkles, family: "remaster" },
  remaster: { icon: Sparkles, family: "remaster" },
};
const fallbackKind: RelationKind = { icon: Link2, family: "other" };
const familyRank: Record<string, number> = { base: 0, bonus: 1, dlc: 2, audio: 3, video: 4, cg: 5 };

export function relatedWorkShelves(works: CatalogRelatedWork[], baseLabel: string): RelationShelf[] {
  const shelves: RelationShelf[] = [];
  const baseItems = works.filter((work) => work.direction === "child");
  if (baseItems.length) shelves.push({ icon: Crown, family: "base", label: baseLabel, items: baseItems });

  const byType = new Map<string, CatalogRelatedWork[]>();
  for (const work of works) {
    if (work.direction === "child") continue;
    const items = byType.get(work.relationTypeCode) ?? [];
    items.push(work);
    byType.set(work.relationTypeCode, items);
  }
  for (const [type, items] of byType) {
    const kind = relationKinds[type] ?? fallbackKind;
    shelves.push({ ...kind, label: items[0]?.relationTypeLabel || type, items });
  }
  return shelves.sort((left, right) => {
    const family = (familyRank[left.family] ?? 6) - (familyRank[right.family] ?? 6);
    if (family !== 0) return family;
    if (left.items.length !== right.items.length) return right.items.length - left.items.length;
    return left.label.localeCompare(right.label);
  });
}

export function RelatedWorks({
  works,
  onOpenWork,
}: {
  works: CatalogRelatedWork[];
  onOpenWork: (code: string) => void;
}) {
  const { t } = usePresentation();
  if (!works.length) return <p className="work-detail-empty">{t("detail.related.empty")}</p>;

  return (
    <div className="related-works">
      {relatedWorkShelves(works, t("detail.related.baseWork")).map((shelf) => {
        const Icon = shelf.icon;
        return (
          <section className="related-shelf" data-family={shelf.family} key={`${shelf.family}:${shelf.label}`}>
            <div className="related-shelf-heading">
              <span><Icon aria-hidden="true" /></span>
              <h3>{shelf.label}</h3>
              <small>{shelf.items.length}</small>
              <i aria-hidden="true" />
            </div>
            <ul className="related-grid">
              {shelf.items.map((work) => (
                <li key={`${work.code}:${work.direction}`}>
                  <button type="button" className="related-card" onClick={() => onOpenWork(work.code)}>
                    <span className="related-thumb">
                      <Icon aria-hidden="true" />
                      <RelatedImage urls={work.thumbnailUrls} />
                    </span>
                    <span className="related-copy">
                      <strong>{work.titleEnglish || work.title}</strong>
                      <small>{work.code}</small>
                    </span>
                    <span className="related-open">{t("detail.related.open")} <ChevronRight aria-hidden="true" /></span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        );
      })}
    </div>
  );
}

function RelatedImage({ urls }: { urls: string[] }) {
  const [index, setIndex] = useState(0);
  useEffect(() => setIndex(0), [urls]);
  const source = urls[index];
  if (!source) return <ArchiveImagePlaceholder className="related-image-placeholder" label="" />;
  return <img src={source} alt="" loading="lazy" decoding="async" onError={() => setIndex((value) => value + 1)} />;
}
