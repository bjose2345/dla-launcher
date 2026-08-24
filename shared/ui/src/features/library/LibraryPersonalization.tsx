import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import {
  EyeOff,
  Headphones,
  Heart,
  LoaderCircle,
  PlayCircle,
  Sparkles,
} from "lucide-react";

import { CatalogWorkCard } from "../catalog";
import { AnchoredPopover } from "../../app/AnchoredPopover";
import { usePresentation } from "../../preferences/PresentationProvider";
import { ContentCarousel } from "../../carousel/ContentCarousel";
import type {
  LaunchActionKind,
  LibraryGateway,
  LocalPersonalization,
  PersonalizationAnchor,
  PersonalizedRecommendationItem,
  WorkPreferenceKind,
} from "./types";

interface LibraryPersonalizationProps {
  gateway: LibraryGateway;
  personalization: LocalPersonalization;
  onOpenWork: (code: string) => void | Promise<void>;
  onOpenVoiceQueue: () => void | Promise<void>;
}

interface PreferenceChange {
  workCode: string;
  preference: WorkPreferenceKind | null;
}

export const discoverLanes = ["suggested", "favorites", "voiceMix"] as const;

export type DiscoverLane = (typeof discoverLanes)[number];

export function firstLaneWithContent(personalization: LocalPersonalization): DiscoverLane {
  if (personalization.becauseYou.length > 0) return "suggested";
  if (personalization.favorites.length > 0) return "favorites";
  if (personalization.voiceMix.length > 0) return "voiceMix";
  return "suggested";
}

const laneLabelKeys = {
  suggested: "library.becauseYou",
  favorites: "library.favorites",
  voiceMix: "library.voiceMix",
} as const;

