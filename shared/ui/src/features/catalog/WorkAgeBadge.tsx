import { usePresentation } from "../../preferences/PresentationProvider";
import { ageRatingLabel } from "./workDetail";

export function WorkAgeBadge({ age }: { age: string }) {
  const { t } = usePresentation();
  const normalized = age.trim().toLowerCase();
  const tone = normalized === "r18"
    ? "restricted"
    : normalized === "r15"
      ? "teen"
      : normalized === "all_ages"
        ? "all"
        : "unknown";

  return (
    <span className={`work-age-badge age-${tone}`}>
      {ageRatingLabel(age, {
        allAges: t("detail.allAges"),
        notCataloged: t("detail.notCataloged"),
      })}
    </span>
  );
}
