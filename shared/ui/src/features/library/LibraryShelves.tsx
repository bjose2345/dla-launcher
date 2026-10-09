import {
  ArrowRight,
  CircleDashed,
  History,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import type { ReactNode } from "react";

import type { CatalogWork } from "../catalog";
import { usePresentation } from "../../preferences/PresentationProvider";
import { launchActionMessageKey } from "../../i18n/domainLabels";
import { formatDateTime } from "../../i18n/formatters";
import type { MessageKey } from "../../preferences/preferences";
import { LibraryArtwork } from "./LibraryArtwork";
import type { ManageTab } from "./LibraryManageDrawer";
import { LibraryWorkMenu } from "./LibraryWorkMenu";
import { ContentCarousel } from "../../carousel/ContentCarousel";
import {
  libraryContentKind,
  libraryDisplayCreator,
  libraryDisplayTitle,
} from "./libraryHome";
import { displayPath } from "../../displayPath";
import { effectiveIdentity, installationFolder } from "./libraryPresentation";
import {
  installationPrimaryAction,
  mediaActionMessageKey,
} from "./mediaSession";
import type {
  Installation,
  LaunchActionKind,
  LibraryRecentActivity,
  LibraryShelves as LibraryShelvesModel,
  MediaResume,
  PreparedPackageInstallation,
} from "./types";

interface LibraryShelvesProps {
  shelves: LibraryShelvesModel;
  workByCode: Map<string, CatalogWork>;
  preparedByInstallation: Map<string, PreparedPackageInstallation | null>;
  inLens: (installationId: string) => boolean;
  blockedExecutableInstallations: ReadonlySet<string>;
  onActivate: (installationId: string, action: LaunchActionKind | null) => void;
  onManage: (installationId: string, tab: ManageTab) => void;
  onOpenWork: (code: string) => void | Promise<void>;
  onBrowse?: (installationId: string) => void;
}

interface ShelfDefinition {
  key: string;
  title: MessageKey;
  help: MessageKey;
  icon: LucideIcon;
  content: ReactNode[];
}

export function LibraryShelves({
  shelves,
  workByCode,
  preparedByInstallation,
  inLens,
  blockedExecutableInstallations,
  onActivate,
  onManage,
  onOpenWork,
  onBrowse,
}: LibraryShelvesProps) {
  const { locale, t } = usePresentation();
  const withMenu = (installation: Installation, key: string, card: ReactNode) => {
    const work = workForInstallation(installation, workByCode);
    return (
      <div className="library-shelf-item" key={key}>
        {card}
        <LibraryWorkMenu
          className="library-shelf-item-menu"
          title={libraryDisplayTitle(installation, work, locale !== "ja-JP")}
          onOpenWork={work ? () => void onOpenWork(work.code) : undefined}
          onManage={(tab) => onManage(installation.id, tab)}
          onBrowse={onBrowse ? () => onBrowse(installation.id) : undefined}
        />
      </div>
    );
  };
  const installations = new Map(shelves.installations.map((installation) => [installation.id, installation]));
  const recent = shelves.recent.flatMap((activity) => {
    const installation = installations.get(activity.installationId);
    return installation && inLens(installation.id) ? [{ activity, installation }] : [];
  });
  const unfinished = resolveResumes(shelves.unfinished, installations).filter(({ installation }) => (
    inLens(installation.id)
  ));
  const neverLaunched = shelves.neverLaunched.flatMap((installationId) => {
    const installation = installations.get(installationId);
    return installation && inLens(installation.id) ? [installation] : [];
  });
  const definitions: ShelfDefinition[] = [
    {
      key: "recent",
      title: "library.shelf.recent",
      help: "library.shelf.recentHelp",
      icon: History,
      content: recent.map(({ activity, installation }) => withMenu(installation, installation.id, (
        <RecentShelfCard
          activity={activity}
          installation={installation}
          work={workForInstallation(installation, workByCode)}
          locale={locale}
          disabled={blockedExecutableInstallations.has(installation.id)}
          onOpen={() => onActivate(installation.id, activity.action)}
        />
      ))),
    },
    {
      key: "never-launched",
      title: "library.shelf.neverLaunched",
      help: "library.shelf.neverLaunchedHelp",
      icon: Sparkles,
      content: neverLaunched.map((installation) => {
        const action = installationPrimaryAction(
          installation,
          preparedByInstallation.get(installation.id),
        );
        return withMenu(installation, installation.id, (
          <NeverLaunchedShelfCard
            installation={installation}
            action={action}
            work={workForInstallation(installation, workByCode)}
            disabled={blockedExecutableInstallations.has(installation.id)}
            onOpen={() => onActivate(installation.id, action)}
          />
        ));
      }),
    },
    {
      key: "unfinished",
      title: "library.shelf.unfinished",
      help: "library.shelf.unfinishedHelp",
      icon: CircleDashed,
      content: unfinished.map(({ resume, installation }) => withMenu(installation, `${installation.id}:${resume.action}`, (
        <ResumeShelfCard
          installation={installation}
          resume={resume}
          work={workForInstallation(installation, workByCode)}
          onOpen={() => onActivate(installation.id, resume.action)}
        />
      ))),
    },
  ];

  return (
    <section className="library-shelves" aria-label={t("library.shelves")}>
      {definitions.filter((definition) => definition.content.length > 0).map((definition) => (
        <LibraryShelf definition={definition} key={definition.key} />
      ))}
    </section>
  );
}

function LibraryShelf({ definition }: { definition: ShelfDefinition }) {
  const { t } = usePresentation();
  const Icon = definition.icon;
  return (
    <section className="library-shelf">
      <header className="library-shelf-heading">
        <span className="library-shelf-icon"><Icon aria-hidden="true" /></span>
        <div>
          <h2>{t(definition.title)}</h2>
          <p>{t(definition.help)}</p>
        </div>
      </header>
      <ContentCarousel label={t(definition.title)}>{definition.content}</ContentCarousel>
    </section>
  );
}

function RecentShelfCard({
  activity,
  installation,
  work,
  locale,
  disabled,
  onOpen,
}: {
  activity: LibraryRecentActivity;
  installation: Installation;
  work?: CatalogWork;
  locale: string;
  disabled: boolean;
  onOpen: () => void;
}) {
  const { t } = usePresentation();
  const label = activity.kind === "media_session" && activity.action
    ? t(mediaActionMessageKey(activity.action))
    : t("library.shelf.launched");
  const kind = libraryContentKind(installation, activity.action);
  const title = libraryDisplayTitle(installation, work, locale !== "ja-JP");
  const creator = libraryDisplayCreator(installation, work, locale !== "ja-JP");
  return (
    <button
      className="library-shelf-card"
      type="button"
      disabled={disabled || (activity.kind === "executable_launch" && activity.active)}
      onClick={onOpen}
    >
      <span className="library-shelf-card-art">
        <LibraryArtwork kind={kind} title={title} work={work} />
        <span className="library-shelf-card-overlay" aria-hidden="true" />
        <span className="library-shelf-card-kicker">
          {label}
          {activity.active ? <b>{t("library.shelf.active")}</b> : null}
        </span>
      </span>
      <strong>{title}</strong>
      <code>{creator}</code>
      <span className="library-shelf-card-footer">
        <time dateTime={activity.occurredAt}>{formatDateTime(activity.occurredAt, locale)}</time>
        <ArrowRight aria-hidden="true" />
      </span>
    </button>
  );
}

function ResumeShelfCard({
  installation,
  resume,
  work,
  onOpen,
}: {
  installation: Installation;
  resume: MediaResume;
  work?: CatalogWork;
  onOpen: () => void;
}) {
  const { locale, t } = usePresentation();
  const percent = shelfResumePercent(resume);
  const kind = libraryContentKind(installation, resume.action);
  const title = libraryDisplayTitle(installation, work, locale !== "ja-JP");
  const creator = libraryDisplayCreator(installation, work, locale !== "ja-JP");
  return (
    <button className="library-shelf-card" type="button" onClick={onOpen}>
      <span className="library-shelf-card-art">
        <LibraryArtwork kind={kind} title={title} work={work} />
        <span className="library-shelf-card-overlay" aria-hidden="true" />
        <span className="library-shelf-card-kicker">{t(mediaActionMessageKey(resume.action))}</span>
      </span>
      <strong>{title}</strong>
      <code title={resume.relativePath}>{creator} · {resume.relativePath}</code>
      {percent === null ? null : (
        <span className="library-shelf-progress" aria-label={t("library.shelf.progress", { percent })}>
          <i style={{ width: `${percent}%` }} />
        </span>
      )}
      <span className="library-shelf-card-footer">
        <span>{formatResumePosition(resume, t)}</span>
        <ArrowRight aria-hidden="true" />
      </span>
    </button>
  );
}

function NeverLaunchedShelfCard({
  installation,
  action,
  work,
  disabled,
  onOpen,
}: {
  installation: Installation;
  action: LaunchActionKind | null;
  work?: CatalogWork;
  disabled: boolean;
  onOpen: () => void;
}) {
  const { locale, t } = usePresentation();
  const kind = libraryContentKind(installation, action);
  const title = libraryDisplayTitle(installation, work, locale !== "ja-JP");
  const creator = libraryDisplayCreator(installation, work, locale !== "ja-JP");
  return (
    <button className="library-shelf-card" type="button" disabled={disabled} onClick={onOpen}>
      <span className="library-shelf-card-art">
        <LibraryArtwork kind={kind} title={title} work={work} />
        <span className="library-shelf-card-overlay" aria-hidden="true" />
        <span className="library-shelf-card-kicker">{t("library.shelf.ready")}</span>
      </span>
      <strong>{title}</strong>
      <code title={displayPath(installationFolder(installation))}>{creator}</code>
      <span className="library-shelf-card-footer">
        <span>{t(action === "launch_executable"
          ? "detail.play"
          : action
            ? mediaActionMessageKey(action)
            : "library.setUp")}</span>
        <ArrowRight aria-hidden="true" />
      </span>
    </button>
  );
}

function workForInstallation(
  installation: Installation,
  workByCode: Map<string, CatalogWork>,
): CatalogWork | undefined {
  const code = effectiveIdentity(installation);
  return code ? workByCode.get(code) : undefined;
}

function resolveResumes(
  resumes: MediaResume[],
  installations: Map<string, Installation>,
): Array<{ resume: MediaResume; installation: Installation }> {
  return resumes.flatMap((resume) => {
    const installation = installations.get(resume.installationId);
    return installation ? [{ resume, installation }] : [];
  });
}

export function shelfResumePercent(resume: MediaResume): number | null {
  if (resume.durationMs === null || resume.durationMs <= 0) return null;
  return Math.round(Math.max(0, Math.min(100, (resume.positionMs / resume.durationMs) * 100)));
}

function formatResumePosition(
  resume: MediaResume,
  t: ReturnType<typeof usePresentation>["t"],
): string {
  if (resume.durationMs === null || resume.durationMs <= 0) {
    return t(launchActionMessageKey(resume.action));
  }
  return `${formatMilliseconds(resume.positionMs)} / ${formatMilliseconds(resume.durationMs)}`;
}

function formatMilliseconds(value: number): string {
  const totalSeconds = Math.max(0, Math.floor(value / 1_000));
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}