export function LibraryPersonalization({
  gateway,
  personalization,
  onOpenWork,
  onOpenVoiceQueue,
}: LibraryPersonalizationProps) {
  const { locale, t } = usePresentation();
  const queryClient = useQueryClient();
  const initialLane = useMemo(() => firstLaneWithContent(personalization), [personalization]);
  const [selectedLane, setSelectedLane] = useState<DiscoverLane | null>(null);
  const lane = selectedLane ?? initialLane;
  const preference = useMutation({
    mutationFn: ({ workCode, preference: nextPreference }: PreferenceChange) => (
      gateway.replaceWorkPreference(workCode, nextPreference)
    ),
    onSettled: async (_result, _error, change) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["library", "personalization"] }),
        queryClient.invalidateQueries({ queryKey: ["library", "preferences"] }),
        queryClient.invalidateQueries({ queryKey: ["library", "preference", change.workCode] }),
      ]);
    },
  });
  const becauseRemaining = Math.max(
    0,
    personalization.becauseYouMinimum - personalization.activityWorkCount,
  );
  const voiceRemaining = Math.max(
    0,
    personalization.voiceMixMinimum - personalization.voiceActivityWorkCount,
  );
  const changePreference = (workCode: string, nextPreference: WorkPreferenceKind | null) => {
    if (!preference.isPending) {
      preference.mutate({ workCode, preference: nextPreference });
    }
  };
  const counts = {
    suggested: personalization.becauseYou.length,
    favorites: personalization.favorites.length,
    voiceMix: personalization.voiceMix.length,
  } as const;
  const englishLabels = locale !== "ja-JP";
  const changingCode = preference.isPending ? preference.variables?.workCode ?? null : null;
  const moveLaneFocus = (
    event: ReactKeyboardEvent<HTMLButtonElement>,
    current: DiscoverLane,
  ) => {
    const index = discoverLanes.indexOf(current);
    let next: DiscoverLane | undefined;
    if (event.key === "ArrowRight") next = discoverLanes[(index + 1) % discoverLanes.length];
    if (event.key === "ArrowLeft") {
      next = discoverLanes[(index - 1 + discoverLanes.length) % discoverLanes.length];
    }
    if (event.key === "Home") next = discoverLanes[0];
    if (event.key === "End") next = discoverLanes[discoverLanes.length - 1];
    if (!next) return;
    event.preventDefault();
    setSelectedLane(next);
    document.getElementById(`library-discover-lane-${next}`)?.focus();
  };

  return (
    <section className="library-discover" aria-label={t("library.personalization")}>
      <h2 className="library-discover-tab">
        <Sparkles aria-hidden="true" />{t("library.personalization")}
      </h2>

      {preference.error ? (
        <div className="library-callout library-callout-error" role="alert">
          {t("library.preferenceError", { error: String(preference.error) })}
        </div>
      ) : null}

      <div className="library-discover-lanes" role="tablist" aria-label={t("library.personalization")}>
        {discoverLanes.map((value) => (
          <button
            aria-controls="library-discover-panel"
            aria-selected={lane === value}
            className={lane === value ? "is-active" : undefined}
            id={`library-discover-lane-${value}`}
            key={value}
            role="tab"
            tabIndex={lane === value ? 0 : -1}
            type="button"
            onClick={() => setSelectedLane(value)}
            onKeyDown={(event) => moveLaneFocus(event, value)}
          >
            <LaneIcon lane={value} />
            {t(laneLabelKeys[value])}
            <em>{counts[value]}</em>
          </button>
        ))}
      </div>

      <div
        aria-labelledby={`library-discover-lane-${lane}`}
        id="library-discover-panel"
        role="tabpanel"
        tabIndex={0}
      >
        {lane === "suggested" ? (
          <DiscoverLaneBody
            empty={becauseRemaining > 0
              ? t("library.becauseYouWaiting", { remaining: becauseRemaining })
              : t("library.becauseYouEmpty")}
            help={t("library.becauseYouHelp")}
            label={t("library.becauseYou")}
          >
            {personalization.becauseYou.map((item, index) => (
              <RecommendationCard
                changing={changingCode === item.work.code}
                englishLabels={englishLabels}
                index={index}
                item={item}
                key={item.work.code}
                onDismiss={() => changePreference(item.work.code, "not_interested")}
                onOpenWork={onOpenWork}
              />
            ))}
          </DiscoverLaneBody>
        ) : null}

        {lane === "favorites" ? (
          <DiscoverLaneBody
            empty={t("library.favoritesEmpty")}
            help={t("library.favoritesHelp")}
            label={t("library.favorites")}
          >
            {personalization.favorites.map((work, index) => (
              <PersonalizationCard
                actionIcon={<Heart aria-hidden="true" />}
                actionLabel={t("library.removeFavorite")}
                changing={changingCode === work.code}
                englishLabels={englishLabels}
                index={index}
                key={work.code}
                onAction={() => changePreference(work.code, null)}
                onOpenWork={onOpenWork}
                work={work}
              />
            ))}
          </DiscoverLaneBody>
        ) : null}

        {lane === "voiceMix" ? (
          <DiscoverLaneBody
            empty={voiceRemaining > 0
              ? t("library.voiceMixWaiting", { remaining: voiceRemaining })
              : t("library.voiceMixEmpty")}
            help={t("library.voiceMixHelp")}
            label={t("library.voiceMix")}
          >
            {personalization.voiceMix.map((item, index) => (
              <RecommendationCard
                changing={changingCode === item.work.code}
                englishLabels={englishLabels}
                index={index}
                item={item}
                key={item.work.code}
                onDismiss={() => changePreference(item.work.code, "not_interested")}
                onOpenWork={onOpenWork}
              />
            ))}
          </DiscoverLaneBody>
        ) : null}
      </div>

      {lane === "voiceMix" ? (
        <div className="library-discover-mix">
          <div>
            <strong>{t("library.voiceMix")}</strong>
            <span>{voiceRemaining > 0
              ? t("library.voiceMixWaiting", { remaining: voiceRemaining })
              : t("library.voiceMixHelp")}</span>
          </div>
          <button
            className="library-discover-mix-action"
            disabled={voiceRemaining > 0}
            type="button"
            onClick={() => void onOpenVoiceQueue()}
          >
            <PlayCircle aria-hidden="true" />{t("library.playVoiceMix")}
          </button>
        </div>
      ) : null}

      <p className="library-discover-note">{t("library.personalizationHelp")}</p>
    </section>
  );
}

function LaneIcon({ lane }: { lane: DiscoverLane }) {
  if (lane === "favorites") return <Heart aria-hidden="true" />;
  if (lane === "voiceMix") return <Headphones aria-hidden="true" />;
  return <Sparkles aria-hidden="true" />;
}

