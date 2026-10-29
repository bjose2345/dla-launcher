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
import { isReaderAction } from "./ReaderProvider";
import {
  libraryContentKind,
  libraryDisplayCreator,
  libraryDisplayTitle,
  resumePlaceLabel,
} from "./libraryHome";
import { effectiveIdentity } from "./libraryPresentation";
import { useOwnWork } from "./ownWorkDetails";
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
  /** The work shown in the featured spot, marked on its card. */
  selectedId?: string | null;
  onSelect: (installationId: string) => void;
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
  selectedId,
  onSelect,
  onActivate,
  onManage,
  onOpenWork,
  onBrowse,
}: LibraryShelvesProps) {
  const { locale, t } = usePresentation();
  const own = useOwnWork();
  const withMenu = (
    installation: Installation,
    key: string,
    card: ReactNode,
    launch: { action: LaunchActionKind | null; disabled: boolean },
  ) => {
    const work = workForInstallation(installation, workByCode);
    const label = t("library.launchFor", {
      action: t(launchLabelKey(launch.action)),
      title: libraryDisplayTitle(installation, work, locale !== "ja-JP"),
    });
    return (
      <div className="library-shelf-item" key={key}>
        {card}
        <button
          className="library-shelf-card-launch"
          type="button"
          aria-label={label}
          title={label}
          disabled={launch.disabled}
          onClick={() => onActivate(installation.id, launch.action)}
        >
          <ArrowRight aria-hidden="true" />
        </button>
        <LibraryWorkMenu
          className="library-shelf-item-menu"
          title={libraryDisplayTitle(installation, work, locale !== "ja-JP")}
          onOpenWork={work
            ? () => void onOpenWork(work.code)
            : own.openPage ? () => own.openPage!(installation.id) : undefined}
          ownWork={!work}
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
          selected={selectedId === installation.id}
          onSelect={() => onSelect(installation.id)}
        />
      ), {
        action: activity.action,
        disabled: blockedExecutableInstallations.has(installation.id)
          || (activity.kind === "executable_launch" && activity.active),
      })),
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
            selected={selectedId === installation.id}
            onSelect={() => onSelect(installation.id)}
          />
        ), { action, disabled: blockedExecutableInstallations.has(installation.id) });
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
          selected={selectedId === installation.id}
          onSelect={() => onSelect(installation.id)}
        />
      ), { action: resume.action, disabled: false })),
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
  selected,
  onSelect,
}: {
  activity: LibraryRecentActivity;
  installation: Installation;
  work?: CatalogWork;
  locale: string;
  selected: boolean;
  onSelect: () => void;
}) {
  const { t } = usePresentation();
  const label = activity.kind === "media_session" && activity.action
    ? t(mediaActionMessageKey(activity.action))
    : t("library.shelf.launched");
  const own = useOwnWork();
  const kind = libraryContentKind(installation, activity.action);
  const title = libraryDisplayTitle(installation, work, locale !== "ja-JP");
  const creator = libraryDisplayCreator(work, locale !== "ja-JP", own.details(installation.id));
  return (
    <button className="library-shelf-card" type="button" aria-current={selected || undefined} onClick={onSelect}>
      <span className="library-shelf-card-art">
        <LibraryArtwork kind={kind} title={title} work={work} cover={own.coverUrl(installation.id)} />
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
      </span>
    </button>
  );
}

function ResumeShelfCard({
  installation,
  resume,
  work,
  selected,
  onSelect,
}: {
  installation: Installation;
  resume: MediaResume;
  work?: CatalogWork;
  selected: boolean;
  onSelect: () => void;
}) {
  const { locale, t } = usePresentation();
  const own = useOwnWork();
  const percent = shelfResumePercent(resume);
  const kind = libraryContentKind(installation, resume.action);
  const title = libraryDisplayTitle(installation, work, locale !== "ja-JP");
  const creator = libraryDisplayCreator(work, locale !== "ja-JP", own.details(installation.id));
  return (
    <button className="library-shelf-card" type="button" aria-current={selected || undefined} onClick={onSelect}>
      <span className="library-shelf-card-art">
        <LibraryArtwork kind={kind} title={title} work={work} cover={own.coverUrl(installation.id)} />
        <span className="library-shelf-card-overlay" aria-hidden="true" />
        <span className="library-shelf-card-kicker">{t(mediaActionMessageKey(resume.action))}</span>
      </span>
      <strong>{title}</strong>
      <code>{creator}</code>
      {percent === null ? null : (
        <span className="library-shelf-progress" aria-label={t("library.shelf.progress", { percent })}>
          <i style={{ width: `${percent}%` }} />
        </span>
      )}
      <span className="library-shelf-card-footer">
        <span>{formatResumePosition(resume, t)}</span>
      </span>
    </button>
  );
}

function NeverLaunchedShelfCard({
  installation,
  action,
  work,
  selected,
  onSelect,
}: {
  installation: Installation;
  action: LaunchActionKind | null;
  work?: CatalogWork;
  selected: boolean;
  onSelect: () => void;
}) {
  const { locale, t } = usePresentation();
  const own = useOwnWork();
  const kind = libraryContentKind(installation, action);
  const title = libraryDisplayTitle(installation, work, locale !== "ja-JP");
  const creator = libraryDisplayCreator(work, locale !== "ja-JP", own.details(installation.id));
  return (
    <button className="library-shelf-card" type="button" aria-current={selected || undefined} onClick={onSelect}>
      <span className="library-shelf-card-art">
        <LibraryArtwork kind={kind} title={title} work={work} cover={own.coverUrl(installation.id)} />
        <span className="library-shelf-card-overlay" aria-hidden="true" />
        <span className="library-shelf-card-kicker">{t("library.shelf.ready")}</span>
      </span>
      <strong>{title}</strong>
      <code>{creator}</code>
      <span className="library-shelf-card-footer">
        <span>{t(launchLabelKey(action))}</span>
      </span>
    </button>
  );
}

function launchLabelKey(action: LaunchActionKind | null): MessageKey {
  if (action === "launch_executable") return "detail.play";
  return action ? mediaActionMessageKey(action) : "library.setUp";
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
  if (isReaderAction(resume.action)) {
    return resumePlaceLabel(resume.action, resume.relativePath, resume.positionMs, resume.durationMs, t);
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
