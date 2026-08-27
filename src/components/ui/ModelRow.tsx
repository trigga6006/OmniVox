import { Fragment, type CSSProperties, type ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Card } from "./Card";
import { Progress } from "./Progress";

type Accent = "amber" | "violet" | "teal" | "green";

interface ModelRowProps {
  name: string;
  /** Badge chips rendered beside the name. */
  badges?: ReactNode;
  description?: string;
  /**
   * Mono meta segments, joined with " · " and set in tabular-nums —
   * `["1.5 GB", "q5_0", "~2.7 GB RAM"]`. Falsy entries are dropped so callers
   * can inline conditionals.
   */
  meta?: ReactNode[];
  /** Trailing slot: a Button, a status line, or both. */
  action?: ReactNode;
  /** 0–100 while downloading. Omit when idle. */
  progress?: number;
  /** Download/activation failure, shown under the meta line. */
  error?: string;
  /** Colour identity — drives the left rail and the progress fill. */
  accent?: Accent;
  /** Draws the left rail: `strong` for the active row, `soft` for a default/recommended one. */
  rail?: "strong" | "soft";
  /** Expandable slot below the row (test boxes, per-model settings). */
  footer?: ReactNode;
  className?: string;
  style?: CSSProperties;
}

const RAIL: Record<Accent, { strong: string; soft: string }> = {
  amber: { strong: "border-l-amber-500/70", soft: "border-l-amber-500/40" },
  violet: { strong: "border-l-violet-400/80", soft: "border-l-violet-500/45" },
  teal: { strong: "border-l-teal-400/80", soft: "border-l-teal-500/45" },
  green: { strong: "border-l-success/75", soft: "border-l-success/40" },
};

/** `green` has no Progress fill of its own — downloads are always tab-accented. */
const PROGRESS_ACCENT: Record<Accent, "amber" | "violet" | "teal"> = {
  amber: "amber",
  violet: "violet",
  teal: "teal",
  green: "amber",
};

/**
 * One model card row, shared by all four Models tabs.
 *
 * Speech / LLM / Command / Cleanup each grew their own copy of this recipe,
 * which is why they drifted into three different meta layouts and ~50
 * arbitrary text sizes. The shape here is the union of those four: identity
 * on the left, one mono meta line, one trailing action, optional progress and
 * an optional expandable footer.
 */
export function ModelRow({
  name,
  badges,
  description,
  meta,
  action,
  progress,
  error,
  accent = "amber",
  rail,
  footer,
  className,
  style,
}: ModelRowProps) {
  const segments = (meta ?? []).filter(Boolean);

  return (
    <Card
      className={cn(
        "flex flex-col rounded-[var(--radius-l)]",
        "transition-[border-color] duration-[var(--dur-2)] ease-out hover:border-border-hover",
        rail && `border-l-[3px] ${RAIL[accent][rail]}`,
        className
      )}
      style={style}
    >
      <div className="flex items-center gap-4 px-5 py-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-medium text-text-primary">{name}</span>
            {badges}
          </div>

          {description && (
            <p className="mt-0.5 line-clamp-1 text-xs leading-relaxed text-text-muted">
              {description}
            </p>
          )}

          {segments.length > 0 && (
            <p className="mt-1 font-mono text-2xs tabular-nums text-text-muted">
              {segments.map((s, i) => (
                <Fragment key={i}>
                  {i > 0 && <span className="mx-1.5 opacity-60">·</span>}
                  {s}
                </Fragment>
              ))}
            </p>
          )}

          {progress !== undefined && (
            <Progress
              value={progress}
              accent={PROGRESS_ACCENT[accent]}
              className="mt-2"
              aria-label={`Downloading ${name}`}
            />
          )}

          {error && <p className="mt-2 line-clamp-2 text-xs leading-snug text-error">{error}</p>}
        </div>

        {action && (
          <div className="flex shrink-0 items-center justify-end gap-1.5">{action}</div>
        )}
      </div>

      {footer && <div className="border-t border-border/60 px-5 py-3">{footer}</div>}
    </Card>
  );
}
