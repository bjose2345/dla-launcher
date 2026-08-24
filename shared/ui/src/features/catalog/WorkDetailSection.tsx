import type { ReactNode } from "react";

interface WorkDetailSectionProps {
  title: string;
  count?: number;
  headingAccessory?: ReactNode;
  children: ReactNode;
}

export function WorkDetailSection({
  title,
  count,
  headingAccessory,
  children,
}: WorkDetailSectionProps) {
  return (
    <section className="work-detail-section">
      <details open>
        <summary>
          <div className="work-detail-section-heading">
            <h2>{title}{count !== undefined && count > 0 && <small>{count}</small>}</h2>
            {headingAccessory}
          </div>
          <span aria-hidden="true"><i /><i /></span>
        </summary>
        <div className="work-detail-section-body">{children}</div>
      </details>
    </section>
  );
}
