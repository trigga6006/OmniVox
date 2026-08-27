import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md" | "lg";

const VARIANTS: Record<Variant, string> = {
  primary:
    "bg-amber-500 text-[var(--primary-foreground)] font-semibold hover:bg-amber-400",
  secondary:
    "bg-surface-2 text-text-primary border border-border-hover hover:bg-surface-3",
  ghost: "bg-transparent text-text-secondary hover:bg-surface-2 hover:text-text-primary",
  danger: "bg-error text-white font-medium hover:brightness-110",
};

/** Heights come from the density tokens: 28 / 32 / 36. */
const SIZES: Record<Size, string> = {
  sm: "h-[var(--control-h-s)] px-3 text-xs gap-1.5",
  md: "h-[var(--control-h-m)] px-4 text-sm gap-2",
  lg: "h-[var(--control-h-l)] px-5 text-sm gap-2",
};

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
  /** Leading icon (a lucide element). Hidden while loading. */
  icon?: ReactNode;
}

/**
 * The one button. Variants map to the graphite·amber roles: primary = amber,
 * danger = red, secondary/ghost = neutral. Use this instead of inline buttons.
 *
 * Carries the global interaction standard: pressed scale(0.97) @ --dur-1,
 * named transition properties (never `all`), radius from --radius-m, and a
 * focus ring that follows that radius (globals.css `:focus-visible`).
 */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", loading, icon, className, children, disabled, ...props },
  ref
) {
  return (
    <button
      ref={ref}
      disabled={disabled || loading}
      className={cn(
        "pressable inline-flex select-none items-center justify-center whitespace-nowrap font-medium",
        "rounded-[var(--radius-m)]",
        "transition-[background-color,border-color,color,transform,opacity]",
        "duration-[var(--dur-2)] ease-out",
        "disabled:cursor-not-allowed disabled:opacity-45",
        "[&>svg]:size-[15px] [&>svg]:shrink-0",
        SIZES[size],
        VARIANTS[variant],
        className
      )}
      {...props}
    >
      {loading ? <Loader2 className="animate-spin" /> : icon}
      {children}
    </button>
  );
});
