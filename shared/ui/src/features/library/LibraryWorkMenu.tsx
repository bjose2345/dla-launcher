import { ActionMenu, type ActionMenuItem } from "../../app/ActionMenu";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { ManageTab } from "./LibraryManageDrawer";

/** The "···" menu each Library work has, wherever the Library shows it. */
export function LibraryWorkMenu({
  title,
  className,
  onOpenWork,
  ownWork,
  onManage,
  onBrowse,
  playTime,
}: {
  title: string;
  className?: string;
  onOpenWork?: () => void;
  /** The work is outside the catalog, so its page is the Library's own. */
  ownWork?: boolean;
  onManage: (tab: ManageTab) => void;
  onBrowse?: () => void;
  /** Where the menu sits beside the work's play time: hides it everywhere, or starts it from nothing. */
  playTime?: { shown: boolean; onShow: (shown: boolean) => void; onReset?: () => void };
}) {
  const { t } = usePresentation();
  const items: ActionMenuItem[] = [
    ...(onOpenWork ? [{ label: t(ownWork ? "workMenu.workPage" : "workMenu.catalogPage"), onSelect: onOpenWork }] : []),
    { label: t("workMenu.edit"), onSelect: () => onManage("general") },
    ...(onBrowse ? [{ label: t("workMenu.browse"), onSelect: onBrowse }] : []),
    ...(playTime ? [{
      label: t(playTime.shown ? "workMenu.hidePlayTime" : "workMenu.showPlayTime"),
      onSelect: () => playTime.onShow(!playTime.shown),
    }] : []),
    ...(playTime?.onReset ? [{ label: t("workMenu.resetPlayTime"), onSelect: playTime.onReset, danger: true }] : []),
    { label: t("workMenu.remove"), onSelect: () => onManage("files"), danger: true },
  ];
  return <ActionMenu label={t("library.moreFor", { title })} items={items} className={className} />;
}
