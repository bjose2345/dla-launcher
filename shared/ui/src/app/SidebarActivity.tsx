import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { ProgressBar } from "./ProgressBar";

/**
 * A job running in the sidebar while the rest of the app stays usable: an
 * icon, the work's title, a line or two of status and a slim progress bar.
 * The whole entry opens the page that owns the job.
 */
export function SidebarActivity({
  icon: Icon,
  title,
  label,
  percent,
  onOpen,
  children,
}: {
  icon: LucideIcon;
  title: string;
  label: string;
  percent: number | null;
  onOpen: () => void;
  children?: ReactNode;
}) {
  return (
    <button className="sidebar-activity" type="button" onClick={onOpen} aria-label={label}>
      <Icon className="sidebar-activity-icon" aria-hidden="true" />
      <span className="sidebar-activity-text">
        <strong>{title}</strong>
        {children}
      </span>
      <ProgressBar className="sidebar-activity-progress" percent={percent} size="slim" />
    </button>
  );
}
