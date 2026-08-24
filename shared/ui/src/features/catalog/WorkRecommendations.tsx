import { Sparkles, UsersRound } from "lucide-react";
import { useMemo } from "react";

import { ContentCarousel } from "../../carousel/ContentCarousel";
import { CatalogWorkCard } from "./CatalogResults";
import { recommendationReasonLabels, visibleRecommendationLanes } from "./catalogRecommendations";
import type {
  CatalogRecommendationItem,
  CatalogRecommendationLane,
  CatalogRecommendationLaneKey,
  CatalogRecommendations,
} from "./types";
import { usePresentation } from "../../preferences/PresentationProvider";

interface WorkRecommendationsProps {
  recommendations?: CatalogRecommendations;
  loading: boolean;
  onOpenWork: (code: string) => void;
}

export function WorkRecommendations({
  recommendations,
  loading,
  onOpenWork,
}: WorkRecommendationsProps) {
  const { locale, t } = usePresentation();
  const lanes = useMemo(
    () => recommendations ? visibleRecommendationLanes(recommendations) : [],
    [recommendations],
  );

  if (!loading && lanes.length === 0) return null;

  return (
    <section className="work-recommendations" aria-labelledby="work-recommendations-title">
      <header className="work-recommendations-heading">
        <div>
          <span>{t("recommendation.local")}</span>
          <h2 id="work-recommendations-title">{t("recommendation.title")}</h2>
        </div>
      </header>
      {loading ? (
        <RecommendationSkeleton label={t("recommendation.loading")} />
      ) : (
        lanes.map((lane) => (
          <RecommendationLane
            lane={lane}
            englishLabels={locale !== "ja-JP"}
            onOpenWork={onOpenWork}
            key={lane.key}
          />
        ))
      )}
    </section>
  );
}

function RecommendationLane({
  lane,
  englishLabels,
  onOpenWork,
}: {
  lane: CatalogRecommendationLane;
  englishLabels: boolean;
  onOpenWork: (code: string) => void;
}) {
  const { t } = usePresentation();
  const copy = laneCopy(lane.key);

  const Icon = lane.key === "same_circle" ? UsersRound : Sparkles;
  return (
    <section className="recommendation-lane">
      <header className="recommendation-lane-heading">
        <span className="recommendation-lane-icon"><Icon aria-hidden="true" /></span>
        <div>
          <h3>{t(copy.title)}</h3>
          {copy.subtitle ? <p>{t(copy.subtitle)}</p> : null}
        </div>
        <span className="recommendation-lane-count">{lane.items.length}</span>
      </header>
      <ContentCarousel label={t(copy.title)}>
        {lane.items.map((item, index) => (
          <RecommendationCard
            item={item}
            englishLabels={englishLabels}
            animationIndex={index}
            onOpenWork={onOpenWork}
            key={item.work.code}
          />
        ))}
      </ContentCarousel>
    </section>
  );
}

function RecommendationCard({
  item,
  englishLabels,
  animationIndex,
  onOpenWork,
}: {
  item: CatalogRecommendationItem;
  englishLabels: boolean;
  animationIndex: number;
  onOpenWork: (code: string) => void;
}) {
  const { t } = usePresentation();
  const reasons = recommendationReasonLabels(item, englishLabels).slice(0, 3);
  return (
    <article className="recommendation-card">
      <CatalogWorkCard
        work={item.work}
        englishLabels={englishLabels}
        animationIndex={animationIndex}
        onOpenWork={onOpenWork}
      />
      {reasons.length > 0 && (
        <p>{t("recommendation.because", { reasons: reasons.join(" · ") })}</p>
      )}
    </article>
  );
}

function RecommendationSkeleton({ label }: { label: string }) {
  return (
    <div className="recommendation-skeleton" role="status">
      <span>{label}</span>
      <div>
        <i /><i /><i /><i />
      </div>
    </div>
  );
}

function laneCopy(key: CatalogRecommendationLaneKey): {
  title: "recommendation.sameCircle" | "recommendation.similar";
  subtitle?: "recommendation.sameCircleSubtitle";
} {
  return key === "same_circle"
    ? { title: "recommendation.sameCircle", subtitle: "recommendation.sameCircleSubtitle" }
    : { title: "recommendation.similar" };
}
