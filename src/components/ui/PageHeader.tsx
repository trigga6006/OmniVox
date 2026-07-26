import { type ReactNode, type CSSProperties } from "react";

interface PageHeaderProps {
  title: string;
  subtitle?: string;
  /** Optional right-aligned action (button, controls). */
  action?: ReactNode;
  className?: string;
  style?: CSSProperties;
}

/** Canonical top-of-page header: a display title + muted subtitle, with an
 *  optional right-aligned action. Replaces the per-page copies whose title
 *  tracking had drifted (-0.02em vs -0.025em) and whose action rows mixed
 *  items-center with items-start. */
export function PageHeader({ title, subtitle, action, className, style }: PageHeaderProps) {
  const heading = (
    <div className="min-w-0">
      <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
        {title}
      </h1>
      {subtitle && <p className="mt-1 text-sm text-text-muted">{subtitle}</p>}
    </div>
  );

  if (!action) {
    return (
      <div className={className} style={style}>
        {heading}
      </div>
    );
  }

  return (
    <div className={className} style={style}>
      <div className="flex items-start justify-between gap-4">
        {heading}
        <div className="shrink-0">{action}</div>
      </div>
    </div>
  );
}
