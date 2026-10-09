import { ActionMenu, type ActionMenuItem } from "../../app/ActionMenu";
import { usePresentation } from "../../preferences/PresentationProvider";
import type { ManageTab } from "./LibraryManageDrawer";

/** The "···" menu each Library work has, wherever the Library shows it. */
export function LibraryWorkMenu({
  title,
  className,
  onOpenWork,
  onManage,
  onBrowse,
}: {
  title: string;
  className?: string;
  onOpenWork?: () => void;
  onManage: (tab: ManageTab) => void;
  onBrowse?: () => void;
}) {
  const { t } = usePresentation();
  const items: ActionMenuItem[] = [
    ...(onOpenWork ? [{ label: t("workMenu.catalogPage"), onSelect: onOpenWork }] : []),
    { label: t("workMenu.edit"), onSelect: () => onManage("general") },
    ...(onBrowse ? [{ label: t("workMenu.browse"), onSelect: onBrowse }] : []),
    { label: t("workMenu.remove"), onSelect: () => onManage("files"), danger: true },
  ];
  return <ActionMenu label={t("library.moreFor", { title })} items={items} className={className} />;
}
