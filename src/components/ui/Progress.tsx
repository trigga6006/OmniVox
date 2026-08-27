import { cn } from "@/lib/utils";

type Accent = "amber" | "violet" | "teal";

interface ProgressProps {
  /** 0–100. Omit for the indeterminate sweep. */
  value?: number;
  accent?: Accent;
  /** Applied to the track. Height defaults to h-1. */
  className?: string;
  "aria-label"?: string;
}

const FILL: Record<Accent, string> = {
  amber: "bg-amber-400",
  violet: "bg-violet-400",
  teal: "bg-teal-400",
};

/**
 * The one progress bar. Replaces five independent h-1 implementations across
 * Dictation / LLM models / Cleanup / Meetings / OpenRouter.
 *
 * Determinate mode animates `width` with a transition, not a keyframe: a
 * download that jumps 12% → 47% retargets mid-flight instead of restarting
 * the animation from zero.
 */
export function Progress({ value, accent = "amber", className, ...aria }: ProgressProps) {
  const indeterminate = value === undefined;
  const pct = indeterminate ? 0 : Math.min(100, Math.max(0, value));

  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(pct)}
      className={cn("h-1 w-full overflow-hidden rounded-full bg-surface-3", className)}
      {...aria}
    >
      {indeterminate ? (
        <div className={cn("h-full w-[35%] animate-progress-sweep rounded-full", FILL[accent])} />
      ) : (
        <div
          className={cn(
            "h-full rounded-full transition-[width] duration-[var(--dur-2)] ease-out",
            FILL[accent]
          )}
          style={{ width: `${pct}%` }}
        />
      )}
    </div>
  );
}
