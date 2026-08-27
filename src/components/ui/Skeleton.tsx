import { cn } from "@/lib/utils";

/**
 * Loading placeholder. Every page used to block behind one centered 16px
 * spinner and then pop in whole; a skeleton holds the shape of the content
 * so the swap costs no layout shift.
 *
 * The shimmer is a translating gradient inside an `overflow-hidden` block, so
 * the reduced-motion rule in globals.css parks it off-screen and leaves a flat
 * surface-2 tile — static, never a strobing tile.
 */
export function Skeleton({ className }: { className?: string }) {
  return (
    <div
      aria-hidden="true"
      className={cn(
        "relative overflow-hidden rounded-[var(--radius-s)] bg-surface-2",
        className
      )}
    >
      <span className="absolute inset-0 -translate-x-full animate-shimmer bg-gradient-to-r from-transparent via-surface-3 to-transparent" />
    </div>
  );
}

/**
 * `model` — a ModelRow-shaped placeholder: same Card chrome, same 3-line left
 * column and trailing action.
 * `text` — a History-row placeholder: one transcript line, `px-4 py-3.5`.
 *
 * The two are NOT interchangeable, which is what the one-size row got wrong:
 * measured against the real rows it stood 11 px short of a ModelRow (78 vs
 * 89.6) and 20 px tall for a History row (78 vs 58), so every list visibly
 * jumped as it loaded. Each variant's line boxes are sized to the text rows
 * they stand in for, so the swap is within a pixel.
 */
type SkeletonRowVariant = "model" | "text";

export function SkeletonRow({
  variant = "model",
  className,
}: {
  variant?: SkeletonRowVariant;
  className?: string;
}) {
  if (variant === "text") {
    return (
      <div
        aria-hidden="true"
        className={cn(
          "flex items-center gap-3 rounded-[var(--radius-l)] border border-border bg-surface-1 px-4 py-3.5",
          className
        )}
      >
        {/* The real row's height comes from its hover action row, not its
            text — 28 px, the same --control-h-s box. */}
        <div className="flex h-[var(--control-h-s)] min-w-0 flex-1 items-center">
          <Skeleton className="h-3.5 w-[72%]" />
        </div>
      </div>
    );
  }

  return (
    <div
      aria-hidden="true"
      className={cn(
        "flex items-center gap-4 rounded-[var(--radius-l)] border border-border bg-surface-1 px-5 py-3",
        className
      )}
    >
      {/* Line boxes, not bar heights, are what has to match: ModelRow's name /
          description / meta rows measure 21 px, 2 + 19.5 px and 4 + 17 px. */}
      <div className="min-w-0 flex-1">
        <div className="flex h-[21px] items-center">
          <Skeleton className="h-3.5 w-[38%]" />
        </div>
        <div className="mt-0.5 flex h-[19.5px] items-center">
          <Skeleton className="h-3 w-[64%]" />
        </div>
        <div className="mt-1 flex h-[17px] items-center">
          <Skeleton className="h-2.5 w-[46%]" />
        </div>
      </div>
      <Skeleton className="h-[var(--control-h-s)] w-[84px] rounded-[var(--radius-m)]" />
    </div>
  );
}

/** `count` stacked SkeletonRows — the shape most of our lists load into. */
export function SkeletonRows({
  count = 3,
  variant,
  className,
}: {
  count?: number;
  variant?: SkeletonRowVariant;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-col gap-2", className)}>
      {Array.from({ length: count }, (_, i) => (
        <SkeletonRow key={i} variant={variant} />
      ))}
    </div>
  );
}
