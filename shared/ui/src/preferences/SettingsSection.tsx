import type { ReactNode } from "react";

export function SettingsSection({
  className,
  icon,
  title,
  description,
  action,
  children,
}: {
  className?: string;
  icon: ReactNode;
  title: string;
  description: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className={`settings-section${className ? ` ${className}` : ""}`}>
      <div className="settings-section-head">
        <span className="settings-mark">{icon}</span>
        <div>
          <h2>{title}</h2>
          <p>{description}</p>
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}