function DiscoverLaneBody({
  label,
  help,
  empty,
  children,
}: {
  label: string;
  help: string;
  empty: string;
  children: ReactNode;
}) {
  const hasItems = Array.isArray(children) ? children.length > 0 : Boolean(children);
  if (!hasItems) return <p className="library-discover-empty">{empty}</p>;
  return (
    <>
      <p className="library-discover-help">{help}</p>
      <ContentCarousel label={label}>{children}</ContentCarousel>
    </>
  );
}

function RecommendationCard({
  item,
  index,
  englishLabels,
  changing,
  onOpenWork,
  onDismiss,
}: {
  item: PersonalizedRecommendationItem;
  index: number;
  englishLabels: boolean;
  changing: boolean;
  onOpenWork: (code: string) => void | Promise<void>;
  onDismiss: () => void;
}) {
  const { t } = usePresentation();
  return (
    <PersonalizationCard
      actionIcon={<EyeOff aria-hidden="true" />}
      actionLabel={t("library.notInterested")}
      changing={changing}
      englishLabels={englishLabels}
      index={index}
      onAction={onDismiss}
      onOpenWork={onOpenWork}
      reason={item.anchors.length > 0 ? <RecommendationReason anchors={item.anchors} /> : null}
      work={item.work}
    />
  );
}

function PersonalizationCard({
  work,
  index,
  englishLabels,
  actionLabel,
  actionIcon,
  changing,
  reason = null,
  onOpenWork,
  onAction,
}: {
  work: PersonalizedRecommendationItem["work"];
  index: number;
  englishLabels: boolean;
  actionLabel: string;
  actionIcon: ReactNode;
  changing: boolean;
  reason?: ReactNode;
  onOpenWork: (code: string) => void | Promise<void>;
  onAction: () => void;
}) {
  return (
    <article className="library-discover-card">
      <CatalogWorkCard
        animationIndex={index}
        englishLabels={englishLabels}
        onOpenWork={onOpenWork}
        work={work}
      />
      <div className="library-discover-card-foot">
        <div className="library-discover-card-reason">{reason}</div>
        <button
          aria-label={actionLabel}
          disabled={changing}
          title={actionLabel}
          type="button"
          onClick={onAction}
        >
          {changing ? <LoaderCircle className="library-spin" aria-hidden="true" /> : actionIcon}
        </button>
      </div>
    </article>
  );
}

function RecommendationReason({ anchors }: { anchors: PersonalizationAnchor[] }) {
  const { t } = usePresentation();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const descriptionId = useId();
  const [open, setOpen] = useState(false);
  const descriptions = anchors.map((anchor) => (
    `${personalizationReason(anchor, t)}: ${anchor.title}`
  ));

  return (
    <>
      <button
        ref={triggerRef}
        aria-describedby={descriptionId}
        className="library-discover-reason"
        type="button"
        onBlur={() => setOpen(false)}
        onClick={() => setOpen(true)}
        onFocus={() => setOpen(true)}
        onPointerEnter={() => setOpen(true)}
        onPointerLeave={() => setOpen(false)}
      >
        {personalizationReason(anchors[0]!, t)}
      </button>
      {open ? (
        <AnchoredPopover
          align="start"
          anchorRef={triggerRef}
          className="library-discover-reason-tooltip"
          id={descriptionId}
          maximumWidth={320}
          role="tooltip"
          side="top"
          onClose={() => setOpen(false)}
        >
          <ul>
            {descriptions.map((description, index) => (
              <li key={`${anchors[index]!.workCode}:${anchors[index]!.action}`}>
                {description}
              </li>
            ))}
          </ul>
        </AnchoredPopover>
      ) : (
        <span className="library-discover-reason-description" id={descriptionId}>
          {descriptions.join("; ")}
        </span>
      )}
    </>
  );
}

function personalizationReason(
  anchor: PersonalizationAnchor,
  t: ReturnType<typeof usePresentation>["t"],
): string {
  return t(reasonMessageKey(anchor.action));
}

function reasonMessageKey(action: LaunchActionKind):
  | "library.reasonPlayed"
  | "library.reasonListened"
  | "library.reasonRead"
  | "library.reasonWatched"
  | "library.reasonOpened" {
  switch (action) {
    case "launch_executable": return "library.reasonPlayed";
    case "play_audio": return "library.reasonListened";
    case "read_images":
    case "open_document": return "library.reasonRead";
    case "play_video": return "library.reasonWatched";
    case "open_archive":
    case "open_android_package": return "library.reasonOpened";
  }
}
